//! Standalone spike (not part of any production path, not wired into the
//! real benchmark harness): measures the raw enqueue latency of one ZMQ
//! PUSH send (the same transport, socket options, `ipc://` scheme and
//! real msgpack frame sizes as `rsg-server`'s production `tx-zmq` writer)
//! against an equivalent in-process `std::sync::mpsc::sync_channel` send,
//! under three load conditions. See README.md in this crate for what this
//! does and does not prove.

use std::sync::mpsc::sync_channel;
use std::thread;
use std::time::{Duration, Instant};

use hdrhistogram::Histogram;
use rsg_wire::{BackendMsg, SamplingParams, Tensor};

/// Matches libzmq's default `ZMQ_SNDHWM` / `ZMQ_RCVHWM` (1000 messages),
/// and is used as the in-process channel's bound too, so both transports
/// hit backpressure at the same queue depth.
const HWM: usize = 1000;

fn new_hist() -> Histogram<u64> {
    Histogram::<u64>::new_with_bounds(1, 60_000_000, 3).expect("static bounds are valid")
}

fn print_pcts(label: &str, h: &Histogram<u64>) {
    println!(
        "{label:<32} n={:<7} p50={:>9.1}us p90={:>9.1}us p99={:>9.1}us p99.9={:>9.1}us max={:>9.1}us",
        h.len(),
        h.value_at_quantile(0.50) as f64,
        h.value_at_quantile(0.90) as f64,
        h.value_at_quantile(0.99) as f64,
        h.value_at_quantile(0.999) as f64,
        h.max() as f64,
    );
}

/// `AbortBackendMsg`-sized frame: the realistic "cancel storm" message
/// (S1's scenario sends one of these per abort).
fn abort_frame() -> Vec<u8> {
    rsg_wire::encode_backend(&BackendMsg::AbortBackendMsg { uid: 1 }).expect("encode")
}

/// `UserMsg`-sized frame with a 32-token prompt, matching S2's "32-token
/// short-prompt saturation" scenario -- the realistic submit-path message.
fn user_frame() -> Vec<u8> {
    let ids: Vec<i32> = (0..32).collect();
    rsg_wire::encode_backend(&BackendMsg::UserMsg {
        uid: 1,
        input_ids: Tensor::from_i32_slice(&ids),
        sampling_params: SamplingParams::default(),
    })
    .expect("encode")
}

/// Opens a PUSH (connect) / PULL (bind) pair over a fresh `ipc://` path,
/// mirroring `rsg-server::transport::open_socket`'s options exactly
/// (linger=0, reconnect_ivl=1ms).
fn open_zmq_pair(tag: &str) -> (zmq::Context, zmq::Socket, zmq::Socket, String) {
    let path = format!("/tmp/zmq-vs-channel-spike-{}-{}", std::process::id(), tag);
    let addr = format!("ipc://{path}");
    let ctx = zmq::Context::new();

    let pull = ctx.socket(zmq::PULL).expect("create PULL");
    pull.set_linger(0).expect("set linger");
    pull.bind(&addr).expect("bind PULL");

    let push = ctx.socket(zmq::PUSH).expect("create PUSH");
    push.set_linger(0).expect("set linger");
    push.set_reconnect_ivl(1).expect("set reconnect ivl");
    push.connect(&addr).expect("connect PUSH");

    // libzmq's PUSH/PULL default HWM is already 1000 each way; set it
    // explicitly so this stays true regardless of the linked libzmq's
    // own defaults.
    push.set_sndhwm(HWM as i32).expect("set sndhwm");
    pull.set_rcvhwm(HWM as i32).expect("set rcvhwm");

    // Let the connect settle before any timed send.
    thread::sleep(Duration::from_millis(50));

    (ctx, push, pull, path)
}

/// Scenario A: the drainer keeps up immediately. Best case for both
/// transports -- the floor latency of the transport itself.
fn zmq_drained(frame: &[u8], n: usize) -> Histogram<u64> {
    let (_ctx, push, pull, path) = open_zmq_pair("drained");
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done_r = done.clone();
    let drainer = thread::spawn(move || {
        while !done_r.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = pull.recv_bytes(zmq::DONTWAIT);
        }
        // Drain whatever's left.
        while pull.recv_bytes(zmq::DONTWAIT).is_ok() {}
    });
    let mut hist = new_hist();
    for _ in 0..n {
        let t0 = Instant::now();
        push.send(frame, 0).expect("send");
        hist.record(t0.elapsed().as_micros() as u64).unwrap();
        thread::sleep(Duration::from_micros(50)); // let the drainer keep pace
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    drainer.join().unwrap();
    let _ = std::fs::remove_file(&path);
    hist
}

fn channel_drained(frame: &[u8], n: usize) -> Histogram<u64> {
    let (tx, rx) = sync_channel::<Vec<u8>>(HWM);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done_r = done.clone();
    let drainer = thread::spawn(move || {
        while !done_r.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = rx.try_recv();
        }
        while rx.try_recv().is_ok() {}
    });
    let mut hist = new_hist();
    for _ in 0..n {
        let t0 = Instant::now();
        tx.send(frame.to_vec()).expect("send");
        hist.record(t0.elapsed().as_micros() as u64).unwrap();
        thread::sleep(Duration::from_micros(50));
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    drainer.join().unwrap();
    hist
}

/// Scenario B: the drainer never runs until after every send is issued.
/// Both transports' queues fill past HWM and the blocking send has to
/// wait for the peer to make room -- this is the backpressure cliff the
/// real `tx-zmq` thread (and `WriterHandle::submit().await`, transitively)
/// would hit if the Python scheduler fell behind draining its PULL socket.
fn zmq_backed_up(frame: &[u8], n: usize) -> Histogram<u64> {
    let (_ctx, push, pull, path) = open_zmq_pair("backed_up");
    let mut hist = new_hist();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let b2 = barrier.clone();
    let sends_done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sends_done_r = sends_done.clone();
    // The drainer waits until every send has been *issued* (not
    // necessarily completed -- blocked sends are still "issued") before
    // it starts pulling, so every send after the HWM fills has to wait.
    let drainer = thread::spawn(move || {
        b2.wait();
        while !sends_done_r.load(std::sync::atomic::Ordering::Relaxed)
            || pull.poll(zmq::POLLIN, 0).unwrap_or(0) > 0
        {
            let _ = pull.recv_bytes(zmq::DONTWAIT);
            thread::sleep(Duration::from_micros(200));
        }
    });
    barrier.wait();
    for _ in 0..n {
        let t0 = Instant::now();
        push.send(frame, 0).expect("send");
        hist.record(t0.elapsed().as_micros() as u64).unwrap();
    }
    sends_done.store(true, std::sync::atomic::Ordering::Relaxed);
    drainer.join().unwrap();
    let _ = std::fs::remove_file(&path);
    hist
}

fn channel_backed_up(frame: &[u8], n: usize) -> Histogram<u64> {
    let (tx, rx) = sync_channel::<Vec<u8>>(HWM);
    let mut hist = new_hist();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let b2 = barrier.clone();
    let sends_done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sends_done_r = sends_done.clone();
    let drainer = thread::spawn(move || {
        b2.wait();
        while !sends_done_r.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = rx.try_recv();
            thread::sleep(Duration::from_micros(200));
        }
        while rx.try_recv().is_ok() {}
    });
    barrier.wait();
    for _ in 0..n {
        let t0 = Instant::now();
        tx.send(frame.to_vec()).expect("send");
        hist.record(t0.elapsed().as_micros() as u64).unwrap();
    }
    sends_done.store(true, std::sync::atomic::Ordering::Relaxed);
    drainer.join().unwrap();
    hist
}

/// Scenario C: "cancel storm" -- the sender fires as fast as possible
/// (no sleep between sends, unlike scenario A) while the drainer only
/// pulls at a fixed, deliberately slow rate (simulating a scheduler that
/// is itself CPU-bound and can only process aborts at a limited rate).
fn zmq_cancel_storm(frame: &[u8], n: usize, drain_every: Duration) -> Histogram<u64> {
    let (_ctx, push, pull, path) = open_zmq_pair("storm");
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done_r = done.clone();
    let drainer = thread::spawn(move || {
        while !done_r.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = pull.recv_bytes(zmq::DONTWAIT);
            thread::sleep(drain_every);
        }
        while pull.recv_bytes(zmq::DONTWAIT).is_ok() {}
    });
    let mut hist = new_hist();
    for _ in 0..n {
        let t0 = Instant::now();
        push.send(frame, 0).expect("send");
        hist.record(t0.elapsed().as_micros() as u64).unwrap();
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    drainer.join().unwrap();
    let _ = std::fs::remove_file(&path);
    hist
}

fn channel_cancel_storm(frame: &[u8], n: usize, drain_every: Duration) -> Histogram<u64> {
    let (tx, rx) = sync_channel::<Vec<u8>>(HWM);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done_r = done.clone();
    let drainer = thread::spawn(move || {
        while !done_r.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = rx.try_recv();
            thread::sleep(drain_every);
        }
        while rx.try_recv().is_ok() {}
    });
    let mut hist = new_hist();
    for _ in 0..n {
        let t0 = Instant::now();
        tx.send(frame.to_vec()).expect("send");
        hist.record(t0.elapsed().as_micros() as u64).unwrap();
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    drainer.join().unwrap();
    hist
}

fn run_frame(label: &str, frame: &[u8]) {
    println!("\n=== frame: {label} ({} bytes) ===", frame.len());

    println!("-- scenario A: drained (receiver keeps up) --");
    print_pcts("zmq_push_pull", &zmq_drained(frame, 5_000));
    print_pcts("std_sync_channel", &channel_drained(frame, 5_000));

    println!("-- scenario B: backed up (receiver idle until HWM blocks sends) --");
    print_pcts("zmq_push_pull", &zmq_backed_up(frame, 3_000));
    print_pcts("std_sync_channel", &channel_backed_up(frame, 3_000));

    println!("-- scenario C: cancel storm (sender fast, receiver drains at 2ms/msg) --");
    print_pcts(
        "zmq_push_pull",
        &zmq_cancel_storm(frame, 3_000, Duration::from_millis(2)),
    );
    print_pcts(
        "std_sync_channel",
        &channel_cancel_storm(frame, 3_000, Duration::from_millis(2)),
    );
}

fn main() {
    let abort = abort_frame();
    let user = user_frame();
    run_frame("AbortBackendMsg (S1 cancel storm)", &abort);
    run_frame("UserMsg, 32-token prompt (S2 saturation)", &user);
}
