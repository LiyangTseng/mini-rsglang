//! The per-uid reply dispatcher (D-04/D-05/D-07). One dedicated OS thread,
//! named `rx-zmq`, exclusively owns the detokenizer PULL half of the
//! transport (`DetokSource`) and routes every `DetokenizeMsg` to the
//! broadcast channel registered for its `uid`.
//!
//! Each uid's channel has a fixed capacity [`UID_CHANNEL_CAPACITY`] — a
//! power of two, so tokio's internal capacity rounding leaves it at exactly
//! 16 (D-07: a fixed constant, no CLI or config knob). The dispatcher's
//! `send` never blocks: when a uid's channel is full, the oldest buffered
//! token is overwritten, and the drop is visible only to that uid's own
//! consumer, on its next `recv()`, as `UidEvent::Dropped(n)` (RESEARCH.md
//! Pitfall 4 — the drop point and the send point are on opposite ends of
//! the channel by design). A `DetokenizeMsg` for a uid with no registered
//! route is dropped silently (D-04) — the normal case for late tokens after
//! an abort.
//!
//! Caller contract: `register(uid)` before `submit(uid)`. A route is
//! removed automatically once its finished reply has been routed, or once
//! its `UidStream` has been dropped (the broadcast `Sender`'s next `send`
//! then errs and the dispatcher removes the entry).
//!
//! The drop itself is visible only to the consumer, never to the
//! dispatcher's own `send()` call: `broadcast::Sender::send` returns `Ok`
//! even when it overwrites an unread value (RESEARCH.md Pitfall 4 — the
//! drop point and the send point are on opposite ends of the channel by
//! design). Every drop is still signaled three ways, all on the consumer
//! side, in [`UidStream::recv`]: an in-band [`UidEvent::Dropped(n)`], the
//! per-uid [`UidStream::dropped`] running total, and a `tracing::warn!`
//! with `uid`, `dropped` and `total_dropped` fields (D-06) — so a gap in a
//! uid's token stream is never silent.
//!
//! [`DispatchHandle::stats`] exposes four lock-free `AtomicU64` counters,
//! updated on the `rx-zmq` thread with `Ordering::Relaxed` (they are
//! independent counts, never used to synchronize any other access):
//! `routed` (a successful send, counted per `DetokenizeMsg` entry),
//! `unknown_uid` (a reply for a uid with no route — never registered,
//! already deregistered, or already finished; logged at `debug`, not
//! `warn`, because a mass cancellation can produce thousands of these and
//! the counter, not the log, is the signal), `closed_route` (a send that
//! failed because the uid's `UidStream` was dropped — the route is removed
//! immediately), and `malformed_frames` (an undecodable frame, counted per
//! frame, which keeps its existing `tracing::warn!` since these should be
//! rare). [`DispatchHandle::deregister`] removes a uid's route explicitly,
//! synchronously enqueued and never blocking; re-registering a uid
//! replaces its route outright, dropping the old `Sender` and ending the
//! old `UidStream` after whatever it had already buffered.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rustc_hash::FxHashMap;
use tokio::sync::broadcast;
use tokio::sync::mpsc::error::TryRecvError;

use crate::transport::DetokSource;

/// Fixed per-uid channel capacity (D-07). Already a power of two, so
/// tokio's capacity rounding is a no-op.
pub const UID_CHANNEL_CAPACITY: usize = 16;

/// How long the dispatcher thread blocks on one `recv_detok` poll before
/// looping back to drain its control channel again.
pub const DISPATCH_POLL_MS: i64 = 50;

/// One token reply for one uid, decoded off the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenReply {
    pub uid: i64,
    pub next_token: i64,
    pub finished: bool,
}

/// What a [`UidStream`] yields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UidEvent {
    Token(TokenReply),
    /// The consumer fell behind: `n` tokens were overwritten before being read.
    Dropped(u64),
}

/// Registration messages sent to the dispatcher thread.
enum Control {
    Register(i64, broadcast::Sender<TokenReply>),
    Deregister(i64),
}

/// Lock-free routing counters shared between [`DispatchHandle`] and the
/// `rx-zmq` thread. See the module doc for what increments each one.
#[derive(Default)]
struct DispatchStats {
    routed: AtomicU64,
    unknown_uid: AtomicU64,
    closed_route: AtomicU64,
    malformed_frames: AtomicU64,
}

/// A point-in-time snapshot of [`DispatchHandle::stats`]. Every count is
/// per `DetokenizeMsg` entry except `malformed_frames`, which counts
/// frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DispatchStatsSnapshot {
    pub routed: u64,
    pub unknown_uid: u64,
    pub closed_route: u64,
    pub malformed_frames: u64,
}

/// A cheap, cloneable handle used to register new uids with the dispatcher.
#[derive(Clone)]
pub struct DispatchHandle {
    tx: tokio::sync::mpsc::UnboundedSender<Control>,
    stats: Arc<DispatchStats>,
}

impl DispatchHandle {
    /// Registers `uid` and returns its stream. Never blocks. If the
    /// dispatcher thread has already stopped, the returned stream still
    /// works correctly: its own `Sender` (held only by this call) is
    /// dropped immediately after, so the very first `recv()` sees
    /// `RecvError::Closed` and yields `None`.
    ///
    /// Registering a uid that is already registered replaces its route:
    /// the map entry's old `Sender` is dropped, so the earlier
    /// `UidStream` ends (after yielding whatever it had already
    /// buffered) and every new reply goes to the new stream.
    pub fn register(&self, uid: i64) -> UidStream {
        let (tx, rx) = broadcast::channel(UID_CHANNEL_CAPACITY);
        let _ = self.tx.send(Control::Register(uid, tx));
        UidStream {
            uid,
            rx,
            dropped: 0,
        }
    }

    /// Removes `uid`'s route, if any. Never blocks — the request is
    /// enqueued on the same control channel `register` uses, and is
    /// applied before the dispatcher routes its next frame. The uid's
    /// `UidStream` (if any is still held) ends after whatever it had
    /// already buffered; any reply for `uid` that arrives after this call
    /// is counted in `unknown_uid`, exactly like a reply for a uid that
    /// was never registered.
    pub fn deregister(&self, uid: i64) {
        let _ = self.tx.send(Control::Deregister(uid));
    }

    /// A point-in-time snapshot of the dispatcher's routing counters.
    pub fn stats(&self) -> DispatchStatsSnapshot {
        DispatchStatsSnapshot {
            routed: self.stats.routed.load(Ordering::Relaxed),
            unknown_uid: self.stats.unknown_uid.load(Ordering::Relaxed),
            closed_route: self.stats.closed_route.load(Ordering::Relaxed),
            malformed_frames: self.stats.malformed_frames.load(Ordering::Relaxed),
        }
    }
}

/// The per-request receiving half of a uid's registered route.
pub struct UidStream {
    uid: i64,
    rx: broadcast::Receiver<TokenReply>,
    dropped: u64,
}

impl UidStream {
    pub fn uid(&self) -> i64 {
        self.uid
    }

    /// The total number of tokens dropped for this uid so far (D-06): the
    /// running sum of every `Lagged(n)` this stream has observed.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Awaits the next event for this uid. `None` once the route has
    /// closed: the dispatcher removed it after a finished reply, or the
    /// dispatcher thread itself stopped.
    pub async fn recv(&mut self) -> Option<UidEvent> {
        match self.rx.recv().await {
            Ok(t) => Some(UidEvent::Token(t)),
            Err(broadcast::error::RecvError::Lagged(n)) => {
                self.dropped += n;
                tracing::warn!(
                    uid = self.uid,
                    dropped = n,
                    total_dropped = self.dropped,
                    "slow consumer: dropped oldest buffered tokens"
                );
                Some(UidEvent::Dropped(n))
            }
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }
}

/// Spawns the `rx-zmq` thread owning `source`, and returns a handle for
/// registering new uids. The thread runs until every [`DispatchHandle`]
/// clone is dropped (its control channel disconnects) or `recv_detok` errs,
/// at which point every live route's `Sender` is dropped, ending every
/// live `UidStream`.
pub fn spawn_dispatcher<S: DetokSource + 'static>(source: S) -> std::io::Result<DispatchHandle> {
    let (ctrl_tx, mut ctrl_rx) = tokio::sync::mpsc::unbounded_channel::<Control>();
    let stats = Arc::new(DispatchStats::default());
    let thread_stats = Arc::clone(&stats);
    std::thread::Builder::new()
        .name("rx-zmq".to_owned())
        .spawn(move || {
            let mut routes: FxHashMap<i64, broadcast::Sender<TokenReply>> = FxHashMap::default();
            loop {
                if !drain_control(&mut ctrl_rx, &mut routes) {
                    tracing::debug!("rx-zmq: every DispatchHandle dropped, stopping");
                    return;
                }

                let frame = match source.recv_detok(DISPATCH_POLL_MS) {
                    Ok(Some(frame)) => frame,
                    Ok(None) => continue,
                    Err(e) => {
                        tracing::error!("rx-zmq: {e:#}");
                        return;
                    }
                };

                // A uid's register() may have completed after the poll
                // started but before this frame's first reply for it needs
                // to route — drain again before decoding.
                if !drain_control(&mut ctrl_rx, &mut routes) {
                    tracing::debug!("rx-zmq: every DispatchHandle dropped, stopping");
                    return;
                }

                match rsg_wire::decode_tokenizer(&frame) {
                    Ok(msg) => route_message(&mut routes, msg, &thread_stats),
                    Err(e) => {
                        thread_stats
                            .malformed_frames
                            .fetch_add(1, Ordering::Relaxed);
                        tracing::warn!("rx-zmq: dropping undecodable frame: {e}");
                    }
                }
            }
        })?;
    Ok(DispatchHandle {
        tx: ctrl_tx,
        stats,
    })
}

/// Drains every pending registration/deregistration into `routes`. Returns
/// `false` once the control channel has disconnected (every
/// `DispatchHandle` dropped).
fn drain_control(
    ctrl_rx: &mut tokio::sync::mpsc::UnboundedReceiver<Control>,
    routes: &mut FxHashMap<i64, broadcast::Sender<TokenReply>>,
) -> bool {
    loop {
        match ctrl_rx.try_recv() {
            Ok(Control::Register(uid, tx)) => {
                // Replacing an existing entry drops its old Sender, so the
                // earlier UidStream ends after whatever it had buffered.
                routes.insert(uid, tx);
            }
            Ok(Control::Deregister(uid)) => {
                routes.remove(&uid);
            }
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => return false,
        }
    }
}

/// Routes one decoded `TokenizerMsg`, recursing through `BatchTokenizerMsg`
/// items in order (mirrors upstream's `_unwrap_msg`).
fn route_message(
    routes: &mut FxHashMap<i64, broadcast::Sender<TokenReply>>,
    msg: rsg_wire::TokenizerMsg,
    stats: &DispatchStats,
) {
    match msg {
        rsg_wire::TokenizerMsg::DetokenizeMsg {
            uid,
            next_token,
            finished,
        } => route_one(
            routes,
            uid,
            TokenReply {
                uid,
                next_token,
                finished,
            },
            stats,
        ),
        rsg_wire::TokenizerMsg::BatchTokenizerMsg { data } => {
            for item in data {
                route_message(routes, item, stats);
            }
        }
    }
}

/// Routes one reply to its uid's channel. Unknown uid (never registered,
/// already deregistered, or already finished): counted in `unknown_uid`
/// and logged at `debug` (D-04) — a mass cancellation can produce
/// thousands of these, so the counter, not the log, is the signal. Send
/// error (no receivers left): counted in `closed_route` and the route is
/// removed. A successful send is counted in `routed`; a finished reply
/// also removes the route after that successful send.
fn route_one(
    routes: &mut FxHashMap<i64, broadcast::Sender<TokenReply>>,
    uid: i64,
    reply: TokenReply,
    stats: &DispatchStats,
) {
    let Some(tx) = routes.get(&uid) else {
        stats.unknown_uid.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(uid, "rx-zmq: reply for unknown or deregistered uid");
        return;
    };
    if tx.send(reply).is_err() {
        routes.remove(&uid);
        stats.closed_route.fetch_add(1, Ordering::Relaxed);
        return;
    }
    stats.routed.fetch_add(1, Ordering::Relaxed);
    if reply.finished {
        routes.remove(&uid);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A `std::io::Write` sink that appends every write into a shared
    /// buffer, so a test can capture `tracing` output without an external
    /// subscriber.
    struct CapturingSink(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for CapturingSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn lagged_stream_reports_dropped_event_counter_and_warning() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || CapturingSink(Arc::clone(&sink)))
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build current-thread runtime");
            rt.block_on(async {
                let (tx, rx) = broadcast::channel(UID_CHANNEL_CAPACITY);
                let mut stream = UidStream {
                    uid: 3,
                    rx,
                    dropped: 0,
                };

                for next_token in 0..20i64 {
                    tx.send(TokenReply {
                        uid: 3,
                        next_token,
                        finished: next_token == 19,
                    })
                    .expect("send");
                }

                let first = stream.recv().await.expect("first recv");
                assert_eq!(first, UidEvent::Dropped(4));
                assert_eq!(stream.dropped(), 4);

                for expected_token in 4..20i64 {
                    let event = stream.recv().await.expect("recv");
                    match event {
                        UidEvent::Token(reply) => {
                            assert_eq!(reply.next_token, expected_token);
                            assert_eq!(reply.finished, expected_token == 19);
                        }
                        UidEvent::Dropped(n) => panic!("unexpected extra drop of {n}"),
                    }
                }
            });
        });

        let text = String::from_utf8(captured.lock().unwrap().clone()).expect("utf8 log");
        assert!(text.contains("slow consumer"), "log missing message: {text}");
        assert!(text.contains("uid=3"), "log missing uid field: {text}");
        assert!(text.contains("dropped=4"), "log missing dropped field: {text}");
    }

    // --- Task 2: stats, deregister, unknown/malformed/closed routes,
    // re-register, and finished-survives-lag. ---

    use std::time::Duration;

    use crate::transport::{Endpoint, Role, ZmqBackendTx, ZmqTransport};

    /// A real `ZmqTransport`'s detok half, driven by a raw `zmq::PUSH` peer
    /// connected straight into it, with a live `DispatchHandle` routing
    /// whatever the peer sends. The backend half is opened (Connect, to an
    /// address nobody binds) and kept alive for the test's duration, as
    /// `ZmqTransport::open` always opens both sockets together.
    struct TestDispatcher {
        handle: DispatchHandle,
        _backend_tx: ZmqBackendTx,
        _ctx: zmq::Context,
        peer: zmq::Socket,
        detok_path: String,
    }

    impl TestDispatcher {
        fn open(tag: &str) -> TestDispatcher {
            let pid = std::process::id();
            let backend_addr = format!("ipc:///tmp/rsgt-{pid}-b{tag}");
            let detok_addr = format!("ipc:///tmp/rsgt-{pid}-d{tag}");
            let backend = Endpoint {
                addr: backend_addr,
                role: Role::Connect,
            };
            let detok = Endpoint {
                addr: detok_addr.clone(),
                role: Role::Bind,
            };
            let transport = ZmqTransport::open(&backend, &detok).expect("open transport");
            let (backend_tx, detok_rx) = transport.split();
            let handle = spawn_dispatcher(detok_rx).expect("spawn dispatcher");

            let ctx = zmq::Context::new();
            let peer = ctx.socket(zmq::PUSH).expect("create PUSH peer");
            peer.set_linger(0).expect("set linger on PUSH peer");
            peer.connect(&detok_addr).expect("connect PUSH peer");

            TestDispatcher {
                handle,
                _backend_tx: backend_tx,
                _ctx: ctx,
                peer,
                detok_path: detok_addr
                    .strip_prefix("ipc://")
                    .expect("ipc address")
                    .to_owned(),
            }
        }

        fn send_tokenizer(&self, msg: &rsg_wire::TokenizerMsg) {
            let bytes = rsg_wire::encode_tokenizer(msg).expect("encode TokenizerMsg");
            self.peer.send(&bytes, 0).expect("send tokenizer frame");
        }

        fn send_raw(&self, bytes: &[u8]) {
            self.peer.send(bytes, 0).expect("send raw frame");
        }

        /// Waits up to 2s for `pred` to hold on a fresh `stats()` snapshot.
        fn wait_for_stats(&self, pred: impl Fn(DispatchStatsSnapshot) -> bool) -> DispatchStatsSnapshot {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            loop {
                let snap = self.handle.stats();
                if pred(snap) {
                    return snap;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for stats predicate; last snapshot: {snap:?}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Drop for TestDispatcher {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.detok_path);
        }
    }

    fn detok(uid: i64, next_token: i64, finished: bool) -> rsg_wire::TokenizerMsg {
        rsg_wire::TokenizerMsg::DetokenizeMsg {
            uid,
            next_token,
            finished,
        }
    }

    /// Awaits one `UidStream` event within 2s, panicking on timeout or a
    /// `Dropped` event, and returns the token's `next_token`.
    async fn recv_token(stream: &mut UidStream) -> i64 {
        match tokio::time::timeout(Duration::from_secs(2), stream.recv())
            .await
            .expect("timed out waiting for token")
            .expect("stream ended unexpectedly")
        {
            UidEvent::Token(reply) => reply.next_token,
            UidEvent::Dropped(n) => panic!("unexpected drop of {n} tokens"),
        }
    }

    #[tokio::test]
    async fn unknown_uid_reply_is_dropped_and_counted() {
        let d = TestDispatcher::open("unk");

        d.send_tokenizer(&detok(42, 0, false));
        let snap = d.wait_for_stats(|s| s.unknown_uid == 1);
        assert_eq!(snap.routed, 0);

        let mut stream = d.handle.register(1);
        d.send_tokenizer(&detok(1, 7, false));
        let event = tokio::time::timeout(Duration::from_secs(2), stream.recv())
            .await
            .expect("timed out")
            .expect("stream ended");
        assert_eq!(
            event,
            UidEvent::Token(TokenReply {
                uid: 1,
                next_token: 7,
                finished: false
            })
        );
    }

    #[tokio::test]
    async fn malformed_frame_is_skipped_and_dispatcher_keeps_routing() {
        let d = TestDispatcher::open("mal");
        let mut stream = d.handle.register(1);

        d.send_raw(b"garbage");
        d.wait_for_stats(|s| s.malformed_frames == 1);

        d.send_tokenizer(&detok(1, 3, false));
        let event = tokio::time::timeout(Duration::from_secs(2), stream.recv())
            .await
            .expect("timed out")
            .expect("stream ended");
        assert_eq!(
            event,
            UidEvent::Token(TokenReply {
                uid: 1,
                next_token: 3,
                finished: false
            })
        );
    }

    #[tokio::test]
    async fn batch_entries_route_to_their_own_uids_in_order() {
        let d = TestDispatcher::open("batch");
        let mut stream1 = d.handle.register(1);
        let mut stream2 = d.handle.register(2);

        d.send_tokenizer(&rsg_wire::TokenizerMsg::BatchTokenizerMsg {
            data: vec![
                detok(1, 10, false),
                detok(2, 20, false),
                detok(1, 11, false),
                rsg_wire::TokenizerMsg::BatchTokenizerMsg {
                    data: vec![detok(2, 21, false)],
                },
            ],
        });

        let snap = d.wait_for_stats(|s| s.routed == 4);
        assert_eq!(snap.routed, 4);

        assert_eq!(recv_token(&mut stream1).await, 10);
        assert_eq!(recv_token(&mut stream1).await, 11);
        assert_eq!(recv_token(&mut stream2).await, 20);
        assert_eq!(recv_token(&mut stream2).await, 21);
    }

    #[tokio::test]
    async fn deregistered_uid_late_reply_counts_as_unknown() {
        let d = TestDispatcher::open("dereg");
        let mut stream = d.handle.register(1);
        d.handle.deregister(1);

        d.send_tokenizer(&detok(1, 5, false));
        d.wait_for_stats(|s| s.unknown_uid == 1);

        let event = tokio::time::timeout(Duration::from_secs(2), stream.recv())
            .await
            .expect("timed out waiting for stream to end");
        assert_eq!(event, None);
    }

    #[tokio::test]
    async fn reregister_replaces_route_and_ends_old_stream() {
        let d = TestDispatcher::open("rereg");
        let mut old = d.handle.register(1);
        let mut new = d.handle.register(1);

        d.send_tokenizer(&detok(1, 5, false));
        assert_eq!(recv_token(&mut new).await, 5);

        let old_event = tokio::time::timeout(Duration::from_secs(2), old.recv())
            .await
            .expect("timed out waiting for old stream to end");
        assert_eq!(old_event, None);
    }

    #[tokio::test]
    async fn dropped_stream_counts_closed_route() {
        let d = TestDispatcher::open("closed");
        let stream = d.handle.register(1);
        drop(stream);

        d.send_tokenizer(&detok(1, 1, false));
        d.wait_for_stats(|s| s.closed_route == 1);

        d.send_tokenizer(&detok(1, 2, false));
        d.wait_for_stats(|s| s.unknown_uid == 1);
    }

    #[tokio::test]
    async fn finished_reply_ends_stream_and_removes_route() {
        let d = TestDispatcher::open("finish");
        let mut stream = d.handle.register(1);

        d.send_tokenizer(&detok(1, 7, true));
        let event = tokio::time::timeout(Duration::from_secs(2), stream.recv())
            .await
            .expect("timed out")
            .expect("stream ended before token");
        assert_eq!(
            event,
            UidEvent::Token(TokenReply {
                uid: 1,
                next_token: 7,
                finished: true
            })
        );

        let end = tokio::time::timeout(Duration::from_secs(2), stream.recv())
            .await
            .expect("timed out waiting for stream end");
        assert_eq!(end, None);

        d.send_tokenizer(&detok(1, 8, false));
        d.wait_for_stats(|s| s.unknown_uid == 1);
    }

    #[tokio::test]
    async fn finished_token_survives_lag() {
        let d = TestDispatcher::open("lag");
        let mut stream = d.handle.register(1);

        let data: Vec<_> = (0..40i64)
            .map(|next_token| detok(1, next_token, next_token == 39))
            .collect();
        d.send_tokenizer(&rsg_wire::TokenizerMsg::BatchTokenizerMsg { data });
        d.wait_for_stats(|s| s.routed == 40);

        let first = tokio::time::timeout(Duration::from_secs(2), stream.recv())
            .await
            .expect("timed out")
            .expect("stream ended");
        assert_eq!(first, UidEvent::Dropped(24));
        assert_eq!(stream.dropped(), 24);

        for expected in 24..40i64 {
            let event = tokio::time::timeout(Duration::from_secs(2), stream.recv())
                .await
                .expect("timed out")
                .expect("stream ended early");
            match event {
                UidEvent::Token(reply) => {
                    assert_eq!(reply.next_token, expected);
                    assert_eq!(reply.finished, expected == 39);
                }
                UidEvent::Dropped(n) => panic!("unexpected extra drop of {n}"),
            }
        }

        let end = tokio::time::timeout(Duration::from_secs(2), stream.recv())
            .await
            .expect("timed out waiting for stream end after finished token");
        assert_eq!(end, None);
    }

    #[tokio::test]
    async fn dropping_every_handle_stops_dispatcher_and_ends_streams() {
        let d = TestDispatcher::open("dropall");
        let mut stream = d.handle.register(1);

        // No DispatchHandle clone exists anywhere else, so dropping the
        // whole TestDispatcher (which owns the only handle) stops the
        // rx-zmq thread.
        drop(d);

        let event = tokio::time::timeout(Duration::from_secs(1), stream.recv())
            .await
            .expect("timed out waiting for dispatcher shutdown");
        assert_eq!(event, None);
    }
}
