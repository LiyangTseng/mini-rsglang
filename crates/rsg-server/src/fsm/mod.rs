//! The registry actor (LIFE-01/LIFE-03 accounting): one tokio task owning
//! `FxHashMap<i64, ReqState>` for every live request, with no locks on the
//! hot path. Every lifecycle transition in the system is reported here
//! through [`RegistryHandle::report`]; a leaked or double-terminated
//! request shows up as a non-zero `active` or `invalid_transitions` count
//! in [`RegistrySnapshot`].

pub mod state;

use std::time::Instant;

use rustc_hash::FxHashMap;
use tokio::sync::{mpsc, oneshot};

use crate::metrics::ServerMetrics;
use state::{LifecycleState, ReqState};

/// Messages the registry task's inbox accepts.
enum Msg {
    Report {
        uid: i64,
        to: LifecycleState,
        at: Instant,
    },
    Snapshot(oneshot::Sender<RegistrySnapshot>),
}

/// A cheap, cloneable handle onto the registry actor's inbox.
#[derive(Clone)]
pub struct RegistryHandle {
    tx: mpsc::UnboundedSender<Msg>,
}

impl RegistryHandle {
    /// Reports that `uid` reached `to`. Sync, never blocks, stamps
    /// `Instant::now()`. Silently dropped if the registry task has already
    /// stopped (ignored like every other handle-after-shutdown case in
    /// this project).
    pub fn report(&self, uid: i64, to: LifecycleState) {
        let _ = self.tx.send(Msg::Report {
            uid,
            to,
            at: Instant::now(),
        });
    }

    /// Awaits a point-in-time snapshot of the registry's counters.
    pub async fn snapshot(&self) -> RegistrySnapshot {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(Msg::Snapshot(tx));
        rx.await.unwrap_or_default()
    }
}

/// A point-in-time snapshot of the registry's counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegistrySnapshot {
    pub received: u64,
    pub active: u64,
    pub finished: u64,
    pub cancelled: u64,
    pub failed: u64,
    pub invalid_transitions: u64,
}

/// Spawns the registry actor. Must be called inside a tokio runtime.
/// `metrics` records every transition this actor applies (API-02): a
/// request count on `Received`, a TTFT observation on `Submitted ->
/// Decoding`, a terminal count on every terminal state, and the active
/// gauge after every applied report.
pub fn spawn_registry(metrics: ServerMetrics) -> RegistryHandle {
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    tokio::spawn(async move {
        let mut table: FxHashMap<i64, ReqState> = FxHashMap::default();
        let mut counts = RegistrySnapshot::default();
        while let Some(msg) = rx.recv().await {
            match msg {
                Msg::Report { uid, to, at } => {
                    if to == LifecycleState::Received {
                        // A uid already present (an earlier Received that
                        // hasn't terminated yet) is a duplicate report, not
                        // a second request.
                        match table.entry(uid) {
                            std::collections::hash_map::Entry::Vacant(e) => {
                                e.insert(ReqState::new(uid, at));
                                counts.received += 1;
                                metrics.record_received();
                            }
                            std::collections::hash_map::Entry::Occupied(_) => {
                                tracing::warn!(uid, "duplicate Received report for a tracked uid");
                                counts.invalid_transitions += 1;
                            }
                        }
                    } else {
                        match table.get_mut(&uid) {
                            None => {
                                // Never registered, already deregistered by
                                // this same rule, or already terminal (and
                                // thus already removed below).
                                tracing::warn!(
                                    uid,
                                    to = ?to,
                                    "report for unknown or already-terminal uid"
                                );
                                counts.invalid_transitions += 1;
                            }
                            Some(entry) => {
                                let from = entry.state;
                                match entry.advance(to, at) {
                                    Ok(()) => {
                                        if from == LifecycleState::Submitted
                                            && to == LifecycleState::Decoding
                                        {
                                            metrics
                                                .record_ttft(at.duration_since(entry.received_at));
                                        }
                                        if to.is_terminal() {
                                            match to {
                                                LifecycleState::Finished => counts.finished += 1,
                                                LifecycleState::Cancelled => counts.cancelled += 1,
                                                LifecycleState::Failed => counts.failed += 1,
                                                _ => {}
                                            }
                                            metrics.record_terminal(to);
                                            table.remove(&uid);
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            uid = e.uid,
                                            from = ?e.from,
                                            to = ?e.to,
                                            "invalid lifecycle transition"
                                        );
                                        counts.invalid_transitions += 1;
                                    }
                                }
                            }
                        }
                    }
                    metrics.set_active(table.len() as u64);
                }
                Msg::Snapshot(reply) => {
                    let mut snap = counts;
                    snap.active = table.len() as u64;
                    let _ = reply.send(snap);
                }
            }
        }
    });
    RegistryHandle { tx }
}

#[cfg(test)]
mod tests {
    use super::*;
    use state::LifecycleState::*;

    #[tokio::test]
    async fn second_terminal_is_counted_invalid() {
        let registry = spawn_registry(ServerMetrics::new());
        registry.report(1, Received);
        registry.report(1, Tokenizing);
        registry.report(1, Submitted);
        registry.report(1, Decoding);
        registry.report(1, Finished);
        registry.report(1, Cancelled);

        let snap = registry.snapshot().await;
        assert_eq!(
            snap,
            RegistrySnapshot {
                received: 1,
                active: 0,
                finished: 1,
                cancelled: 0,
                failed: 0,
                invalid_transitions: 1,
            }
        );
    }

    #[tokio::test]
    async fn unknown_uid_and_duplicate_received_are_invalid() {
        let registry = spawn_registry(ServerMetrics::new());
        registry.report(9, Finished);
        registry.report(2, Received);
        registry.report(2, Received);

        let snap = registry.snapshot().await;
        assert_eq!(snap.received, 1);
        assert_eq!(snap.invalid_transitions, 2);
    }

    #[tokio::test]
    async fn active_counts_live_requests() {
        let registry = spawn_registry(ServerMetrics::new());
        registry.report(1, Received);
        registry.report(2, Received);
        registry.report(3, Received);
        registry.report(3, Tokenizing);
        registry.report(3, Failed);

        let snap = registry.snapshot().await;
        assert_eq!(snap.active, 2);
        assert_eq!(snap.failed, 1);
    }
}
