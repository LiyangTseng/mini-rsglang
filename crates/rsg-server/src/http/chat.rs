//! `POST /v1/chat/completions`: streaming and non-streaming, mirroring
//! upstream's `stream_chat_completions` and `v1_completions`
//! (`vendor/mini-sglang/python/minisgl/server/api_server.py:160-188,
//! 255-310`).
//!
//! Streaming uses real SSE framing (`\n\n`, unlike `/generate`'s single
//! `\n`) with a closing `finish_reason: "stop"` chunk before `data:
//! [DONE]\n\n`. Every streaming chunk's JSON text comes from
//! [`super::pyjson::chat_stream_chunk`], never `serde_json` — Python's
//! `json.dumps(ensure_ascii=True)` escapes non-ASCII characters that
//! `serde_json`'s compact encoder would leave as literal UTF-8 bytes.
//!
//! Task 2 (this same plan) fills in the non-streaming branch and the
//! Python-escaping/defaults unit tests; this task's state only needs the
//! streaming path to compile and pass its tracer test.

use std::collections::VecDeque;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::Response;
use futures::stream;
use serde::Deserialize;

use crate::codec::{ChatMessage, Prompt};
use crate::engine::{AbortGuard, RequestEvent};
use rsg_wire::SamplingParams;

use super::pyjson::chat_stream_chunk;
use super::{ApiError, AppState, EVENT_STREAM_CONTENT_TYPE, parse_json};

fn default_max_tokens() -> i64 {
    16
}
fn default_temperature() -> f64 {
    1.0
}
fn default_top_k() -> i64 {
    -1
}
fn default_top_p() -> f64 {
    1.0
}
fn default_n() -> i64 {
    1
}

/// Mirrors upstream's `OpenAICompletionRequest` (`api_server.py:59-83`): no
/// `deny_unknown_fields` — Pydantic ignores extra keys, and this struct
/// matches that permissiveness. `n`, `stop`, `presence_penalty` and
/// `frequency_penalty` parse (so a request setting them doesn't 422) but
/// are never read, same as upstream's own TODO. `model` is read by Task 2's
/// non-streaming response; `#[allow(dead_code)]` here is this task's own
/// placeholder non-streaming branch not yet reading any of these fields —
/// Task 2 (this same plan) removes it.
#[derive(Deserialize)]
#[allow(dead_code)]
pub(crate) struct ChatCompletionRequest {
    pub model: String,
    pub prompt: Option<String>,
    pub messages: Option<Vec<ChatMessage>>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: i64,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_top_k")]
    pub top_k: i64,
    #[serde(default = "default_top_p")]
    pub top_p: f64,
    #[serde(default = "default_n")]
    pub n: i64,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub stop: Vec<String>,
    #[serde(default)]
    pub presence_penalty: f64,
    #[serde(default)]
    pub frequency_penalty: f64,
    #[serde(default)]
    pub ignore_eos: bool,
}

impl ChatCompletionRequest {
    /// A non-empty `messages` list wins; an absent or empty one falls back
    /// to `prompt`; neither gives [`ApiError::MissingPrompt`] — mirrors
    /// upstream's `if req.messages:` followed by its assert, both of which
    /// run before `new_user`, so this must run before `Engine::start` (the
    /// caller's responsibility, not this method's).
    fn prompt(&self) -> Result<Prompt, ApiError> {
        if let Some(messages) = &self.messages
            && !messages.is_empty()
        {
            return Ok(Prompt::Chat(messages.clone()));
        }
        if let Some(prompt) = &self.prompt {
            return Ok(Prompt::Text(prompt.clone()));
        }
        Err(ApiError::MissingPrompt)
    }

    /// `n`, `stop` and the penalty fields are intentionally not read here —
    /// upstream's own TODO leaves them unused.
    fn sampling_params(&self) -> SamplingParams {
        SamplingParams {
            temperature: self.temperature,
            top_k: self.top_k,
            top_p: self.top_p,
            ignore_eos: self.ignore_eos,
            max_tokens: self.max_tokens,
        }
    }
}

/// The `futures::stream::unfold` state for the streaming response body: the
/// event receiver, the guard that must live exactly as long as the body
/// (so a dropped body — a client disconnect — cancels the request), a
/// queue of already-produced chunks not yet yielded, whether the stream has
/// reached a terminal outcome, the uid (for `chat_stream_chunk`), and
/// whether the next `Token` event is the first (so only it gets
/// `delta.role`).
struct ChatStream {
    events: tokio::sync::mpsc::UnboundedReceiver<RequestEvent>,
    _guard: AbortGuard,
    pending: VecDeque<Result<Bytes, std::io::Error>>,
    done: bool,
    uid: i64,
    first: bool,
}

pub async fn chat_completions(State(state): State<AppState>, body: Bytes) -> Result<Response, ApiError> {
    // Parsed before touching the engine: a 422 here must never consume a uid.
    let req: ChatCompletionRequest = parse_json(&body)?;
    let engine = state.engine()?;
    // MissingPrompt before `start`, like upstream's assert before new_user.
    let prompt = req.prompt()?;
    let params = req.sampling_params();

    let mut active = engine.start(prompt, params);
    active.accepted().await?;
    let uid = active.uid();

    if req.stream {
        let (_uid, events, guard) = active.into_parts();
        let st = ChatStream {
            events,
            _guard: guard,
            pending: VecDeque::new(),
            done: false,
            uid,
            first: true,
        };

        let body_stream = stream::unfold(st, |mut st| async move {
            loop {
                if let Some(item) = st.pending.pop_front() {
                    return Some((item, st));
                }
                if st.done {
                    return None;
                }
                match st.events.recv().await {
                    Some(RequestEvent::Token { text, finished }) => {
                        let content = if text.is_empty() { None } else { Some(text.as_str()) };
                        let chunk = chat_stream_chunk(st.uid, st.first, content, false);
                        st.first = false;
                        st.pending
                            .push_back(Ok(Bytes::from(format!("data: {chunk}\n\n"))));
                        if finished {
                            let stop_chunk = chat_stream_chunk(st.uid, false, None, true);
                            st.pending
                                .push_back(Ok(Bytes::from(format!("data: {stop_chunk}\n\n"))));
                            st.pending
                                .push_back(Ok(Bytes::from_static(b"data: [DONE]\n\n")));
                            st.done = true;
                        }
                    }
                    Some(RequestEvent::Failed(e)) => {
                        st.pending.push_back(Err(std::io::Error::other(e.to_string())));
                        st.done = true;
                    }
                    Some(other) => {
                        tracing::error!(?other, "unexpected engine event after acceptance");
                        st.pending
                            .push_back(Err(std::io::Error::other("unexpected engine event")));
                        st.done = true;
                    }
                    None => {
                        st.pending
                            .push_back(Err(std::io::Error::other("backend connection closed")));
                        st.done = true;
                    }
                }
            }
        });

        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, EVENT_STREAM_CONTENT_TYPE)
            .body(Body::from_stream(body_stream))
            .map_err(|e| ApiError::Internal(e.to_string()))
    } else {
        // Task 2 (this same plan) replaces this with the real non-streaming
        // response. `uid` is already captured above for that task to use.
        let _ = uid;
        Err(ApiError::Internal(
            "non-streaming chat completions not yet implemented".to_string(),
        ))
    }
}
