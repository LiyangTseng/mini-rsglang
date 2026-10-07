//! rsg-server: the Rust frontend. Loads Phase 4's tokenizer for `--model`,
//! serves HTTP from startup (`/health` answers immediately, `/health/ready`
//! and the generation endpoints flip once the readiness handshake arrives
//! on stdin), and becomes the real engine only after that handshake
//! succeeds.

use std::io::BufRead;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use rsg_server::codec::TextCodec;
use rsg_server::dispatch::spawn_dispatcher;
use rsg_server::engine::{AbortTiming, DEFAULT_BACKEND_TIMEOUT_MS, Engine, EngineConfig};
use rsg_server::fsm::spawn_registry;
use rsg_server::handshake;
use rsg_server::hf_codec::HfCodec;
use rsg_server::http::{self, AppState};
use rsg_server::metrics::ServerMetrics;
use rsg_server::transport::{Endpoint, Role, ZmqTransport};
use rsg_server::writer::spawn_writer;

/// SIGINT or SIGTERM.
const EXIT_OK: i32 = 0;
/// Socket, HTTP-listener, tokenizer-load or signal-handler setup failed.
const EXIT_STARTUP: i32 = 1;
/// Malformed handshake, unsupported version or upstream SHA mismatch.
const EXIT_BAD_HANDSHAKE: i32 = 2;
/// stdin closed before or after the handshake: the launcher went away (D-12).
const EXIT_STDIN_EOF: i32 = 3;

/// Static configuration, passed by the launcher at spawn (D-11).
#[derive(Parser, Debug)]
#[command(name = "rsg-server", about = "mini-rsglang Rust frontend")]
struct Cli {
    /// Full address of the scheduler's backend endpoint (minisgl_0).
    #[arg(long, value_name = "ADDR")]
    backend_addr: String,
    /// Whether to bind or connect the backend endpoint.
    #[arg(long, value_enum)]
    backend_role: Role,
    /// Full address of the detokenizer endpoint (minisgl_1).
    #[arg(long, value_name = "ADDR")]
    detok_addr: String,
    /// Whether to bind or connect the detokenizer endpoint.
    #[arg(long, value_enum)]
    detok_role: Role,
    /// Model path or Hugging Face id.
    #[arg(long, value_name = "MODEL")]
    model: String,
    /// Run id / ipc suffix.
    #[arg(long, value_name = "ID")]
    run_id: String,
    /// HTTP listen host (upstream's own default).
    #[arg(long, value_name = "HOST", default_value = "127.0.0.1")]
    host: String,
    /// HTTP listen port (upstream's own default).
    #[arg(long, default_value_t = 1919)]
    port: u16,
    /// Server-wide abort-timing mode (LIFE-05, CONTEXT D-01): `immediate`
    /// aborts a cancelled request as soon as it is observed; `deferred`
    /// waits for the first token first. Exists because of the suspected
    /// abort-during-prefill double-free bug named in STATE.md's Phase 6
    /// blocker -- Phase 6/7 must run both frontends under the same setting
    /// for a fair comparison.
    #[arg(long, value_enum, default_value_t = AbortTiming::Immediate)]
    abort_timing: AbortTiming,
    /// Backend-inactivity timeout (LIFE-04), in milliseconds: a request
    /// whose backend has gone silent for this long ends in error instead
    /// of hanging forever.
    #[arg(
        long,
        default_value_t = DEFAULT_BACKEND_TIMEOUT_MS,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    backend_timeout_ms: u64,
}

/// What the stdin reader thread reports.
enum StdinEvent {
    Line(String),
    Eof,
    Error(String),
}

/// Read stdin on a dedicated OS thread: tokio's stdin is a blocking read that
/// cannot be cancelled and would hang runtime shutdown (RESEARCH Pattern 5).
fn spawn_stdin_reader() -> mpsc::Receiver<StdinEvent> {
    let (tx, rx) = mpsc::channel(16);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let event = match line {
                Ok(line) => StdinEvent::Line(line),
                Err(e) => {
                    let _ = tx.blocking_send(StdinEvent::Error(e.to_string()));
                    return;
                }
            };
            if tx.blocking_send(event).is_err() {
                return;
            }
        }
        let _ = tx.blocking_send(StdinEvent::Eof);
    });
    rx
}

fn install_signal(kind: SignalKind, name: &str) -> Signal {
    match signal(kind) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("failed to install {name} handler: {e}");
            std::process::exit(EXIT_STARTUP);
        }
    }
}

/// Exit without running destructors, so no zmq context term or blocked stdin
/// read can hang shutdown (sockets use linger 0).
fn exit_on_signal(name: &str) -> ! {
    tracing::info!("received {name}; exiting");
    std::process::exit(EXIT_OK);
}

/// Logs why the HTTP server's serving task ended -- this should never
/// happen while the process is otherwise healthy, so it is always treated
/// as a startup-class failure (EXIT_STARTUP) by the caller.
fn log_serve_ended(result: Result<std::io::Result<()>, tokio::task::JoinError>) {
    match result {
        Ok(Ok(())) => tracing::error!("http server stopped serving unexpectedly"),
        Ok(Err(e)) => tracing::error!("http server error: {e}"),
        Err(e) => tracing::error!("http server task panicked: {e}"),
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        backend_addr = %cli.backend_addr,
        backend_role = %cli.backend_role,
        detok_addr = %cli.detok_addr,
        detok_role = %cli.detok_role,
        model = %cli.model,
        run_id = %cli.run_id,
        host = %cli.host,
        port = cli.port,
        "rsg-server starting"
    );

    let mut sigint = install_signal(SignalKind::interrupt(), "SIGINT");
    let mut sigterm = install_signal(SignalKind::terminate(), "SIGTERM");

    // Loads in parallel with the backend's own weight loading (it has no
    // dependency on the handshake), so it never adds to end-to-end cold
    // start (Phase 7 scenario 3). A load failure means the server can never
    // serve a real request, so it exits before ever looking alive.
    let codec_start = Instant::now();
    let codec = match HfCodec::load(&cli.model) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("failed to load tokenizer for {:?}: {e:#}", cli.model);
            std::process::exit(EXIT_STARTUP);
        }
    };
    tracing::info!(
        model = %cli.model,
        elapsed_ms = codec_start.elapsed().as_millis() as u64,
        "tokenizer loaded"
    );
    let codec: Arc<dyn TextCodec> = Arc::new(codec);

    let backend = Endpoint {
        addr: cli.backend_addr.clone(),
        role: cli.backend_role,
    };
    let detok = Endpoint {
        addr: cli.detok_addr.clone(),
        role: cli.detok_role,
    };
    // Nothing is sent before the handshake (D-10): the transport is opened
    // and split into the tx-zmq/rx-zmq halves now, but no engine exists
    // yet, and the writer/dispatcher threads only relay what they are
    // asked to send.
    let transport = match ZmqTransport::open(&backend, &detok) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("failed to open sockets: {e:#}");
            std::process::exit(EXIT_STARTUP);
        }
    };
    tracing::info!("sockets ready");

    let (backend_tx, detok_rx) = transport.split();
    let (writer, _writer_join) = match spawn_writer(backend_tx) {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("failed to spawn tx-zmq writer: {e}");
            std::process::exit(EXIT_STARTUP);
        }
    };
    let dispatch = match spawn_dispatcher(detok_rx) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("failed to spawn rx-zmq dispatcher: {e}");
            std::process::exit(EXIT_STARTUP);
        }
    };

    let metrics = ServerMetrics::new();
    let state = AppState::new(cli.model.clone(), metrics.clone());

    let listener = match tokio::net::TcpListener::bind((cli.host.as_str(), cli.port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("failed to bind {}:{}: {e}", cli.host, cli.port);
            std::process::exit(EXIT_STARTUP);
        }
    };
    let local_addr = match listener.local_addr() {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("failed to read listener's local_addr: {e}");
            std::process::exit(EXIT_STARTUP);
        }
    };
    tracing::info!(addr = %local_addr, "http server listening");

    let mut serve_join = tokio::spawn(http::serve(listener, state.clone()));

    let mut stdin = spawn_stdin_reader();
    tracing::info!("awaiting handshake on stdin");
    let hs = tokio::select! {
        event = stdin.recv() => match event {
            Some(StdinEvent::Line(line)) => {
                match handshake::parse_handshake(&line, handshake::EXPECTED_UPSTREAM_SHA) {
                    Ok(hs) => {
                        tracing::info!(
                            max_seq_len = hs.max_seq_len,
                            eos_token_id = %hs.eos_display(),
                            page_size = hs.page_size,
                            max_running_req = hs.max_running_req,
                            num_pages = hs.num_pages,
                            upstream_sha = %hs.upstream_sha,
                            "handshake received"
                        );
                        hs
                    }
                    Err(e) => {
                        tracing::error!("handshake rejected: {e}");
                        std::process::exit(EXIT_BAD_HANDSHAKE);
                    }
                }
            }
            Some(StdinEvent::Error(e)) => {
                tracing::error!("launcher went away (stdin EOF) before handshake: {e}");
                std::process::exit(EXIT_STDIN_EOF);
            }
            Some(StdinEvent::Eof) | None => {
                tracing::error!("launcher went away (stdin EOF) before handshake");
                std::process::exit(EXIT_STDIN_EOF);
            }
        },
        _ = sigint.recv() => exit_on_signal("SIGINT"),
        _ = sigterm.recv() => exit_on_signal("SIGTERM"),
        result = &mut serve_join => {
            log_serve_ended(result);
            std::process::exit(EXIT_STARTUP);
        }
    };

    let engine_config = EngineConfig {
        max_seq_len: hs.max_seq_len,
        abort_timing: cli.abort_timing,
        backend_timeout: Duration::from_millis(cli.backend_timeout_ms),
    };
    let registry = spawn_registry(metrics);
    let engine = Engine::new(writer, dispatch, codec, registry, engine_config);
    state.set_engine(engine);
    tracing::info!(
        abort_timing = ?cli.abort_timing,
        backend_timeout_ms = cli.backend_timeout_ms,
        "ready to serve"
    );

    tracing::info!("idle until SIGINT/SIGTERM or stdin EOF");
    loop {
        tokio::select! {
            event = stdin.recv() => match event {
                Some(StdinEvent::Line(_)) => tracing::warn!("ignoring unexpected stdin line"),
                Some(StdinEvent::Error(e)) => {
                    tracing::error!("launcher went away (stdin EOF): {e}");
                    std::process::exit(EXIT_STDIN_EOF);
                }
                Some(StdinEvent::Eof) | None => {
                    tracing::error!("launcher went away (stdin EOF)");
                    std::process::exit(EXIT_STDIN_EOF);
                }
            },
            _ = sigint.recv() => exit_on_signal("SIGINT"),
            _ = sigterm.recv() => exit_on_signal("SIGTERM"),
            result = &mut serve_join => {
                log_serve_ended(result);
                std::process::exit(EXIT_STARTUP);
            }
        }
    }
}
