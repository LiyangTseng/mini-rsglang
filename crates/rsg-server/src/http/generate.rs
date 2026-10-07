//! `POST /generate`: upstream's single-newline `data:` framing
//! (`vendor/mini-sglang/python/minisgl/server/api_server.py:152-158`).
//! Never use `axum::response::sse::Sse` here — it always emits a blank
//! line (`\n\n`) between events, while upstream's `/generate` emits a
//! single `\n`.

use std::collections::VecDeque;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::Response;
use futures::stream;
use serde::Deserialize;

use crate::codec::Prompt;
use crate::engine::{AbortGuard, RequestEvent};
use rsg_wire::SamplingParams;

use super::{ApiError, AppState, EVENT_STREAM_CONTENT_TYPE, parse_json};

/// Mirrors upstream's `GenerateRequest` (`api_server.py:53-57`): no
/// `deny_unknown_fields` — Pydantic ignores extra keys by default, and this
/// struct matches that permissiveness.
#[derive(Deserialize)]
struct GenerateRequest {
    prompt: String,
    max_tokens: i64,
    #[serde(default)]
    ignore_eos: bool,
}

/// The `futures::stream::unfold` state for the response body: the event
/// receiver, the guard that must live exactly as long as the body (so a
/// dropped body — a client disconnect — cancels the request), a queue of
/// already-produced chunks not yet yielded (a finished token produces two:
/// its own data line, then `data: [DONE]\n`), and whether the stream has
/// reached a terminal outcome.
struct GenStream {
    events: tokio::sync::mpsc::UnboundedReceiver<RequestEvent>,
    _guard: AbortGuard,
    pending: VecDeque<Result<Bytes, std::io::Error>>,
    done: bool,
}

pub async fn handler(State(state): State<AppState>, body: Bytes) -> Result<Response, ApiError> {
    // Parsed before touching the engine: a 422 here must never consume a uid.
    let req: GenerateRequest = parse_json(&body)?;
    let engine = state.engine()?;

    let params = SamplingParams {
        ignore_eos: req.ignore_eos,
        max_tokens: req.max_tokens,
        ..SamplingParams::default()
    };
    let mut active = engine.start(Prompt::Text(req.prompt), params);
    active.accepted().await?;

    let (_uid, events, guard) = active.into_parts();
    let state = GenStream {
        events,
        _guard: guard,
        pending: VecDeque::new(),
        done: false,
    };

    let body_stream = stream::unfold(state, |mut st| async move {
        loop {
            if let Some(item) = st.pending.pop_front() {
                return Some((item, st));
            }
            if st.done {
                return None;
            }
            match st.events.recv().await {
                Some(RequestEvent::Token { text, finished }) => {
                    st.pending
                        .push_back(Ok(Bytes::from(format!("data: {text}\n"))));
                    if finished {
                        st.pending
                            .push_back(Ok(Bytes::from_static(b"data: [DONE]\n")));
                        st.done = true;
                    }
                }
                Some(RequestEvent::Failed(e)) => {
                    st.pending
                        .push_back(Err(std::io::Error::other(e.to_string())));
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
}
