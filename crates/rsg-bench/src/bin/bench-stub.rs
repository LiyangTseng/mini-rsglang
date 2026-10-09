//! `bench-stub`: a 127.0.0.1-only test-fixture HTTP server speaking the
//! OpenAI SSE chat-completion shape (D-01/D-03). It lets Phase 7's harness
//! mechanics (launch, stream, cancel, teardown) be proven on the Mac without
//! Phase 5's real HTTP API or a GPU. It speaks only this HTTP/SSE shape, not
//! the ZMQ wire protocol, so it does not duplicate `mock-scheduler`.
//!
//! Named `bench-stub` (not e.g. `bench_stub_server`) because Linux truncates
//! a process `comm` to 15 bytes, and later Phase 7 plans identify processes
//! by name.
//!
//! Every event this binary cares about is printed to stdout as one flushed
//! line prefixed `stub-event`, so the harness-side test helpers can tail the
//! log file and assert on exact event sequences.
//!
//! The disconnect proof (D-02: cancellation logic is client-side, so the
//! server must observe it) splits the connection into read/write halves once
//! the chat-completion headers are written. A watcher task reads the read
//! half until EOF/error and fires a oneshot; the writer loop selects between
//! that signal and its TTFT/ITL sleep, so a client that drops during the
//! prefill wait is noticed immediately, not only on the next write attempt.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use clap::Parser;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Duration;

/// Bad CLI arguments or failed to bind the listener.
const EXIT_STARTUP: i32 = 1;

#[derive(Parser, Debug, Clone)]
#[command(name = "bench-stub", about = "Phase 7 test-fixture HTTP/SSE server")]
struct Cli {
    /// Port to bind on 127.0.0.1 (ignored, but still accepted, with --idle).
    #[arg(long)]
    port: u16,
    /// Delay before the first SSE chunk of a chat completion, in ms.
    #[arg(long, default_value_t = 20)]
    ttft_ms: u64,
    /// Delay between subsequent SSE chunks, in ms.
    #[arg(long, default_value_t = 5)]
    itl_ms: u64,
    /// Sleep this long before binding the listener.
    #[arg(long, default_value_t = 0)]
    ready_delay_ms: u64,
    /// After this many ms, write "stub backend ready" to stderr and flush.
    #[arg(long)]
    marker_after_ms: Option<u64>,
    /// The `id` field GET /v1/models reports.
    #[arg(long, default_value = "stub-model")]
    model_id: String,
    /// Every Nth chat request (counting from 1) answers 500 instead of
    /// streaming.
    #[arg(long)]
    fail_every: Option<u64>,
    /// Spawn `current_exe() --idle --port <same>` as a child, inheriting
    /// this process's group, so teardown tests have a grandchild to reach.
    #[arg(long)]
    spawn_child: bool,
    /// Never bind a port; sleep forever. Used for the `--spawn-child` child.
    #[arg(long)]
    idle: bool,
}

fn print_event(line: &str) {
    println!("stub-event {line}");
    let _ = io::Write::flush(&mut io::stdout());
}

fn env_or_unset(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| "unset".to_string())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    print_event(&format!(
        "kind=env profile_mode={} profile_dir={} interval={} pythonpath={}",
        env_or_unset("RSGLANG_PROFILE_MODE"),
        env_or_unset("RSGLANG_PROFILE_DIR"),
        env_or_unset("RSGLANG_PROFILE_INTERVAL_S"),
        env_or_unset("PYTHONPATH"),
    ));

    // `--marker-after-ms` is relative to process start, the same clock
    // `--ready-delay-ms` uses, so plan 07-08's cold-start harness can set
    // the two independently to place the backend-ready marker before,
    // at, or after readiness. Spawning this before the ready-delay sleep
    // (rather than after the listener binds) is what makes that true --
    // otherwise the marker could only ever land at
    // `ready_delay_ms + marker_after_ms`, strictly after readiness.
    if let Some(ms) = cli.marker_after_ms {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            eprintln!("stub backend ready");
            let _ = io::Write::flush(&mut io::stderr());
        });
    }

    if cli.ready_delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(cli.ready_delay_ms)).await;
    }

    if cli.idle {
        std::future::pending::<()>().await;
        unreachable!("pending future never resolves");
    }

    let listener = match TcpListener::bind(("127.0.0.1", cli.port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bench-stub: failed to bind 127.0.0.1:{}: {e}", cli.port);
            std::process::exit(EXIT_STARTUP);
        }
    };
    print_event(&format!("kind=ready port={}", cli.port));

    if cli.spawn_child {
        // Deliberately no .process_group() override: a plain child inherits
        // this process's own process group, giving teardown a grandchild to
        // reach. Dropping the Child handle does not kill the process.
        let exe = std::env::current_exe().expect("current_exe");
        let _ = std::process::Command::new(exe)
            .args(["--idle", "--port", &cli.port.to_string()])
            .spawn();
    }

    let next_id = Arc::new(AtomicU64::new(0));
    loop {
        let (stream, _addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(_) => continue,
        };
        let cli = cli.clone();
        let next_id = next_id.clone();
        tokio::spawn(async move {
            let _ = handle_connection(stream, &cli, &next_id).await;
        });
    }
}

struct ParsedRequest {
    method: String,
    path: String,
    #[allow(dead_code)]
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

async fn read_request(stream: &mut BufReader<TcpStream>) -> io::Result<Option<ParsedRequest>> {
    let mut request_line = String::new();
    let n = stream.read_line(&mut request_line).await?;
    if n == 0 {
        return Ok(None); // client closed before sending anything
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        let n = stream.read_line(&mut line).await?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    let content_length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        stream.read_exact(&mut body).await?;
    }

    Ok(Some(ParsedRequest {
        method,
        path,
        headers,
        body,
    }))
}

async fn write_response(
    stream: &mut BufReader<TcpStream>,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

async fn handle_connection(
    stream: TcpStream,
    cli: &Cli,
    next_id: &Arc<AtomicU64>,
) -> io::Result<()> {
    let mut stream = BufReader::new(stream);
    let Some(req) = read_request(&mut stream).await? else {
        return Ok(());
    };

    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/v1/models") => {
            let body = serde_json::json!({
                "object": "list",
                "data": [{"id": cli.model_id, "object": "model"}],
            })
            .to_string();
            write_response(&mut stream, "200 OK", "application/json", body.as_bytes()).await
        }
        ("GET", "/health") => write_response(&mut stream, "200 OK", "text/plain", b"ok").await,
        ("POST", "/v1/chat/completions") => {
            let id = next_id.fetch_add(1, Ordering::SeqCst);
            handle_chat_completion(stream, cli, id, &req.body).await
        }
        _ => write_response(&mut stream, "404 Not Found", "text/plain", b"not found").await,
    }
}

fn parse_max_tokens(body: &[u8]) -> u32 {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("max_tokens").and_then(|m| m.as_u64()))
        .unwrap_or(16) as u32
}

async fn handle_chat_completion(
    mut stream: BufReader<TcpStream>,
    cli: &Cli,
    id: u64,
    body: &[u8],
) -> io::Result<()> {
    if let Some(n) = cli.fail_every
        && n > 0
        && (id + 1).is_multiple_of(n)
    {
        write_response(
            &mut stream,
            "500 Internal Server Error",
            "text/plain",
            b"error",
        )
        .await?;
        print_event(&format!("kind=failed id={id} status=500"));
        return Ok(());
    }

    let max_tokens = parse_max_tokens(body);

    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    stream.write_all(head.as_bytes()).await?;
    stream.flush().await?;

    let (mut read_half, mut write_half) = stream.into_inner().into_split();

    // Watches for the client going away (EOF or error) during the TTFT/ITL
    // wait, not only on the writer's own next write attempt.
    let (disc_tx, mut disc_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let mut buf = [0u8; 64];
        loop {
            match read_half.read(&mut buf).await {
                Ok(0) => {
                    let _ = disc_tx.send(());
                    break;
                }
                Ok(_) => continue, // the client sends no further body; ignore
                Err(_) => {
                    let _ = disc_tx.send(());
                    break;
                }
            }
        }
    });

    let mut tokens_sent: u32 = 0;
    let mut disconnected = false;
    for i in 0..max_tokens {
        let delay = if i == 0 { cli.ttft_ms } else { cli.itl_ms };
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
            _ = &mut disc_rx => {
                disconnected = true;
            }
        }
        if disconnected {
            break;
        }

        let finish_reason = if i + 1 == max_tokens {
            serde_json::Value::String("length".to_string())
        } else {
            serde_json::Value::Null
        };
        let chunk = serde_json::json!({
            "id": format!("stub-{id}"),
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {"content": "tok"}, "finish_reason": finish_reason}],
        });
        let line = format!("data: {chunk}\n\n");
        if write_half.write_all(line.as_bytes()).await.is_err() || write_half.flush().await.is_err()
        {
            disconnected = true;
            break;
        }
        tokens_sent += 1;
    }

    if disconnected {
        print_event(&format!("kind=disconnect id={id} tokens={tokens_sent}"));
        return Ok(());
    }

    let _ = write_half.write_all(b"data: [DONE]\n\n").await;
    let _ = write_half.flush().await;
    print_event(&format!("kind=done id={id} tokens={tokens_sent}"));
    Ok(())
}
