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
}

/// A cheap, cloneable handle used to register new uids with the dispatcher.
#[derive(Clone)]
pub struct DispatchHandle {
    tx: tokio::sync::mpsc::UnboundedSender<Control>,
}

impl DispatchHandle {
    /// Registers `uid` and returns its stream. Never blocks. If the
    /// dispatcher thread has already stopped, the returned stream still
    /// works correctly: its own `Sender` (held only by this call) is
    /// dropped immediately after, so the very first `recv()` sees
    /// `RecvError::Closed` and yields `None`.
    pub fn register(&self, uid: i64) -> UidStream {
        let (tx, rx) = broadcast::channel(UID_CHANNEL_CAPACITY);
        let _ = self.tx.send(Control::Register(uid, tx));
        UidStream { uid, rx }
    }
}

/// The per-request receiving half of a uid's registered route.
pub struct UidStream {
    uid: i64,
    rx: broadcast::Receiver<TokenReply>,
}

impl UidStream {
    pub fn uid(&self) -> i64 {
        self.uid
    }

    /// Awaits the next event for this uid. `None` once the route has
    /// closed: the dispatcher removed it after a finished reply, or the
    /// dispatcher thread itself stopped.
    pub async fn recv(&mut self) -> Option<UidEvent> {
        match self.rx.recv().await {
            Ok(t) => Some(UidEvent::Token(t)),
            Err(broadcast::error::RecvError::Lagged(n)) => Some(UidEvent::Dropped(n)),
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
                    Ok(msg) => route_message(&mut routes, msg),
                    Err(e) => tracing::warn!("rx-zmq: dropping undecodable frame: {e}"),
                }
            }
        })?;
    Ok(DispatchHandle { tx: ctrl_tx })
}

/// Drains every pending registration into `routes`. Returns `false` once
/// the control channel has disconnected (every `DispatchHandle` dropped).
fn drain_control(
    ctrl_rx: &mut tokio::sync::mpsc::UnboundedReceiver<Control>,
    routes: &mut FxHashMap<i64, broadcast::Sender<TokenReply>>,
) -> bool {
    loop {
        match ctrl_rx.try_recv() {
            Ok(Control::Register(uid, tx)) => {
                routes.insert(uid, tx);
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
        ),
        rsg_wire::TokenizerMsg::BatchTokenizerMsg { data } => {
            for item in data {
                route_message(routes, item);
            }
        }
    }
}

/// Routes one reply to its uid's channel. Unknown uid: drop silently
/// (D-04). Send error (no receivers left): remove the route. A finished
/// reply also removes the route after a successful send.
fn route_one(
    routes: &mut FxHashMap<i64, broadcast::Sender<TokenReply>>,
    uid: i64,
    reply: TokenReply,
) {
    let Some(tx) = routes.get(&uid) else {
        return;
    };
    if tx.send(reply).is_err() {
        routes.remove(&uid);
        return;
    }
    if reply.finished {
        routes.remove(&uid);
    }
}
