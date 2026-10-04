//! rsg-server: the Rust frontend. In Phase 1 a skeleton that opens its two ZMQ
//! sockets, reads the readiness handshake and idles (D-08).

mod handshake;
mod transport;

use clap::Parser;
use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::EnvFilter;

use transport::{Endpoint, Role, ZmqTransport};

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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
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

    let backend = Endpoint { addr: cli.backend_addr.clone(), role: cli.backend_role };
    let detok = Endpoint { addr: cli.detok_addr.clone(), role: cli.detok_role };
    let _transport = match ZmqTransport::open(&backend, &detok) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("failed to open sockets: {e:#}");
            std::process::exit(1);
        }
    };
    tracing::info!("sockets ready");

    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("failed to install SIGTERM handler: {e}");
            std::process::exit(1);
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => tracing::info!("received SIGINT; exiting"),
        _ = sigterm.recv() => tracing::info!("received SIGTERM; exiting"),
    }
    std::process::exit(0);
}
