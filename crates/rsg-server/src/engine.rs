//! The per-request lifecycle driver. `Engine` is shared by every request;
//! `Engine::start` allocates a uid, spawns a driver task that owns that
//! uid's lifecycle end to end (encode -> register -> submit -> stream
//! tokens -> terminal), and returns an [`ActiveRequest`] handle for the
//! HTTP layer to await acceptance and then consume [`RequestEvent`]s from.
//!
//! The driver observes cancellation (the [`AbortGuard`]'s
//! [`CancellationToken`]) with a `biased` `tokio::select!`: the
//! cancellation branch always goes first, so an abort reaches the backend
//! promptly even while tokens are streaming (LIFE-02). Plan 05-04's later
//! tasks add abort-timing (`Immediate`/`Deferred`, CONTEXT D-01), a
//! per-request backend-inactivity timeout, and the overlong-prompt check
//! on top of this.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_util::sync::{CancellationToken, DropGuard};

use rsg_wire::{SamplingParams, Tensor};

use crate::codec::{CodecError, Prompt, TextCodec};
use crate::dispatch::{DispatchHandle, DispatchStatsSnapshot, UidEvent, UidStream};
use crate::fsm::RegistryHandle;
use crate::fsm::state::LifecycleState;
use crate::writer::{Submitted, WriterClosed, WriterHandle};

/// Default backend-unresponsive timeout (LIFE-04), in milliseconds. A
/// later task in this plan wires this into the driver; this constant and
/// the config field it initializes are already in place from plan 05-01.
pub const DEFAULT_BACKEND_TIMEOUT_MS: u64 = 60_000;

/// Server-wide abort-timing mode (LIFE-05, CONTEXT D-01): `Immediate`
/// aborts as soon as a disconnect is noticed; `Deferred` (default since
/// Phase 6 D-09 -- see `main.rs`'s `--abort-timing` doc comment and
/// `docs/benchmarks/parity-report.md`'s "Abort-timing decision (D-09)")
/// waits for the first token.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum AbortTiming {
    Immediate,
    #[default]
    Deferred,
}

/// Per-server engine configuration, built once at startup from the
/// readiness handshake and CLI flags.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// The readiness handshake's `max_seq_len` (LIFE-04's overlong-prompt bound).
    pub max_seq_len: u64,
    pub abort_timing: AbortTiming,
    pub backend_timeout: Duration,
}

/// Why a request was rejected before ever producing a token.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SubmitError {
    #[error("prompt is {input_len} tokens, exceeding max_seq_len {max_seq_len}")]
    PromptTooLong { input_len: usize, max_seq_len: u64 },
    #[error("codec error: {0}")]
    Codec(String),
    #[error("backend unavailable")]
    BackendUnavailable,
}

/// Why an already-accepted request failed before reaching a normal finish.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RequestError {
    #[error("backend did not respond within {timeout_ms}ms")]
    BackendTimeout { timeout_ms: u64 },
    #[error("backend connection closed")]
    BackendGone,
    #[error("slow consumer: {dropped} tokens dropped")]
    SlowConsumer { dropped: u64 },
    #[error("decode error: {0}")]
    Decode(String),
}

/// One event in a request's lifecycle, sent from the driver task to its
/// [`ActiveRequest`]/response-body consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestEvent {
    Accepted,
    Rejected(SubmitError),
    Token { text: String, finished: bool },
    Failed(RequestError),
}

/// A `Drop`-based cancellation hook (CONTEXT Pattern A). Wraps a
/// [`tokio_util::sync::DropGuard`]: dropping whatever owns this guard (the
/// response body stream, or the handler future that holds an
/// [`ActiveRequest`]) cancels the request's [`CancellationToken`]. The
/// driver observes the cancellation with a `biased` `tokio::select!` (the
/// cancellation branch always checked first) and turns it into an abort on
/// the backend. `AbortGuard` is `Send` and exposes no public methods beyond
/// construction — the project's minimal-wrapper-type convention (see
/// `transport.rs`'s split halves).
///
/// **D-03 (accepted limitation):** hyper only notices a closed connection
/// when it next tries to write to it (or otherwise polls the connection).
/// A request that is still queued — nothing written yet, e.g. still in
/// prefill — or a non-streaming request — nothing written until the whole
/// response is ready — may not have its guard dropped until that next
/// write, or until the request completes. This is a documented limitation,
/// not patched with liveness probing (CONTEXT D-03). The actual bound is
/// measured, not eliminated, by
/// `http_cancellation::queued_stream_disconnect_abort_bound` (plan 05-04)
/// and `http_nonstream::tracer_nonstream_disconnect_reaches_one_terminal_state`
/// (plan 05-07).
pub struct AbortGuard {
    _guard: DropGuard,
}

/// The HTTP layer's handle onto one in-flight request: the uid the engine
/// allocated for it, and the event stream the driver task feeds.
pub struct ActiveRequest {
    uid: i64,
    events: mpsc::UnboundedReceiver<RequestEvent>,
    guard: AbortGuard,
}

impl ActiveRequest {
    pub fn uid(&self) -> i64 {
        self.uid
    }

    /// Awaits the first event: [`RequestEvent::Accepted`] is `Ok(())`,
    /// [`RequestEvent::Rejected`] is `Err(e)`, and a closed channel (the
    /// driver task ended before sending anything) is
    /// `Err(SubmitError::BackendUnavailable)`. Any other first event would
    /// be a bug in the driver, not a condition this caller can act on
    /// differently, so it is also reported as `Err(BackendUnavailable)`
    /// (logged at error) rather than panicking.
    pub async fn accepted(&mut self) -> Result<(), SubmitError> {
        match self.events.recv().await {
            Some(RequestEvent::Accepted) => Ok(()),
            Some(RequestEvent::Rejected(e)) => Err(e),
            Some(other) => {
                tracing::error!(
                    uid = self.uid,
                    ?other,
                    "driver's first event was neither Accepted nor Rejected"
                );
                Err(SubmitError::BackendUnavailable)
            }
            None => Err(SubmitError::BackendUnavailable),
        }
    }

    /// Awaits the next event after acceptance. `None` once the driver has
    /// ended (it always sends exactly one terminal event first).
    pub async fn next_event(&mut self) -> Option<RequestEvent> {
        self.events.recv().await
    }

    /// Splits into (uid, the event receiver, the abort guard), so a
    /// response-body stream can drive the receiver directly while keeping
    /// the guard alive for exactly as long as the body.
    pub fn into_parts(self) -> (i64, mpsc::UnboundedReceiver<RequestEvent>, AbortGuard) {
        (self.uid, self.events, self.guard)
    }
}

/// Shared per-server state the HTTP layer constructs once at startup.
pub struct Engine {
    writer: WriterHandle,
    dispatch: DispatchHandle,
    codec: Arc<dyn TextCodec>,
    registry: RegistryHandle,
    config: EngineConfig,
    next_uid: AtomicI64,
}

impl Engine {
    pub fn new(
        writer: WriterHandle,
        dispatch: DispatchHandle,
        codec: Arc<dyn TextCodec>,
        registry: RegistryHandle,
        config: EngineConfig,
    ) -> Arc<Engine> {
        Arc::new(Engine {
            writer,
            dispatch,
            codec,
            registry,
            config,
            next_uid: AtomicI64::new(0),
        })
    }

    pub fn dispatch_stats(&self) -> DispatchStatsSnapshot {
        self.dispatch.stats()
    }

    pub fn registry(&self) -> &RegistryHandle {
        &self.registry
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Allocates a uid from one counter shared by every generation
    /// endpoint (matching upstream `FrontendManager.new_user` — no request
    /// field can choose or influence it), spawns this request's driver
    /// task, and returns a handle for awaiting acceptance and consuming its
    /// events.
    pub fn start(self: &Arc<Self>, prompt: Prompt, params: SamplingParams) -> ActiveRequest {
        let uid = self.next_uid.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::unbounded_channel();
        let token = CancellationToken::new();
        let driver_token = token.clone();
        let guard = AbortGuard {
            _guard: token.drop_guard(),
        };

        let engine = Arc::clone(self);
        tokio::spawn(drive_request(engine, uid, prompt, params, tx, driver_token));

        ActiveRequest {
            uid,
            events: rx,
            guard,
        }
    }
}

/// Sends `event`, ignoring a closed channel (the receiver — the HTTP
/// handler's `ActiveRequest` or response-body stream — may already be
/// gone, e.g. the client disconnected).
fn send_event(tx: &mpsc::UnboundedSender<RequestEvent>, event: RequestEvent) {
    let _ = tx.send(event);
}

/// Reports `to` to the registry and sends `event`, in that order, so every
/// driver exit path reports exactly one terminal through this one helper
/// (LIFE-01).
fn finish(
    engine: &Engine,
    tx: &mpsc::UnboundedSender<RequestEvent>,
    uid: i64,
    to: LifecycleState,
    event: RequestEvent,
) {
    engine.registry.report(uid, to);
    send_event(tx, event);
}

/// Reports `to` (always a terminal state) to the registry without sending
/// any event. Used only for cancellation paths: the HTTP layer's event
/// receiver and `AbortGuard` only disappear because something dropped
/// them, so nobody is listening on `tx` by construction — sending an
/// event there would be a silently-ignored no-op anyway, but this makes
/// that explicit rather than inventing an event nobody consumes.
fn finish_silent(engine: &Engine, uid: i64, to: LifecycleState) {
    engine.registry.report(uid, to);
}

/// Deregisters `uid`'s route, then sends an `AbortBackendMsg` through the
/// single ordered writer using `submitted`'s ticket. Deregistering first
/// means every reply that arrives after this point is counted by the
/// dispatcher as `unknown_uid`/`closed_route` (LIFE-02's "dropped and
/// counted"); going through the ticket means this abort can never overtake
/// its own submit (WIRE-03). A `WriterClosed` here is logged at warn — the
/// request still ends Cancelled.
async fn abort_now(engine: &Engine, uid: i64, submitted: &Submitted) {
    engine.dispatch.deregister(uid);
    if let Err(WriterClosed) = engine.writer.abort(submitted).await {
        tracing::warn!(uid, "writer closed while sending abort");
    }
}

/// Handles a cancellation observed once a submit ticket exists (CONTEXT
/// D-01). In `Immediate` mode, or once a first token has already been seen
/// (`first_token_seen`), aborts right away. In `Deferred` mode with no
/// first token yet, waits instead (see [`deferred_wait`]).
async fn cancel_after_submit(
    engine: &Engine,
    uid: i64,
    submitted: &Submitted,
    stream: &mut UidStream,
    first_token_seen: bool,
) {
    if engine.config.abort_timing == AbortTiming::Immediate || first_token_seen {
        abort_now(engine, uid, submitted).await;
    } else {
        deferred_wait(engine, uid, submitted, stream).await;
    }
}

/// `Deferred` mode's wait for the first token (CONTEXT D-01): a
/// cancellation observed after submit but before any first token enters
/// this wait instead of aborting immediately.
///
/// - The first `Token`: if it is already `finished`, the backend already
///   completed and the dispatcher already removed the route, so no abort
///   is needed. Otherwise the backend is actively producing, so abort now.
/// - A `Dropped` lag event: the backend is producing (we just fell behind
///   reading it), so abort now.
/// - The deadline expiring first: a backend silent for `backend_timeout`
///   is already treated as unresponsive, so abort anyway.
/// - The stream ending on its own: nothing to abort.
///
/// All four outcomes end Cancelled, never Decoding -> Cancelled: the
/// client never received any token during this wait, so the transition is
/// always Submitted -> Cancelled (the caller does the actual
/// `finish_silent` call after this returns).
async fn deferred_wait(engine: &Engine, uid: i64, submitted: &Submitted, stream: &mut UidStream) {
    let deadline = Instant::now() + engine.config.backend_timeout;
    tokio::select! {
        event = stream.recv() => {
            match event {
                Some(UidEvent::Token(reply)) if reply.finished => {
                    tracing::debug!(uid, "deferred cancel observed the finished token; no abort needed");
                }
                Some(UidEvent::Token(_)) | Some(UidEvent::Dropped(_)) => {
                    abort_now(engine, uid, submitted).await;
                }
                None => {}
            }
        }
        _ = tokio::time::sleep_until(deadline) => {
            abort_now(engine, uid, submitted).await;
        }
    }
}

/// The per-request driver's happy path (plus cancellation): encode ->
/// register -> submit -> stream tokens through the codec's decoder ->
/// exactly one terminal event. Every exit path below reports exactly one
/// terminal state to the registry, and sends exactly one terminal
/// (`Rejected`/`Failed`) event or none at all (a cancellation nobody is
/// listening for — see [`finish_silent`]), or returns after the finished
/// `Token` event. See the module doc for what later tasks in this plan add
/// on top (abort timing, the backend timeout, the overlong check).
async fn drive_request(
    engine: Arc<Engine>,
    uid: i64,
    prompt: Prompt,
    params: SamplingParams,
    tx: mpsc::UnboundedSender<RequestEvent>,
    cancel: CancellationToken,
) {
    let config = &engine.config;
    engine.registry.report(uid, LifecycleState::Received);
    engine.registry.report(uid, LifecycleState::Tokenizing);

    let input_ids = match engine.codec.encode(&prompt) {
        Ok(ids) => ids,
        Err(CodecError(msg)) => {
            tracing::warn!(uid, "codec encode failed");
            finish(
                &engine,
                &tx,
                uid,
                LifecycleState::Failed,
                RequestEvent::Rejected(SubmitError::Codec(msg)),
            );
            return;
        }
    };
    let input_len = input_ids.len();
    let max_tokens = params.max_tokens;

    // LIFE-04: mirrors the scheduler's own silent-drop rule
    // (scheduler.py:177-188) — every prompt the backend would drop gets a
    // 400 instead, and nothing shorter is rejected. No register, submit or
    // abort for a rejected prompt. The limit comes only from the
    // handshake's max_seq_len, never a hardcoded constant.
    if input_len as u64 >= config.max_seq_len {
        tracing::warn!(
            uid,
            input_len,
            max_seq_len = config.max_seq_len,
            "prompt too long"
        );
        finish(
            &engine,
            &tx,
            uid,
            LifecycleState::Failed,
            RequestEvent::Rejected(SubmitError::PromptTooLong {
                input_len,
                max_seq_len: config.max_seq_len,
            }),
        );
        return;
    }

    // A cancellation observed before anything was ever sent to the backend
    // needs no abort at all: there is no ticket to abort with.
    if cancel.is_cancelled() {
        tracing::debug!(uid, "cancelled before submit; nothing sent to backend");
        finish_silent(&engine, uid, LifecycleState::Cancelled);
        return;
    }

    // Built before register/submit, not after (T-05-XX / Rule 1 fix): on
    // the real tokenizer, `decoder()` clones the whole vocab/merge table
    // (`tokenizers::Tokenizer`, ~150k entries for Qwen3-0.6B) -- tens of
    // milliseconds, not free. Building it *after* submit left a window
    // where the backend (especially a zero-decode-delay mock) could
    // already be streaming tokens into the per-uid broadcast channel
    // (fixed capacity, drop-oldest, D-07 -- see dispatch::UID_CHANNEL_CAPACITY)
    // while this task was still busy cloning, overflowing the buffer for a
    // long enough response before the decode loop ever called its first
    // `recv()`. The
    // clone itself doesn't depend on backend state, only on `engine.codec`,
    // so there is no ordering reason to delay it past encode.
    //
    // Run the clone on `spawn_blocking` (code review WR-01, confirmed on CI:
    // a synchronous clone with no `.await` runs on a shared tokio worker
    // thread and can starve *other* in-flight requests' own `stream.recv()`
    // polls long enough to overflow their own 16-entry buffer -- the same
    // failure class the ordering fix above addresses for this request's own
    // stream, just inflicted on a sibling instead. `spawn_blocking` moves the
    // clone to tokio's dedicated blocking pool, so the ordering guarantee
    // above still holds (this `.await` completes before register/submit)
    // but no async worker thread is blocked while it runs.
    let codec_for_decoder = engine.codec.clone();
    let mut decoder = match tokio::task::spawn_blocking(move || codec_for_decoder.decoder()).await {
        Ok(decoder) => decoder,
        Err(join_err) => {
            tracing::warn!(uid, %join_err, "decoder construction panicked");
            finish(
                &engine,
                &tx,
                uid,
                LifecycleState::Failed,
                RequestEvent::Rejected(SubmitError::Codec(format!(
                    "decoder construction panicked: {join_err}"
                ))),
            );
            return;
        }
    };

    // Register before submitting (Phase 3 caller contract): a reply could
    // otherwise arrive before this uid has a route.
    let mut stream = engine.dispatch.register(uid);

    // Do not race the submit await itself against cancellation: tokio's
    // bounded send either enqueues or not, and checking right after keeps
    // the ticket logic simple (CONTEXT D-01).
    let submitted = match engine
        .writer
        .submit(uid, Tensor::from_i32_slice(&input_ids), params)
        .await
    {
        Ok(ticket) => ticket,
        Err(WriterClosed) => {
            engine.dispatch.deregister(uid);
            tracing::warn!(uid, "writer closed before submit");
            finish(
                &engine,
                &tx,
                uid,
                LifecycleState::Failed,
                RequestEvent::Rejected(SubmitError::BackendUnavailable),
            );
            return;
        }
    };
    engine.registry.report(uid, LifecycleState::Submitted);

    if cancel.is_cancelled() {
        cancel_after_submit(&engine, uid, &submitted, &mut stream, false).await;
        finish_silent(&engine, uid, LifecycleState::Cancelled);
        return;
    }

    tracing::debug!(uid, input_len, max_tokens, "request accepted");
    send_event(&tx, RequestEvent::Accepted);
    let mut reported_decoding = false;

    // LIFE-04: armed here, re-armed after every Token; a backend silent
    // longer than this is treated as unresponsive.
    let sleep = tokio::time::sleep_until(Instant::now() + config.backend_timeout);
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                cancel_after_submit(&engine, uid, &submitted, &mut stream, reported_decoding).await;
                finish_silent(&engine, uid, LifecycleState::Cancelled);
                return;
            }
            _ = &mut sleep => {
                engine.dispatch.deregister(uid);
                if let Err(WriterClosed) = engine.writer.abort(&submitted).await {
                    tracing::warn!(uid, "writer closed while sending timeout abort");
                }
                let timeout_ms = config.backend_timeout.as_millis() as u64;
                tracing::warn!(uid, timeout_ms, "backend inactivity timeout");
                finish(
                    &engine,
                    &tx,
                    uid,
                    LifecycleState::Failed,
                    RequestEvent::Failed(RequestError::BackendTimeout { timeout_ms }),
                );
                return;
            }
            event = stream.recv() => {
                match event {
                    Some(UidEvent::Token(reply)) => {
                        sleep.as_mut().reset(Instant::now() + config.backend_timeout);
                        if !reported_decoding {
                            engine.registry.report(uid, LifecycleState::Decoding);
                            reported_decoding = true;
                        }
                        match decoder.step(reply.next_token, reply.finished) {
                            Ok(text) => {
                                if reply.finished {
                                    tracing::debug!(uid, "request finished");
                                    finish(
                                        &engine,
                                        &tx,
                                        uid,
                                        LifecycleState::Finished,
                                        RequestEvent::Token {
                                            text,
                                            finished: true,
                                        },
                                    );
                                    return;
                                }
                                send_event(
                                    &tx,
                                    RequestEvent::Token {
                                        text,
                                        finished: false,
                                    },
                                );
                            }
                            Err(CodecError(msg)) => {
                                engine.dispatch.deregister(uid);
                                let _ = engine.writer.abort(&submitted).await;
                                tracing::warn!(uid, "decode failed");
                                finish(
                                    &engine,
                                    &tx,
                                    uid,
                                    LifecycleState::Failed,
                                    RequestEvent::Failed(RequestError::Decode(msg)),
                                );
                                return;
                            }
                        }
                    }
                    Some(UidEvent::Dropped(n)) => {
                        // A gap would corrupt the decoded text, so the
                        // request fails instead of streaming wrong output.
                        engine.dispatch.deregister(uid);
                        let _ = engine.writer.abort(&submitted).await;
                        tracing::warn!(uid, dropped = n, "slow consumer, failing request");
                        finish(
                            &engine,
                            &tx,
                            uid,
                            LifecycleState::Failed,
                            RequestEvent::Failed(RequestError::SlowConsumer { dropped: n }),
                        );
                        return;
                    }
                    None => {
                        tracing::warn!(uid, "backend gone");
                        finish(
                            &engine,
                            &tx,
                            uid,
                            LifecycleState::Failed,
                            RequestEvent::Failed(RequestError::BackendGone),
                        );
                        return;
                    }
                }
            }
        }
    }
}
