//! Loopback-only HTTP client for one streamed chat completion (D-01/D-02).
//!
//! `local_base_url` is deliberately the only way to build a base URL in this
//! crate (T-02-11, T-07-03): there is no host parameter anywhere else, and
//! the client disables proxies, so `HTTP_PROXY`/`http_proxy` cannot redirect
//! load to a non-local host.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::sse::{SseData, SseLineSplitter};

/// The only base-URL constructor in this crate.
pub fn local_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// A `reqwest` client with every proxy disabled and no total timeout (a long
/// generation must never be cut), but a read timeout as a dead-connection
/// guard.
pub fn build_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .tcp_nodelay(true)
        .read_timeout(Duration::from_secs(300))
        .build()
        .context("build reqwest client")
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
}

/// GETs `{base}/v1/models` and returns `data[0].id`.
pub async fn fetch_model_id(client: &reqwest::Client, base_url: &str) -> anyhow::Result<String> {
    let resp = client
        .get(format!("{base_url}/v1/models"))
        .send()
        .await
        .context("GET /v1/models")?;
    let status = resp.status();
    let bytes = resp.bytes().await.context("read /v1/models body")?;
    if !status.is_success() {
        anyhow::bail!("GET /v1/models returned status {status}");
    }
    let body: ModelsResponse =
        serde_json::from_slice(&bytes).context("decode /v1/models body as JSON")?;
    body.data
        .into_iter()
        .next()
        .map(|m| m.id)
        .context("/v1/models returned no entries in data[]")
}

/// A chat-completion request to drive through [`stream_chat`].
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub prompt: String,
    pub max_tokens: u32,
}

/// When/whether the client drops the response mid-flight (D-02: cancellation
/// is client-side).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelPlan {
    /// Read the stream to completion normally.
    None,
    /// Drop the response immediately after the 200 headers, before reading
    /// any chunk.
    AfterHeaders,
    /// Drop the response as soon as exactly `k` payload chunks have been
    /// read. `AfterChunks(0)` behaves like `AfterHeaders`.
    AfterChunks(u32),
}

/// How a request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    Cancelled,
    Failed,
}

/// Everything recorded about one request.
#[derive(Debug, Clone)]
pub struct RequestRecord {
    pub t_send_unix_ns: u64,
    pub ttft: Option<Duration>,
    pub e2e: Duration,
    pub itl: Vec<Duration>,
    pub chunks: u32,
    pub outcome: Outcome,
    pub error: Option<String>,
}

#[derive(Serialize)]
struct ChatBody<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 1],
    max_tokens: u32,
    temperature: f64,
    stream: bool,
    ignore_eos: bool,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

fn unix_ns_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn failed(t_send_unix_ns: u64, e2e: Duration, error: String) -> RequestRecord {
    RequestRecord {
        t_send_unix_ns,
        ttft: None,
        e2e,
        itl: Vec::new(),
        chunks: 0,
        outcome: Outcome::Failed,
        error: Some(error),
    }
}

fn cancelled_before_read(t_send_unix_ns: u64, e2e: Duration) -> RequestRecord {
    RequestRecord {
        t_send_unix_ns,
        ttft: None,
        e2e,
        itl: Vec::new(),
        chunks: 0,
        outcome: Outcome::Cancelled,
        error: None,
    }
}

/// Streams one chat completion. Never returns `Err`: every failure becomes
/// `Outcome::Failed` inside the returned record (Phase 2 precedent).
pub async fn stream_chat(
    client: &reqwest::Client,
    base_url: &str,
    req: &ChatRequest,
    cancel: CancelPlan,
) -> RequestRecord {
    let t_send_unix_ns = unix_ns_now();
    let t_start = Instant::now();

    let body = ChatBody {
        model: &req.model,
        messages: [ChatMessage {
            role: "user",
            content: &req.prompt,
        }],
        max_tokens: req.max_tokens,
        temperature: 0.0,
        stream: true,
        ignore_eos: true,
    };
    let payload = match serde_json::to_vec(&body) {
        Ok(p) => p,
        Err(e) => return failed(t_send_unix_ns, t_start.elapsed(), e.to_string()),
    };

    let resp = client
        .post(format!("{base_url}/v1/chat/completions"))
        .header("content-type", "application/json")
        .body(payload)
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => return failed(t_send_unix_ns, t_start.elapsed(), e.to_string()),
    };

    let status = resp.status();
    if !status.is_success() {
        return failed(
            t_send_unix_ns,
            t_start.elapsed(),
            format!("status {}", status.as_u16()),
        );
    }

    // AfterHeaders (and AfterChunks(0), which is the same thing): drop right
    // after the headers, before reading any chunk. This models a queued or
    // prefill-stage abort.
    if matches!(
        cancel,
        CancelPlan::AfterHeaders | CancelPlan::AfterChunks(0)
    ) {
        drop(resp);
        return cancelled_before_read(t_send_unix_ns, t_start.elapsed());
    }

    read_stream(resp, t_send_unix_ns, t_start, cancel).await
}

async fn read_stream(
    mut resp: reqwest::Response,
    t_send_unix_ns: u64,
    t_start: Instant,
    cancel: CancelPlan,
) -> RequestRecord {
    let mut splitter = SseLineSplitter::new();
    let mut ttft: Option<Duration> = None;
    let mut itl = Vec::new();
    let mut chunks: u32 = 0;
    let mut last_payload_at: Option<Instant> = None;
    let mut outcome = Outcome::Completed;
    let mut error: Option<String> = None;

    'read: loop {
        let chunk = match resp.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break 'read, // EOF without an explicit [DONE]: still Completed.
            Err(e) => {
                outcome = Outcome::Failed;
                error = Some(e.to_string());
                break 'read;
            }
        };
        for item in splitter.push(&chunk) {
            match item {
                SseData::Done => break 'read,
                SseData::Payload(_) => {
                    let now = Instant::now();
                    if ttft.is_none() {
                        ttft = Some(now - t_start);
                    } else if let Some(prev) = last_payload_at {
                        itl.push(now - prev);
                    }
                    last_payload_at = Some(now);
                    chunks += 1;

                    if let CancelPlan::AfterChunks(k) = cancel
                        && chunks == k
                    {
                        drop(resp);
                        outcome = Outcome::Cancelled;
                        break 'read;
                    }
                }
            }
        }
    }

    RequestRecord {
        t_send_unix_ns,
        ttft,
        e2e: t_start.elapsed(),
        itl,
        chunks,
        outcome,
        error,
    }
}
