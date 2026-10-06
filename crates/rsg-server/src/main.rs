//! rsg-server: the Rust frontend. In Phase 1 a skeleton that opens its two ZMQ
//! sockets, reads the readiness handshake and idles (D-08).

mod handshake;
mod transport;

use std::io::BufRead;

use clap::Parser;
use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use transport::{Endpoint, Role, ZmqTransport};

/// SIGINT or SIGTERM.
const EXIT_OK: i32 = 0;
/// Socket or signal-handler setup failed.
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
        "rsg-server starting"
    );

    let mut sigint = install_signal(SignalKind::interrupt(), "SIGINT");
    let mut sigterm = install_signal(SignalKind::terminate(), "SIGTERM");

    let backend = Endpoint {
        addr: cli.backend_addr.clone(),
        role: cli.backend_role,
    };
    let detok = Endpoint {
        addr: cli.detok_addr.clone(),
        role: cli.detok_role,
    };
    // Kept alive until process exit. Phase 1 never sends on it: nothing may
    // reach the scheduler before the handshake (D-10), and the skeleton sends nothing.
    let _transport = match ZmqTransport::open(&backend, &detok) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("failed to open sockets: {e:#}");
            std::process::exit(EXIT_STARTUP);
        }
    };
    tracing::info!("sockets ready");

    let mut stdin = spawn_stdin_reader();
    tracing::info!("awaiting handshake on stdin");
    tokio::select! {
        event = stdin.recv() => match event {
            Some(StdinEvent::Line(line)) => {
                match handshake::parse_handshake(&line, handshake::EXPECTED_UPSTREAM_SHA) {
                    Ok(hs) => tracing::info!(
                        max_seq_len = hs.max_seq_len,
                        eos_token_id = %hs.eos_display(),
                        page_size = hs.page_size,
                        max_running_req = hs.max_running_req,
                        num_pages = hs.num_pages,
                        upstream_sha = %hs.upstream_sha,
                        "handshake received"
                    ),
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
    }

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
        }
    }
}
