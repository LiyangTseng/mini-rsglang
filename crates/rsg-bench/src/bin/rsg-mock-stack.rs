//! `rsg-mock-stack`: a Mac launcher adapter (D-03) that brings up
//! `mock-scheduler` and `rsg-server` on matched `ipc://` addresses, relays
//! the mock's stdout handshake line verbatim to `rsg-server`'s stdin (the
//! same relay `python/rsglang/launch.py::run_rust_mode` performs against
//! the real scheduler), and tears both children down together.
//!
//! This lets `rsg-bench`'s harness launch the project's real Rust frontend
//! on a GPU-free Mac, with `rsg-mock-stack` itself standing in for the
//! `python -m rsglang.launch --frontend rust` command line. It does not
//! modify or re-implement `crates/rsg-server` or `crates/rsg-server/src/bin/
//! mock-scheduler.rs` (D-02): it only spawns the two binaries and pipes
//! between them.
//!
//! The two children are spawned with no `process_group` override, so they
//! inherit this process's own process group (T-07-24): when the harness
//! wraps `rsg-mock-stack` itself in a fresh group (`rsg_bench::procs::launch`),
//! that same group covers both children, and the harness's own `killpg`
//! teardown reaches them without this binary needing to do anything special.

use std::process::Stdio;
use std::time::{Duration, Instant};

use clap::Parser;
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, Command};
use tokio::signal::unix::{SignalKind, signal as unix_signal};
use tracing_subscriber::EnvFilter;

/// A signal-initiated stop after both children were brought up.
const EXIT_OK: i32 = 0;
/// A child exited on its own, or startup (spawn, handshake wait/relay) failed.
const EXIT_FAILED: i32 = 1;

/// How long to wait for `mock-scheduler`'s stdout handshake line.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
/// Grace period after SIGTERM before escalating to SIGKILL at teardown.
const TEARDOWN_GRACE: Duration = Duration::from_secs(5);

#[derive(Parser, Debug)]
#[command(
    name = "rsg-mock-stack",
    about = "Mac launcher adapter: mock-scheduler + rsg-server, handshake relayed (D-03)"
)]
struct Cli {
    /// Substituted for every literal `{port}` inside an `--rsg-server-arg` value.
    #[arg(long)]
    port: u16,
    /// Extra `rsg-server` argument (repeatable, one argv token per
    /// occurrence); any `{port}` inside it is substituted.
    #[arg(long = "rsg-server-arg")]
    rsg_server_args: Vec<String>,
    /// Path to the `rsg-server` binary.
    #[arg(long, default_value = "target/debug/rsg-server")]
    rsg_server_bin: String,
    /// Path to the `mock-scheduler` binary.
    #[arg(long, default_value = "target/debug/mock-scheduler")]
    mock_scheduler_bin: String,
    /// `rsg-server`'s `--model`.
    #[arg(long, default_value = "Qwen/Qwen3-0.6B")]
    model: String,
    /// `mock-scheduler`'s `--prefill-delay-ms` (passed through unchanged).
    #[arg(long, default_value_t = 0)]
    prefill_delay_ms: u64,
    /// `mock-scheduler`'s `--decode-delay-ms` (passed through unchanged).
    #[arg(long, default_value_t = 0)]
    decode_delay_ms: u64,
    /// `mock-scheduler`'s `--max-seq-len` (passed through unchanged).
    #[arg(long, default_value_t = 4096)]
    max_seq_len: u64,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let cli = Cli::parse();
    std::process::exit(run(cli).await);
}

/// Forwards every line of `stderr` as `[{label}] <line>` until EOF.
fn spawn_stderr_forwarder(stderr: ChildStderr, label: &'static str) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("[{label}] {line}");
        }
    });
}

/// Removes the `ipc://` socket file at `addr`, if any. Never an error: the
/// peer that bound it may already have removed it, or it may never have
/// existed if startup failed early.
fn remove_ipc_socket(addr: &str) {
    if let Some(path) = addr.strip_prefix("ipc://") {
        let _ = std::fs::remove_file(path);
    }
}

/// Sends `sig` to `child`'s pid, ignoring "already gone" (`ESRCH`).
fn send_signal(child: &Child, sig: Signal) {
    if let Some(pid) = child.id() {
        let _ = signal::kill(Pid::from_raw(pid as i32), sig);
    }
}

/// SIGTERM both children, poll up to [`TEARDOWN_GRACE`] for them to exit on
/// their own, SIGKILL any survivor, then reap both.
async fn stop_children(mock: &mut Child, rsg: &mut Child) {
    send_signal(mock, Signal::SIGTERM);
    send_signal(rsg, Signal::SIGTERM);

    let deadline = Instant::now() + TEARDOWN_GRACE;
    loop {
        let mock_done = matches!(mock.try_wait(), Ok(Some(_)));
        let rsg_done = matches!(rsg.try_wait(), Ok(Some(_)));
        if mock_done && rsg_done {
            break;
        }
        if Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    if !matches!(mock.try_wait(), Ok(Some(_))) {
        send_signal(mock, Signal::SIGKILL);
    }
    if !matches!(rsg.try_wait(), Ok(Some(_))) {
        send_signal(rsg, Signal::SIGKILL);
    }
    let _ = mock.wait().await;
    let _ = rsg.wait().await;
}

async fn run(cli: Cli) -> i32 {
    let pid = std::process::id();
    let backend_addr = format!("ipc:///tmp/rsgb-{pid}-0");
    let detok_addr = format!("ipc:///tmp/rsgb-{pid}-1");

    let mut mock = match Command::new(&cli.mock_scheduler_bin)
        .args([
            "--backend-addr",
            &backend_addr,
            "--backend-role",
            "bind",
            "--detok-addr",
            &detok_addr,
            "--detok-role",
            "connect",
            "--prefill-delay-ms",
            &cli.prefill_delay_ms.to_string(),
            "--decode-delay-ms",
            &cli.decode_delay_ms.to_string(),
            "--max-seq-len",
            &cli.max_seq_len.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rsg-mock-stack: failed to spawn mock-scheduler ({}): {e}", cli.mock_scheduler_bin);
            return EXIT_FAILED;
        }
    };

    // mock-scheduler treats a closed/EOF stdin as "my parent went away"
    // and exits immediately (EXIT_STDIN_EOF), before ever printing its
    // handshake line -- it expects its stdin piped and held open for its
    // whole lifetime, exactly like rsg-server's own stdin below. Nothing
    // is ever written to it; only keeping the write end open matters.
    // Dropped only right before teardown, mirroring `rsg_stdin` below.
    let mock_stdin = mock.stdin.take().expect("mock-scheduler stdin piped");
    let mock_pid = mock.id().unwrap_or(0);
    let mock_stderr = mock.stderr.take().expect("mock-scheduler stderr piped");
    spawn_stderr_forwarder(mock_stderr, "mock-scheduler");
    let mut mock_stdout = BufReader::new(mock.stdout.take().expect("mock-scheduler stdout piped"));

    let mut handshake_line = String::new();
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, mock_stdout.read_line(&mut handshake_line)).await {
        Ok(Ok(0)) => {
            eprintln!("rsg-mock-stack: mock-scheduler closed stdout before sending its handshake line");
            drop(mock_stdin);
            let _ = mock.start_kill();
            let _ = mock.wait().await;
            remove_ipc_socket(&backend_addr);
            remove_ipc_socket(&detok_addr);
            return EXIT_FAILED;
        }
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            eprintln!("rsg-mock-stack: failed to read mock-scheduler's handshake line: {e}");
            drop(mock_stdin);
            let _ = mock.start_kill();
            let _ = mock.wait().await;
            remove_ipc_socket(&backend_addr);
            remove_ipc_socket(&detok_addr);
            return EXIT_FAILED;
        }
        Err(_) => {
            eprintln!(
                "rsg-mock-stack: timed out after {:?} waiting for mock-scheduler's handshake line",
                HANDSHAKE_TIMEOUT
            );
            drop(mock_stdin);
            let _ = mock.start_kill();
            let _ = mock.wait().await;
            remove_ipc_socket(&backend_addr);
            remove_ipc_socket(&detok_addr);
            return EXIT_FAILED;
        }
    }

    let port_str = cli.port.to_string();
    let mut rsg_args: Vec<String> = vec![
        "--backend-addr".to_string(),
        backend_addr.clone(),
        "--backend-role".to_string(),
        "connect".to_string(),
        "--detok-addr".to_string(),
        detok_addr.clone(),
        "--detok-role".to_string(),
        "bind".to_string(),
        "--model".to_string(),
        cli.model.clone(),
        "--run-id".to_string(),
        format!("rsgb-{pid}"),
    ];
    for a in &cli.rsg_server_args {
        rsg_args.push(a.replace("{port}", &port_str));
    }

    let mut rsg = match Command::new(&cli.rsg_server_bin)
        .args(&rsg_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rsg-mock-stack: failed to spawn rsg-server ({}): {e}", cli.rsg_server_bin);
            drop(mock_stdin);
            let _ = mock.start_kill();
            let _ = mock.wait().await;
            remove_ipc_socket(&backend_addr);
            remove_ipc_socket(&detok_addr);
            return EXIT_FAILED;
        }
    };

    let rsg_pid = rsg.id().unwrap_or(0);
    let rsg_stderr = rsg.stderr.take().expect("rsg-server stderr piped");
    spawn_stderr_forwarder(rsg_stderr, "rsg-server");

    eprintln!("rsg-mock-stack: children rsg-server={rsg_pid} mock-scheduler={mock_pid}");

    let mut rsg_stdin = rsg.stdin.take().expect("rsg-server stdin piped");
    if let Err(e) = rsg_stdin.write_all(handshake_line.as_bytes()).await {
        eprintln!("rsg-mock-stack: failed to write handshake to rsg-server stdin: {e}");
        drop(mock_stdin);
        stop_children(&mut mock, &mut rsg).await;
        remove_ipc_socket(&backend_addr);
        remove_ipc_socket(&detok_addr);
        return EXIT_FAILED;
    }
    if let Err(e) = rsg_stdin.flush().await {
        eprintln!("rsg-mock-stack: failed to flush handshake to rsg-server stdin: {e}");
        drop(mock_stdin);
        stop_children(&mut mock, &mut rsg).await;
        remove_ipc_socket(&backend_addr);
        remove_ipc_socket(&detok_addr);
        return EXIT_FAILED;
    }
    // Keep rsg_stdin and mock_stdin open past this point: dropping either
    // would close that child's stdin, which both rsg-server and
    // mock-scheduler treat as "the launcher went away" (EXIT_STDIN_EOF).
    // Both are dropped only right before teardown below.

    eprintln!("rsg-mock-stack: backend ready; handshake sent to rsg-server");

    let mut sigint = match unix_signal(SignalKind::interrupt()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rsg-mock-stack: failed to install SIGINT handler: {e}");
            drop(rsg_stdin);
            drop(mock_stdin);
            stop_children(&mut mock, &mut rsg).await;
            remove_ipc_socket(&backend_addr);
            remove_ipc_socket(&detok_addr);
            return EXIT_FAILED;
        }
    };
    let mut sigterm = match unix_signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rsg-mock-stack: failed to install SIGTERM handler: {e}");
            drop(rsg_stdin);
            drop(mock_stdin);
            stop_children(&mut mock, &mut rsg).await;
            remove_ipc_socket(&backend_addr);
            remove_ipc_socket(&detok_addr);
            return EXIT_FAILED;
        }
    };

    let exit_code = tokio::select! {
        _ = sigint.recv() => {
            eprintln!("rsg-mock-stack: received SIGINT; stopping children");
            EXIT_OK
        }
        _ = sigterm.recv() => {
            eprintln!("rsg-mock-stack: received SIGTERM; stopping children");
            EXIT_OK
        }
        status = mock.wait() => {
            eprintln!("rsg-mock-stack: mock-scheduler exited on its own: {status:?}");
            EXIT_FAILED
        }
        status = rsg.wait() => {
            eprintln!("rsg-mock-stack: rsg-server exited on its own: {status:?}");
            EXIT_FAILED
        }
    };

    drop(rsg_stdin);
    drop(mock_stdin);
    stop_children(&mut mock, &mut rsg).await;
    remove_ipc_socket(&backend_addr);
    remove_ipc_socket(&detok_addr);
    exit_code
}
