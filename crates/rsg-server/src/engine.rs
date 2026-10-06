//! The per-request lifecycle driver. `Engine` is shared by every request;
//! `Engine::start` allocates a uid, spawns a driver task that owns that
//! uid's lifecycle end to end (encode -> register -> submit -> stream
//! tokens -> terminal), and returns an [`ActiveRequest`] handle for the
//! HTTP layer to await acceptance and then consume [`RequestEvent`]s from.
//!
//! Plan 05-02 (Task 2, this same plan) adds the `fsm` registry parameter
//! and reports every transition to it. Plan 05-04 adds cancellation
//! (acting on the [`AbortGuard`]'s token), abort timing, the backend
//! timeout and the overlong-prompt check on top of this happy path.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::{CancellationToken, DropGuard};

use rsg_wire::{SamplingParams, Tensor};

use crate::codec::{CodecError, Prompt, TextCodec};
use crate::dispatch::{DispatchHandle, DispatchStatsSnapshot, UidEvent};
use crate::writer::{WriterClosed, WriterHandle};

/// Default backend-unresponsive timeout (LIFE-04), in milliseconds. Plan
/// 05-04 wires this into the driver; this plan only defines the constant
/// and the config field it initializes.
pub const DEFAULT_BACKEND_TIMEOUT_MS: u64 = 60_000;

/// Server-wide abort-timing mode (LIFE-05, CONTEXT D-01): `Immediate`
/// (default) aborts as soon as a disconnect is noticed; `Deferred` waits
/// for the first token. Plan 05-04 wires this into the driver; this plan
/// only defines the enum and the config field it initializes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum AbortTiming {
    #[default]
    Immediate,
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
/// [`ActiveRequest`]) cancels the request's [`CancellationToken`]. Plan
/// 05-04 makes the driver act on that cancellation; this plan only wires
/// the guard through so later plans have it in place. `AbortGuard` is
/// `Send` and exposes no public methods beyond construction — the project's
/// minimal-wrapper-type convention (see `transport.rs`'s split halves).
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
    config: EngineConfig,
    next_uid: AtomicI64,
}

impl Engine {
    pub fn new(
        writer: WriterHandle,
        dispatch: DispatchHandle,
        codec: Arc<dyn TextCodec>,
        config: EngineConfig,
    ) -> Arc<Engine> {
        Arc::new(Engine {
            writer,
            dispatch,
            codec,
            config,
            next_uid: AtomicI64::new(0),
        })
    }

    pub fn dispatch_stats(&self) -> DispatchStatsSnapshot {
        self.dispatch.stats()
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
        let guard = AbortGuard {
            _guard: token.clone().drop_guard(),
        };

        let engine = Arc::clone(self);
        tokio::spawn(drive_request(engine, uid, prompt, params, tx));

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

/// The per-request driver's happy path: encode -> register -> submit ->
/// stream tokens through the codec's decoder -> exactly one terminal
/// event. Every exit path below sends exactly one terminal
/// (`Rejected`/`Failed`) event, or returns after the finished `Token`
/// event — see the module doc for what later plans add on top.
async fn drive_request(
    engine: Arc<Engine>,
    uid: i64,
    prompt: Prompt,
    params: SamplingParams,
    tx: mpsc::UnboundedSender<RequestEvent>,
) {
    let input_ids = match engine.codec.encode(&prompt) {
        Ok(ids) => ids,
        Err(CodecError(msg)) => {
            tracing::warn!(uid, "codec encode failed");
            send_event(&tx, RequestEvent::Rejected(SubmitError::Codec(msg)));
            return;
        }
    };
    let input_len = input_ids.len();
    let max_tokens = params.max_tokens;

    // Register before submitting (Phase 3 caller contract): a reply could
    // otherwise arrive before this uid has a route.
    let mut stream = engine.dispatch.register(uid);

    let submitted = match engine
        .writer
        .submit(uid, Tensor::from_i32_slice(&input_ids), params)
        .await
    {
        Ok(ticket) => ticket,
        Err(WriterClosed) => {
            engine.dispatch.deregister(uid);
            tracing::warn!(uid, "writer closed before submit");
            send_event(
                &tx,
                RequestEvent::Rejected(SubmitError::BackendUnavailable),
            );
            return;
        }
    };

    tracing::debug!(uid, input_len, max_tokens, "request accepted");
    send_event(&tx, RequestEvent::Accepted);
    let mut decoder = engine.codec.decoder();

    loop {
        match stream.recv().await {
            Some(UidEvent::Token(reply)) => match decoder.step(reply.next_token, reply.finished) {
                Ok(text) => {
                    send_event(
                        &tx,
                        RequestEvent::Token {
                            text,
                            finished: reply.finished,
                        },
                    );
                    if reply.finished {
                        tracing::debug!(uid, "request finished");
                        return;
                    }
                }
                Err(CodecError(msg)) => {
                    engine.dispatch.deregister(uid);
                    let _ = engine.writer.abort(&submitted).await;
                    tracing::warn!(uid, "decode failed");
                    send_event(&tx, RequestEvent::Failed(RequestError::Decode(msg)));
                    return;
                }
            },
            Some(UidEvent::Dropped(n)) => {
                // A gap would corrupt the decoded text, so the request
                // fails instead of streaming wrong output.
                engine.dispatch.deregister(uid);
                let _ = engine.writer.abort(&submitted).await;
                tracing::warn!(uid, dropped = n, "slow consumer, failing request");
                send_event(
                    &tx,
                    RequestEvent::Failed(RequestError::SlowConsumer { dropped: n }),
                );
                return;
            }
            None => {
                tracing::warn!(uid, "backend gone");
                send_event(&tx, RequestEvent::Failed(RequestError::BackendGone));
                return;
            }
        }
    }
}
