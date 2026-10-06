//! mock-scheduler: a stand-in for the real scheduler at the ipc boundary
//! (D-08). It speaks only rsg-wire's existing `BackendMsg`/`TokenizerMsg`
//! types; readiness goes out as the Phase 1 handshake JSON line on stdout,
//! never as a new wire message. It emits deterministic echo tokens with
//! fixed, uniform prefill/decode delays (D-10). It does no prefix or radix
//! modeling (deferred to v2).

use std::collections::BTreeMap;
use std::io::Write as _;
use std::time::{Duration, Instant};

use clap::Parser;
use tracing_subscriber::EnvFilter;

use rsg_server::handshake::{EXPECTED_UPSTREAM_SHA, HANDSHAKE_VERSION, Handshake};
use rsg_server::transport::{Endpoint, Role, ZmqSchedulerTransport};
use rsg_wire::{BackendMsg, TokenizerMsg};

/// Clean stop: `ExitMsg` received.
const EXIT_OK: i32 = 0;
/// Socket setup, or a send/encode on the detokenizer socket, failed.
const EXIT_STARTUP: i32 = 1;
/// A backend frame could not be decoded, or a `UserMsg`'s `input_ids` tensor
/// was malformed. Mirrors the real scheduler dying on a message it cannot
/// decode.
const EXIT_BAD_FRAME: i32 = 4;

/// `eos_token_id` this mock reports in its handshake.
const MOCK_EOS_TOKEN_ID: u64 = 151645;
const MOCK_PAGE_SIZE: u64 = 16;
const MOCK_MAX_RUNNING_REQ: u64 = 256;
const MOCK_NUM_PAGES: u64 = 4096;
/// How long the engine blocks on `recv_backend` while no request is due.
const IDLE_POLL_MS: i64 = 100;

/// Static configuration, passed by a test harness at spawn.
#[derive(Parser, Debug)]
#[command(name = "mock-scheduler", about = "mini-rsglang GPU-free mock backend")]
struct Cli {
    /// Full address of the backend (PULL) endpoint.
    #[arg(long, value_name = "ADDR")]
    backend_addr: String,
    /// Whether to bind or connect the backend endpoint.
    #[arg(long, value_enum)]
    backend_role: Role,
    /// Full address of the detokenizer (PUSH) endpoint.
    #[arg(long, value_name = "ADDR")]
    detok_addr: String,
    /// Whether to bind or connect the detokenizer endpoint.
    #[arg(long, value_enum)]
    detok_role: Role,
    /// Fixed delay before a request's first token (D-10).
    #[arg(long, default_value_t = 0)]
    prefill_delay_ms: u64,
    /// Fixed delay between a request's consecutive tokens (D-10).
    #[arg(long, default_value_t = 0)]
    decode_delay_ms: u64,
    /// Reported in the handshake; also governs nothing else in this mock.
    #[arg(long, default_value_t = 4096)]
    max_seq_len: u64,
}

/// One in-flight request the engine is emitting echo tokens for.
struct Running {
    /// The request's prompt token ids, used by the echo rule.
    prompt_ids: Vec<i32>,
    /// Total tokens this request will emit: `max(sampling_params.max_tokens, 1)`.
    total: i64,
    /// Tokens already emitted for this request (0-based index of the next one).
    emitted: i64,
    /// When this request's next token is due.
    due: Instant,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let backend = Endpoint {
        addr: cli.backend_addr.clone(),
        role: cli.backend_role,
    };
    let detok = Endpoint {
        addr: cli.detok_addr.clone(),
        role: cli.detok_role,
    };

    let transport = match ZmqSchedulerTransport::open(&backend, &detok) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("failed to open sockets: {e:#}");
            std::process::exit(EXIT_STARTUP);
        }
    };
    tracing::info!("sockets ready");

    let handshake = Handshake {
        handshake_version: HANDSHAKE_VERSION,
        upstream_sha: EXPECTED_UPSTREAM_SHA.to_string(),
        max_seq_len: cli.max_seq_len,
        eos_token_id: Some(MOCK_EOS_TOKEN_ID),
        page_size: MOCK_PAGE_SIZE,
        max_running_req: MOCK_MAX_RUNNING_REQ,
        num_pages: MOCK_NUM_PAGES,
    };
    println!("{}", handshake.to_json_line());
    if let Err(e) = std::io::stdout().flush() {
        tracing::error!("failed to flush stdout handshake line: {e}");
        std::process::exit(EXIT_STARTUP);
    }
    tracing::info!("mock-scheduler ready");

    let prefill_delay = Duration::from_millis(cli.prefill_delay_ms);
    let decode_delay = Duration::from_millis(cli.decode_delay_ms);

    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("mock-engine".to_string())
        .spawn(move || {
            let code = run_engine(&transport, prefill_delay, decode_delay);
            let _ = tx.send(code);
        })
        .expect("spawn mock-engine thread");

    let code = rx.await.unwrap_or(EXIT_STARTUP);
    std::process::exit(code);
}

/// The result of processing one decoded `BackendMsg`.
enum ProcessOutcome {
    /// Keep processing the rest of the batch (or wait for the next frame).
    Continue,
    /// Stop processing immediately (including the rest of an enclosing
    /// batch) and exit with this code.
    Exit(i32),
}

/// The engine loop: (1) waits for a frame or the next due time, (2) takes one
/// step clock `now`, (3) drains every available backend frame and processes
/// them with arrival time `now`, (4) makes every running request whose due
/// time is <= `now` emit exactly one token, in ascending uid order.
fn run_engine(
    transport: &ZmqSchedulerTransport,
    prefill_delay: Duration,
    decode_delay: Duration,
) -> i32 {
    let mut running: BTreeMap<i64, Running> = BTreeMap::new();

    loop {
        let timeout_ms = next_wait_ms(&running);
        let first = match transport.recv_backend(timeout_ms) {
            Ok(frame) => frame,
            Err(e) => {
                tracing::error!("poll/recv backend socket: {e:#}");
                return EXIT_STARTUP;
            }
        };
        let now = Instant::now();

        if let Some(frame) = first {
            let mut frames = vec![frame];
            loop {
                match transport.recv_backend(0) {
                    Ok(Some(f)) => frames.push(f),
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!("poll/recv backend socket: {e:#}");
                        return EXIT_STARTUP;
                    }
                }
            }
            for frame in frames {
                let msg = match rsg_wire::decode_backend(&frame) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::error!("decode backend frame: {e}");
                        return EXIT_BAD_FRAME;
                    }
                };
                match process_msg(msg, now, prefill_delay, &mut running) {
                    Ok(ProcessOutcome::Continue) => {}
                    Ok(ProcessOutcome::Exit(code)) => return code,
                    Err(()) => return EXIT_BAD_FRAME,
                }
            }
        }

        if let Err(code) = emit_due_tokens(transport, now, decode_delay, &mut running) {
            return code;
        }
    }
}

/// Ms until the earliest due request (0 if already overdue), or
/// `IDLE_POLL_MS` when nothing is running.
fn next_wait_ms(running: &BTreeMap<i64, Running>) -> i64 {
    match running.values().map(|r| r.due).min() {
        Some(due) => {
            let now = Instant::now();
            if due <= now {
                0
            } else {
                i64::try_from((due - now).as_millis()).unwrap_or(i64::MAX)
            }
        }
        None => IDLE_POLL_MS,
    }
}

/// Processes one decoded message, mirroring upstream's
/// `scheduler.py::_process_one_msg`. `Err(())` means the frame's payload
/// (a `UserMsg`'s tensor) was malformed; the caller maps that to
/// `EXIT_BAD_FRAME`.
fn process_msg(
    msg: BackendMsg,
    now: Instant,
    prefill_delay: Duration,
    running: &mut BTreeMap<i64, Running>,
) -> Result<ProcessOutcome, ()> {
    match msg {
        BackendMsg::BatchBackendMsg { data } => {
            for item in data {
                match process_msg(item, now, prefill_delay, running)? {
                    ProcessOutcome::Continue => {}
                    exit @ ProcessOutcome::Exit(_) => return Ok(exit),
                }
            }
            Ok(ProcessOutcome::Continue)
        }
        BackendMsg::ExitMsg {} => Ok(ProcessOutcome::Exit(EXIT_OK)),
        BackendMsg::UserMsg {
            uid,
            input_ids,
            sampling_params,
        } => {
            let prompt_ids = input_ids.to_i32_vec().map_err(|e| {
                tracing::error!(uid, "decode UserMsg input_ids tensor: {e}");
            })?;
            if running.contains_key(&uid) {
                tracing::warn!(uid, "UserMsg for already-running uid; replacing");
            }
            running.insert(
                uid,
                Running {
                    prompt_ids,
                    total: sampling_params.max_tokens.max(1),
                    emitted: 0,
                    due: now + prefill_delay,
                },
            );
            Ok(ProcessOutcome::Continue)
        }
        BackendMsg::AbortBackendMsg { uid } => {
            running.remove(&uid);
            Ok(ProcessOutcome::Continue)
        }
    }
}

/// Emits exactly one token for every request whose due time is <= `now`, in
/// ascending uid order, using the echo rule: token k (0-based) is
/// `prompt_ids[k % len]`, or 0 for an empty prompt. `Err(code)` means a
/// send/encode failed and the engine should exit with `code`.
fn emit_due_tokens(
    transport: &ZmqSchedulerTransport,
    now: Instant,
    decode_delay: Duration,
    running: &mut BTreeMap<i64, Running>,
) -> Result<(), i32> {
    let due_uids: Vec<i64> = running
        .iter()
        .filter(|(_, r)| r.due <= now)
        .map(|(uid, _)| *uid)
        .collect();

    for uid in due_uids {
        let Some(r) = running.get_mut(&uid) else {
            continue;
        };
        let next_token = if r.prompt_ids.is_empty() {
            0
        } else {
            let idx = (r.emitted as usize) % r.prompt_ids.len();
            r.prompt_ids[idx] as i64
        };
        r.emitted += 1;
        let finished = r.emitted >= r.total;

        let msg = TokenizerMsg::DetokenizeMsg {
            uid,
            next_token,
            finished,
        };
        let bytes = match rsg_wire::encode_tokenizer(&msg) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(uid, "encode DetokenizeMsg: {e}");
                return Err(EXIT_STARTUP);
            }
        };
        if let Err(e) = transport.send_detok(&bytes) {
            tracing::error!(uid, "send DetokenizeMsg: {e:#}");
            return Err(EXIT_STARTUP);
        }

        if finished {
            running.remove(&uid);
        } else if let Some(r) = running.get_mut(&uid) {
            r.due = now + decode_delay;
        }
    }
    Ok(())
}
