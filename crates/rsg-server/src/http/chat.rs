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
//! `serde_json`'s compact encoder would leave as literal UTF-8 bytes. The
//! non-streaming response goes through plain `serde_json`/`axum::Json`
//! instead, matching Starlette's own `JSONResponse` (`ensure_ascii=False`).

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures::stream;
use serde::{Deserialize, Serialize};

use crate::codec::{ChatMessage, Prompt};
use crate::engine::{AbortGuard, ActiveRequest, RequestEvent};
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
/// are never read by the handler, same as upstream's own TODO — they are
/// read only by this module's own unit test, which is why production code
/// still needs `#[allow(dead_code)]` on them (the plain `lib` compilation
/// has no `#[cfg(test)]`).
#[derive(Deserialize)]
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
    #[allow(dead_code)]
    #[serde(default = "default_n")]
    pub n: i64,
    #[serde(default)]
    pub stream: bool,
    #[allow(dead_code)]
    #[serde(default)]
    pub stop: Vec<String>,
    #[allow(dead_code)]
    #[serde(default)]
    pub presence_penalty: f64,
    #[allow(dead_code)]
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

#[derive(Serialize)]
struct ChatResponseMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct ChatChoice {
    index: u32,
    message: ChatResponseMessage,
    finish_reason: &'static str,
}

#[derive(Serialize)]
struct ChatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// Field order is exactly id, object, created, model, choices, usage
/// (`api_server.py:313-330`'s dict literal order), which `serde_json`
/// preserves for a struct (field-declaration order, not sorted).
#[derive(Serialize)]
struct ChatCompletionResponse {
    id: String,
    object: &'static str,
    created: u64,
    model: String,
    choices: Vec<ChatChoice>,
    usage: ChatUsage,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Reads events from `active` until the finished `Token`, concatenating
/// each increment into the full response content. `active` (and so its
/// `AbortGuard`) stays alive in the caller's own future for the whole wait
/// — a dropped handler future (client disconnect) cancels the request the
/// same way the streaming branch's body-stream guard does.
async fn collect_full_content(active: &mut ActiveRequest) -> Result<String, ApiError> {
    let mut content = String::new();
    loop {
        match active.next_event().await {
            Some(RequestEvent::Token { text, finished }) => {
                content.push_str(&text);
                if finished {
                    return Ok(content);
                }
            }
            Some(RequestEvent::Failed(e)) => return Err(ApiError::from(e)),
            Some(other) => {
                tracing::error!(?other, "unexpected engine event after acceptance");
                return Err(ApiError::Internal("unexpected engine event".to_string()));
            }
            None => {
                return Err(ApiError::Internal(
                    "request ended without a result".to_string(),
                ));
            }
        }
    }
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
        let content = collect_full_content(&mut active).await?;
        let body = ChatCompletionResponse {
            id: format!("chatcmpl-{uid}"),
            object: "chat.completion",
            created: now_unix(),
            model: req.model,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatResponseMessage {
                    role: "assistant",
                    content,
                },
                finish_reason: "stop",
            }],
            usage: ChatUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        };
        Ok(axum::Json(body).into_response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> ChatCompletionRequest {
        serde_json::from_str(json).expect("valid request")
    }

    #[test]
    fn defaults_mirror_upstream() {
        let req = parse(r#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#);
        assert_eq!(
            req.sampling_params(),
            SamplingParams {
                temperature: 1.0,
                top_k: -1,
                top_p: 1.0,
                ignore_eos: false,
                max_tokens: 16,
            }
        );
        assert!(!req.stream);

        let req = parse(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"temperature":0.5,"top_k":4,"top_p":0.9,"ignore_eos":true,"max_tokens":7}"#,
        );
        assert_eq!(
            req.sampling_params(),
            SamplingParams {
                temperature: 0.5,
                top_k: 4,
                top_p: 0.9,
                ignore_eos: true,
                max_tokens: 7,
            }
        );

        let req = parse(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"n":3,"stop":["a","b"],"presence_penalty":0.2,"frequency_penalty":0.3}"#,
        );
        assert_eq!(req.n, 3);
        assert_eq!(req.stop, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(req.presence_penalty, 0.2);
        assert_eq!(req.frequency_penalty, 0.3);
    }
}
