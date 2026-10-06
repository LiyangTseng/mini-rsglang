//! The single ordered writer (D-01). One dedicated OS thread, named
//! `tx-zmq`, exclusively owns the backend PUSH half of the transport
//! (`BackendSink`). Every outgoing message to the scheduler — `UserMsg`
//! today, plus `AbortBackendMsg` and `ExitMsg` — passes through this one
//! thread's single FIFO queue, so there is exactly one path to the
//! scheduler and no second writer can ever let a later message overtake an
//! earlier one.
//!
//! The queue is a bounded `tokio::sync::mpsc` channel of capacity
//! [`WRITER_QUEUE_CAPACITY`]. The writer thread drains everything already
//! queued before building a frame: a lone pending message is sent bare,
//! two or more are coalesced into one `BackendMsg::BatchBackendMsg` in
//! enqueue order — mirroring upstream's own coalescing
//! (`scheduler/io.py:124-130`, `tokenizer/server.py:99-108`; RESEARCH.md
//! Pitfall 3: never wrap a lone message in a one-element batch).

use anyhow::Context as _;
use rsg_wire::{BackendMsg, SamplingParams, Tensor};

use crate::transport::BackendSink;

/// Bound on the writer's inbox (Claude's discretion, CONTEXT D-01).
pub const WRITER_QUEUE_CAPACITY: usize = 1024;

/// A successfully enqueued submit. The enqueue — not any later send over
/// the wire — is the ordering point 03-04's abort ticket API relies on:
/// once `submit` returns, this uid's `UserMsg` already sits in the single
/// FIFO ahead of anything enqueued after it.
#[derive(Debug, Clone)]
pub struct Submitted {
    uid: i64,
}

impl Submitted {
    pub fn uid(&self) -> i64 {
        self.uid
    }
}

/// The writer thread has stopped — its sink failed, or every [`WriterHandle`]
/// was dropped — so the caller's message was never sent.
#[derive(Debug, thiserror::Error)]
#[error("writer thread has stopped")]
pub struct WriterClosed;

/// A cheap, cloneable handle onto the single writer's inbox.
#[derive(Clone)]
pub struct WriterHandle {
    tx: tokio::sync::mpsc::Sender<BackendMsg>,
}

impl WriterHandle {
    /// Enqueues a `UserMsg`. Returns once the enqueue has completed — the
    /// ordering point described in the module doc.
    pub async fn submit(
        &self,
        uid: i64,
        input_ids: Tensor,
        sampling_params: SamplingParams,
    ) -> Result<Submitted, WriterClosed> {
        self.tx
            .send(BackendMsg::UserMsg {
                uid,
                input_ids,
                sampling_params,
            })
            .await
            .map_err(|_| WriterClosed)?;
        Ok(Submitted { uid })
    }

    /// Enqueues an `ExitMsg`.
    pub async fn exit(&self) -> Result<(), WriterClosed> {
        self.tx
            .send(BackendMsg::ExitMsg {})
            .await
            .map_err(|_| WriterClosed)
    }
}

/// Spawns the `tx-zmq` thread owning `sink`, and returns a handle plus its
/// `JoinHandle`. The thread runs until every [`WriterHandle`] clone is
/// dropped (`Ok(())`) or `sink.send_backend` fails (`Err`, after which every
/// later `submit`/`exit` returns `Err(WriterClosed)` because dropping the
/// receiver on thread exit closes the channel).
pub fn spawn_writer<S: BackendSink + 'static>(
    sink: S,
) -> std::io::Result<(WriterHandle, std::thread::JoinHandle<anyhow::Result<()>>)> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<BackendMsg>(WRITER_QUEUE_CAPACITY);
    let join = std::thread::Builder::new()
        .name("tx-zmq".to_owned())
        .spawn(move || -> anyhow::Result<()> {
            loop {
                let Some(first) = rx.blocking_recv() else {
                    // Every WriterHandle was dropped: a clean shutdown.
                    return Ok(());
                };
                let mut pending = vec![first];
                while let Ok(msg) = rx.try_recv() {
                    pending.push(msg);
                }
                let msg = if pending.len() == 1 {
                    pending.pop().expect("len checked above")
                } else {
                    BackendMsg::BatchBackendMsg { data: pending }
                };
                let bytes = rsg_wire::encode_backend(&msg).context("encode BackendMsg")?;
                if let Err(e) = sink.send_backend(&bytes).context("send on backend socket") {
                    tracing::error!("tx-zmq: {e:#}");
                    return Err(e);
                }
            }
        })?;
    Ok((WriterHandle { tx }, join))
}
