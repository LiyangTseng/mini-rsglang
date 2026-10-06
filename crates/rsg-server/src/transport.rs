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

/// Frames to the scheduler (backend PUSH) and from it (detokenizer PULL).
pub trait Transport: Send {
    #[cfg_attr(not(test), allow(dead_code))]
    fn send_backend(&self, frame: &[u8]) -> anyhow::Result<()>;
    /// Wait up to `timeout_ms` for one frame; `Ok(None)` on timeout.
    #[cfg_attr(not(test), allow(dead_code))]
    fn recv_detok(&self, timeout_ms: i64) -> anyhow::Result<Option<Vec<u8>>>;
}

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
}

impl Transport for ZmqTransport {
    fn send_backend(&self, frame: &[u8]) -> anyhow::Result<()> {
        self.backend
            .send(frame, 0)
            .context("send on backend socket")
    }

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
}
