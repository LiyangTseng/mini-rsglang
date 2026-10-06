#![allow(dead_code)]
//! The shared HTTP test harness (reused by plans 05-03, 05-04, 05-06,
//! 05-07): starts a real `mock-scheduler` subprocess, wires rsg-server's
//! writer/dispatcher/engine/http router onto it in-process, and serves the
//! router on a real `127.0.0.1` TCP port.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rsg_server::codec::{ChatRole, CodecError, IncrementalDecoder, Prompt, TextCodec};
use rsg_server::dispatch::spawn_dispatcher;
use rsg_server::engine::{AbortTiming, Engine, EngineConfig};
use rsg_server::http::{self, AppState};
use rsg_server::writer::spawn_writer;

use super::MockScheduler;

/// A `TextCodec` test double, never used in production. `encode` treats
/// `Prompt::Text` as its raw UTF-8 bytes (one id per byte, 0..=255) and
/// `Prompt::Chat` as `"{role}: {content}\n"` per message, concatenated — a
/// test-only rendering, not upstream's chat template. Its decoder buffers
/// bytes and flushes the longest newly-completed valid UTF-8 prefix on
/// every step; on `finished` it flushes any remainder with
/// `String::from_utf8_lossy`. An id outside `0..=255` is a `CodecError`.
pub struct ByteCodec;

impl TextCodec for ByteCodec {
    fn encode(&self, prompt: &Prompt) -> Result<Vec<i32>, CodecError> {
        let bytes: Vec<u8> = match prompt {
            Prompt::Text(s) => s.as_bytes().to_vec(),
            Prompt::Chat(msgs) => {
                let mut out = Vec::new();
                for m in msgs {
                    out.extend_from_slice(role_str(m.role).as_bytes());
                    out.extend_from_slice(b": ");
                    out.extend_from_slice(m.content.as_bytes());
                    out.push(b'\n');
                }
                out
            }
        };
        Ok(bytes.into_iter().map(i32::from).collect())
    }

    fn decoder(&self) -> Box<dyn IncrementalDecoder> {
        Box::new(ByteDecoder { buf: Vec::new() })
    }
}

fn role_str(role: ChatRole) -> &'static str {
    role.as_str()
}

struct ByteDecoder {
    buf: Vec<u8>,
}

impl IncrementalDecoder for ByteDecoder {
    fn step(&mut self, next_token: i64, finished: bool) -> Result<String, CodecError> {
        let byte = u8::try_from(next_token)
            .map_err(|_| CodecError(format!("token {next_token} is outside 0..=255")))?;
        self.buf.push(byte);

        let valid_len = match std::str::from_utf8(&self.buf) {
            Ok(_) => self.buf.len(),
            Err(e) => e.valid_up_to(),
        };
        let mut out = String::from_utf8(self.buf[..valid_len].to_vec())
            .expect("valid_len is always a valid UTF-8 boundary");
        self.buf.drain(..valid_len);

        if finished && !self.buf.is_empty() {
            out.push_str(&String::from_utf8_lossy(&self.buf));
            self.buf.clear();
        }
        Ok(out)
    }
}

/// Per-test engine configuration for [`TestServer::start`].
#[derive(Clone, Copy, Debug)]
pub struct TestConfig {
    pub abort_timing: AbortTiming,
    pub backend_timeout_ms: u64,
}

impl Default for TestConfig {
    fn default() -> TestConfig {
        TestConfig {
            abort_timing: AbortTiming::Immediate,
            backend_timeout_ms: 5000,
        }
    }
}

/// An in-process axum router (bound to a real TCP port) backed by a
/// spawned `mock-scheduler` subprocess.
pub struct TestServer {
    pub addr: SocketAddr,
    pub mock: MockScheduler,
    pub engine: Arc<Engine>,
    pub state: AppState,
}

impl TestServer {
    /// Spawns `mock-scheduler` with `mock_args`, wires the writer/dispatcher
    /// onto its transport, builds an `Engine` from `config` plus the
    /// handshake's `max_seq_len`, and serves the router on an OS-assigned
    /// `127.0.0.1` port. Must run inside a multi-thread tokio runtime:
    /// spawning and awaiting the mock's handshake runs inside
    /// `block_in_place`, which panics on a current-thread runtime.
    pub async fn start(mock_args: &[&str], config: TestConfig) -> TestServer {
        let (mock, handshake) = tokio::task::block_in_place(|| {
            let mut mock = MockScheduler::spawn(mock_args);
            let handshake = mock.wait_ready();
            (mock, handshake)
        });

        let (tx, rx) = mock.frontend().split();
        let (writer, _writer_join) = spawn_writer(tx).expect("spawn writer");
        let dispatch = spawn_dispatcher(rx).expect("spawn dispatcher");

        let engine_config = EngineConfig {
            max_seq_len: handshake.max_seq_len,
            abort_timing: config.abort_timing,
            backend_timeout: Duration::from_millis(config.backend_timeout_ms),
        };
        let codec: Arc<dyn TextCodec> = Arc::new(ByteCodec);
        let engine = Engine::new(writer, dispatch, codec, engine_config);

        let state = AppState::new("test-model");
        state.set_engine(Arc::clone(&engine));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("local_addr");
        tokio::spawn(http::serve(listener, state.clone()));

        TestServer {
            addr,
            mock,
            engine,
            state,
        }
    }
}
