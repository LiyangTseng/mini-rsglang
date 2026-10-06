//! The small interface over the ZMQ crate (CONTEXT discretion item), so a later
//! phase can swap `zmq` for `zeromq` without touching the rest of the binary.
//!
//! Mirrors upstream `utils/mp.py`: one socket per endpoint, and
//! `bind(addr) if create else connect(addr)` chosen per endpoint.

use std::fmt;

use anyhow::Context as _;

/// Whether this process binds or connects an endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Role {
    Bind,
    Connect,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Role::Bind => "bind",
            Role::Connect => "connect",
        })
    }
}

/// One ZMQ endpoint: a full address (e.g. `ipc:///tmp/minisgl_0.pid=1`) and its role.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub addr: String,
    pub role: Role,
}

/// Send frames to the scheduler's backend (PULL) endpoint.
pub trait BackendSink: Send {
    fn send_backend(&self, frame: &[u8]) -> anyhow::Result<()>;
}

/// Receive frames from the scheduler's detokenizer (PUSH) endpoint.
pub trait DetokSource: Send {
    /// Wait up to `timeout_ms` for one frame; `Ok(None)` on timeout.
    fn recv_detok(&self, timeout_ms: i64) -> anyhow::Result<Option<Vec<u8>>>;
}

/// Frames to the scheduler (backend PUSH) and from it (detokenizer PULL).
pub trait Transport: BackendSink + DetokSource {}

impl<T: BackendSink + DetokSource> Transport for T {}

pub struct ZmqTransport {
    _ctx: zmq::Context,
    backend: zmq::Socket,
    detok: zmq::Socket,
}

impl ZmqTransport {
    /// Open a PUSH socket for `backend` and a PULL socket for `detok`, binding or
    /// connecting each per its role.
    pub fn open(backend: &Endpoint, detok: &Endpoint) -> anyhow::Result<ZmqTransport> {
        let ctx = zmq::Context::new();
        let backend_sock = open_socket(&ctx, zmq::PUSH, backend, "backend")?;
        let detok_sock = open_socket(&ctx, zmq::PULL, detok, "detokenizer")?;
        Ok(ZmqTransport {
            _ctx: ctx,
            backend: backend_sock,
            detok: detok_sock,
        })
    }

    /// Splits into two Send halves, each owning one socket, so each can be
    /// moved into its own dedicated OS thread (the project's tx-zmq/rx-zmq
    /// convention). Both halves keep the context alive via `zmq::Context`'s
    /// cheap, reference-counted clone.
    pub fn split(self) -> (ZmqBackendTx, ZmqDetokRx) {
        (
            ZmqBackendTx {
                _ctx: self._ctx.clone(),
                backend: self.backend,
            },
            ZmqDetokRx {
                _ctx: self._ctx,
                detok: self.detok,
            },
        )
    }
}

impl BackendSink for ZmqTransport {
    fn send_backend(&self, frame: &[u8]) -> anyhow::Result<()> {
        self.backend
            .send(frame, 0)
            .context("send on backend socket")
    }
}

impl DetokSource for ZmqTransport {
    fn recv_detok(&self, timeout_ms: i64) -> anyhow::Result<Option<Vec<u8>>> {
        let ready = self
            .detok
            .poll(zmq::POLLIN, timeout_ms)
            .context("poll detokenizer socket")?;
        if ready == 0 {
            return Ok(None);
        }
        let frame = self
            .detok
            .recv_bytes(0)
            .context("recv on detokenizer socket")?;
        Ok(Some(frame))
    }
}

/// The backend-sending half of a split [`ZmqTransport`]. Owns the PUSH socket.
pub struct ZmqBackendTx {
    _ctx: zmq::Context,
    backend: zmq::Socket,
}

impl BackendSink for ZmqBackendTx {
    fn send_backend(&self, frame: &[u8]) -> anyhow::Result<()> {
        self.backend
            .send(frame, 0)
            .context("send on backend socket")
    }
}

/// The detokenizer-receiving half of a split [`ZmqTransport`]. Owns the PULL socket.
pub struct ZmqDetokRx {
    _ctx: zmq::Context,
    detok: zmq::Socket,
}

impl DetokSource for ZmqDetokRx {
    fn recv_detok(&self, timeout_ms: i64) -> anyhow::Result<Option<Vec<u8>>> {
        let ready = self
            .detok
            .poll(zmq::POLLIN, timeout_ms)
            .context("poll detokenizer socket")?;
        if ready == 0 {
            return Ok(None);
        }
        let frame = self
            .detok
            .recv_bytes(0)
            .context("recv on detokenizer socket")?;
        Ok(Some(frame))
    }
}

/// The scheduler side of the wire: a PULL socket for the backend endpoint
/// (receives `UserMsg`/`AbortBackendMsg`/`BatchBackendMsg`/`ExitMsg`) and a
/// PUSH socket for the detokenizer endpoint (sends `DetokenizeMsg`/
/// `BatchTokenizerMsg`). Reuses the same `Endpoint`/`Role` convention and
/// `open_socket` helper as `ZmqTransport`, so `mock-scheduler` never redefines
/// them (D-08).
pub struct ZmqSchedulerTransport {
    _ctx: zmq::Context,
    backend: zmq::Socket,
    detok: zmq::Socket,
}

impl ZmqSchedulerTransport {
    /// Open a PULL socket for `backend` and a PUSH socket for `detok`, binding
    /// or connecting each per its role.
    pub fn open(backend: &Endpoint, detok: &Endpoint) -> anyhow::Result<ZmqSchedulerTransport> {
        let ctx = zmq::Context::new();
        let backend_sock = open_socket(&ctx, zmq::PULL, backend, "backend")?;
        let detok_sock = open_socket(&ctx, zmq::PUSH, detok, "detokenizer")?;
        Ok(ZmqSchedulerTransport {
            _ctx: ctx,
            backend: backend_sock,
            detok: detok_sock,
        })
    }

    /// Wait up to `timeout_ms` for one backend frame; `Ok(None)` on timeout.
    pub fn recv_backend(&self, timeout_ms: i64) -> anyhow::Result<Option<Vec<u8>>> {
        let ready = self
            .backend
            .poll(zmq::POLLIN, timeout_ms)
            .context("poll backend socket")?;
        if ready == 0 {
            return Ok(None);
        }
        let frame = self
            .backend
            .recv_bytes(0)
            .context("recv on backend socket")?;
        Ok(Some(frame))
    }

    /// Send one frame on the detokenizer socket.
    pub fn send_detok(&self, frame: &[u8]) -> anyhow::Result<()> {
        self.detok
            .send(frame, 0)
            .context("send on detokenizer socket")
    }
}

fn open_socket(
    ctx: &zmq::Context,
    kind: zmq::SocketType,
    ep: &Endpoint,
    name: &str,
) -> anyhow::Result<zmq::Socket> {
    let sock = ctx
        .socket(kind)
        .with_context(|| format!("create {name} socket"))?;
    sock.set_linger(0)
        .with_context(|| format!("set linger on {name} socket"))?;
    match ep.role {
        Role::Bind => sock.bind(&ep.addr),
        Role::Connect => sock.connect(&ep.addr),
    }
    .with_context(|| format!("{} {name} socket at {}", ep.role, ep.addr))?;
    Ok(sock)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(tag: &str) -> String {
        format!("ipc:///tmp/rsgt-{}-{tag}", std::process::id())
    }

    fn path(addr: &str) -> &str {
        addr.strip_prefix("ipc://").unwrap()
    }

    #[test]
    fn detok_bind_receives_from_raw_push_peer() {
        let backend = Endpoint {
            addr: addr("a"),
            role: Role::Connect,
        };
        let detok = Endpoint {
            addr: addr("b"),
            role: Role::Bind,
        };
        let t = ZmqTransport::open(&backend, &detok).expect("open");
        assert!(
            std::path::Path::new(path(&detok.addr)).exists(),
            "bind creates the ipc file"
        );

        let ctx = zmq::Context::new();
        let peer = ctx.socket(zmq::PUSH).unwrap();
        peer.set_linger(0).unwrap();
        peer.connect(&detok.addr).unwrap();
        peer.send(&b"ping"[..], 0).unwrap();

        let got = t.recv_detok(2000).expect("recv");
        assert_eq!(got.as_deref(), Some(&b"ping"[..]));
        let _ = std::fs::remove_file(path(&detok.addr));
    }

    #[test]
    fn backend_connect_delivers_to_raw_pull_peer() {
        let backend = Endpoint {
            addr: addr("c"),
            role: Role::Connect,
        };
        let detok = Endpoint {
            addr: addr("d"),
            role: Role::Bind,
        };

        let ctx = zmq::Context::new();
        let peer = ctx.socket(zmq::PULL).unwrap();
        peer.set_linger(0).unwrap();
        peer.bind(&backend.addr).unwrap();

        let t = ZmqTransport::open(&backend, &detok).expect("open");
        t.send_backend(b"pong").expect("send");

        assert!(
            peer.poll(zmq::POLLIN, 2000).unwrap() > 0,
            "peer got nothing"
        );
        assert_eq!(peer.recv_bytes(0).unwrap(), b"pong");
        let _ = std::fs::remove_file(path(&backend.addr));
        let _ = std::fs::remove_file(path(&detok.addr));
    }

    #[test]
    fn recv_detok_times_out_with_none() {
        let backend = Endpoint {
            addr: addr("e"),
            role: Role::Connect,
        };
        let detok = Endpoint {
            addr: addr("f"),
            role: Role::Bind,
        };
        let t = ZmqTransport::open(&backend, &detok).expect("open");
        assert_eq!(t.recv_detok(200).expect("recv"), None);
        let _ = std::fs::remove_file(path(&detok.addr));
    }

    #[test]
    fn scheduler_side_round_trips_with_split_frontend() {
        // Scheduler side mirrors the frontend's roles: backend Bind (PULL),
        // detok Connect (PUSH) vs. the frontend's backend Connect (PUSH),
        // detok Bind (PULL).
        let backend_addr = addr("g");
        let detok_addr = addr("h");

        let scheduler = ZmqSchedulerTransport::open(
            &Endpoint {
                addr: backend_addr.clone(),
                role: Role::Bind,
            },
            &Endpoint {
                addr: detok_addr.clone(),
                role: Role::Connect,
            },
        )
        .expect("open scheduler transport");

        let frontend = ZmqTransport::open(
            &Endpoint {
                addr: backend_addr.clone(),
                role: Role::Connect,
            },
            &Endpoint {
                addr: detok_addr.clone(),
                role: Role::Bind,
            },
        )
        .expect("open frontend transport");
        let (frontend_tx, frontend_rx) = frontend.split();

        // A freshly connected PUSH socket's connection is established
        // asynchronously by libzmq's io thread; a send issued immediately
        // after `connect()` from a just-spawned OS thread can race that
        // attach and be silently dropped (confirmed with a minimal
        // repro outside this test suite). Retry sending "ping" until the
        // scheduler side observes it, bounded by an overall deadline, so
        // the test proves the split halves work without being racy.
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_sender = std::sync::Arc::clone(&stop);
        let send_handle = std::thread::spawn(move || {
            while !stop_sender.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = frontend_tx.send_backend(b"ping");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        });
        let recv_handle = std::thread::spawn(move || frontend_rx.recv_detok(2000));

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let got = loop {
            match scheduler.recv_backend(50).expect("recv backend") {
                Some(frame) => break frame,
                None => assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for ping over the split frontend"
                ),
            }
        };
        assert_eq!(got, b"ping");
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        send_handle.join().expect("send thread panicked");

        scheduler.send_detok(b"pong").expect("send detok");
        let got = recv_handle
            .join()
            .expect("recv thread panicked")
            .expect("recv detok");
        assert_eq!(got.as_deref(), Some(&b"pong"[..]));

        let _ = std::fs::remove_file(path(&backend_addr));
        let _ = std::fs::remove_file(path(&detok_addr));
    }
}
