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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use rsg_wire::{BackendMsg, decode_backend};

    use super::*;

    /// What [`FakeSink::gated`] returns: the sink, its recorded frames, and
    /// the entered/release signal pair.
    type GatedSink = (
        FakeSink,
        Arc<Mutex<Vec<Vec<u8>>>>,
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::Sender<()>,
    );

    /// A test-local [`BackendSink`] that records every sent frame, and can
    /// optionally gate its first send (to make coalescing deterministic) or
    /// always fail.
    struct FakeSink {
        frames: Arc<Mutex<Vec<Vec<u8>>>>,
        gate: Option<(
            std::sync::mpsc::Sender<()>,
            Mutex<std::sync::mpsc::Receiver<()>>,
        )>,
        always_fail: bool,
    }

    impl FakeSink {
        fn new() -> (Self, Arc<Mutex<Vec<Vec<u8>>>>) {
            let frames = Arc::new(Mutex::new(Vec::new()));
            (
                FakeSink {
                    frames: Arc::clone(&frames),
                    gate: None,
                    always_fail: false,
                },
                frames,
            )
        }

        fn always_fail() -> Self {
            let (_, frames) = Self::new();
            FakeSink {
                frames,
                gate: None,
                always_fail: true,
            }
        }

        /// Returns the sink plus (entered_rx, release_tx): the sink's first
        /// `send_backend` call sends on `entered` and then blocks until a
        /// unit is sent on `release`.
        fn gated() -> GatedSink {
            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let frames = Arc::new(Mutex::new(Vec::new()));
            let sink = FakeSink {
                frames: Arc::clone(&frames),
                gate: Some((entered_tx, Mutex::new(release_rx))),
                always_fail: false,
            };
            (sink, frames, entered_rx, release_tx)
        }
    }

    impl BackendSink for FakeSink {
        fn send_backend(&self, frame: &[u8]) -> anyhow::Result<()> {
            if let Some((entered_tx, release_rx)) = &self.gate {
                self.frames.lock().unwrap().push(frame.to_vec());
                let _ = entered_tx.send(());
                let _ = release_rx.lock().unwrap().recv();
                return Ok(());
            }
            if self.always_fail {
                return Err(anyhow::anyhow!("sink down"));
            }
            self.frames.lock().unwrap().push(frame.to_vec());
            Ok(())
        }
    }

    fn wait_for<T>(mut f: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(Instant::now() < deadline, "timed out waiting for condition");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[tokio::test]
    async fn single_message_is_sent_bare() {
        let (sink, frames) = FakeSink::new();
        let (writer, _join) = spawn_writer(sink).expect("spawn writer");

        writer
            .submit(1, Tensor::from_i32_slice(&[]), SamplingParams::default())
            .await
            .expect("submit");

        let frame = wait_for(|| frames.lock().unwrap().first().cloned());
        assert_eq!(frames.lock().unwrap().len(), 1, "exactly one frame");
        match decode_backend(&frame).expect("decode") {
            BackendMsg::UserMsg { uid, .. } => assert_eq!(uid, 1),
            other => panic!("expected bare UserMsg, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn queued_messages_coalesce_into_one_batch_in_order() {
        let (sink, frames, entered_rx, release_tx) = FakeSink::gated();
        let (writer, _join) = spawn_writer(sink).expect("spawn writer");

        writer
            .submit(1, Tensor::from_i32_slice(&[]), SamplingParams::default())
            .await
            .expect("submit 1");
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("writer entered first send");

        writer
            .submit(2, Tensor::from_i32_slice(&[]), SamplingParams::default())
            .await
            .expect("submit 2");
        writer
            .submit(3, Tensor::from_i32_slice(&[]), SamplingParams::default())
            .await
            .expect("submit 3");
        writer.exit().await.expect("exit");

        let _ = release_tx.send(());

        let frame1 = wait_for(|| frames.lock().unwrap().first().cloned());
        match decode_backend(&frame1).expect("decode frame 1") {
            BackendMsg::UserMsg { uid, .. } => assert_eq!(uid, 1),
            other => panic!("expected bare UserMsg 1, got {other:?}"),
        }

        let frame2 = wait_for(|| frames.lock().unwrap().get(1).cloned());
        match decode_backend(&frame2).expect("decode frame 2") {
            BackendMsg::BatchBackendMsg { data } => {
                assert_eq!(data.len(), 3, "batch has uid 2, uid 3, ExitMsg");
                match &data[0] {
                    BackendMsg::UserMsg { uid, .. } => assert_eq!(*uid, 2),
                    other => panic!("expected UserMsg 2 first, got {other:?}"),
                }
                match &data[1] {
                    BackendMsg::UserMsg { uid, .. } => assert_eq!(*uid, 3),
                    other => panic!("expected UserMsg 3 second, got {other:?}"),
                }
                match &data[2] {
                    BackendMsg::ExitMsg {} => {}
                    other => panic!("expected ExitMsg third, got {other:?}"),
                }
            }
            other => panic!("expected BatchBackendMsg, got {other:?}"),
        }

        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(frames.lock().unwrap().len(), 2, "no third frame");
    }

    #[tokio::test]
    async fn exit_sends_bare_exit_msg() {
        let (sink, frames) = FakeSink::new();
        let (writer, _join) = spawn_writer(sink).expect("spawn writer");

        writer.exit().await.expect("exit");

        let frame = wait_for(|| frames.lock().unwrap().first().cloned());
        assert_eq!(frames.lock().unwrap().len(), 1, "exactly one frame");
        match decode_backend(&frame).expect("decode") {
            BackendMsg::ExitMsg {} => {}
            other => panic!("expected bare ExitMsg, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn sink_error_closes_writer() {
        let sink = FakeSink::always_fail();
        let (writer, join) = spawn_writer(sink).expect("spawn writer");

        let first = writer
            .submit(1, Tensor::from_i32_slice(&[]), SamplingParams::default())
            .await;
        assert!(first.is_ok(), "the enqueue itself succeeds");

        let joined = tokio::task::spawn_blocking(move || join.join().expect("thread panicked"))
            .await
            .expect("spawn_blocking panicked");
        assert!(joined.is_err(), "writer thread ends with Err");

        let after = writer
            .submit(2, Tensor::from_i32_slice(&[]), SamplingParams::default())
            .await;
        assert!(matches!(after, Err(WriterClosed)));
        assert!(matches!(writer.exit().await, Err(WriterClosed)));
    }

    #[tokio::test]
    async fn dropping_every_handle_stops_writer_cleanly() {
        let (sink, _frames) = FakeSink::new();
        let (writer, join) = spawn_writer(sink).expect("spawn writer");
        drop(writer);

        let joined = tokio::task::spawn_blocking(move || join.join().expect("thread panicked"))
            .await
            .expect("spawn_blocking panicked");
        assert!(
            joined.is_ok(),
            "writer thread ends Ok after every handle drops"
        );
    }
}
