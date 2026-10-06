//! The HTTP-layer error vocabulary (`ApiError`), its status-code mapping
//! and JSON body shapes.
//!
//! `Validation` is the only variant with the `{"detail": "<msg>"}` body
//! shape (FastAPI's validation-error status, 422; the body shape itself is
//! not claimed as byte-parity-scoped). Every other variant uses
//! `{"error": {"message": "<Display text>", "type": "<kind>"}}`.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::engine::{RequestError, SubmitError};

/// Every error an HTTP handler can return. Each variant maps to exactly one
/// HTTP status code ([`ApiError::status`]) and one JSON body shape.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    #[error("engine not ready")]
    NotReady,
    #[error("{0}")]
    Validation(String),
    #[error("prompt is {input_len} tokens, exceeding max_seq_len {max_seq_len}")]
    PromptTooLong { input_len: usize, max_seq_len: u64 },
    #[error("codec error: {0}")]
    Codec(String),
    #[error("backend unavailable")]
    BackendUnavailable,
    #[error("backend did not respond within {timeout_ms}ms")]
    BackendTimeout { timeout_ms: u64 },
    #[error("Either 'messages' or 'prompt' must be provided")]
    MissingPrompt,
    #[error("{0}")]
    Internal(String),
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            ApiError::NotReady => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::PromptTooLong { .. } => StatusCode::BAD_REQUEST,
            ApiError::Codec(_) => StatusCode::BAD_REQUEST,
            ApiError::BackendUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::BackendTimeout { .. } => StatusCode::GATEWAY_TIMEOUT,
            ApiError::MissingPrompt => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The `"type"` field of the `{"error": {...}}` body. Never read for
    /// `Validation`, which uses the `{"detail": ...}` shape instead.
    fn kind(&self) -> &'static str {
        match self {
            ApiError::NotReady => "not_ready",
            ApiError::Validation(_) => "validation_error",
            ApiError::PromptTooLong { .. } | ApiError::Codec(_) => "invalid_request_error",
            ApiError::BackendUnavailable => "backend_unavailable",
            ApiError::BackendTimeout { .. } => "backend_timeout",
            ApiError::MissingPrompt | ApiError::Internal(_) => "internal_error",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = match &self {
            ApiError::Validation(msg) => json!({ "detail": msg }),
            other => json!({
                "error": {
                    "message": other.to_string(),
                    "type": other.kind(),
                }
            }),
        };
        (status, Json(body)).into_response()
    }
}

impl From<SubmitError> for ApiError {
    fn from(e: SubmitError) -> Self {
        match e {
            SubmitError::PromptTooLong {
                input_len,
                max_seq_len,
            } => ApiError::PromptTooLong {
                input_len,
                max_seq_len,
            },
            SubmitError::Codec(msg) => ApiError::Codec(msg),
            SubmitError::BackendUnavailable => ApiError::BackendUnavailable,
        }
    }
}

impl From<RequestError> for ApiError {
    fn from(e: RequestError) -> Self {
        match e {
            RequestError::BackendTimeout { timeout_ms } => {
                ApiError::BackendTimeout { timeout_ms }
            }
            other => ApiError::Internal(other.to_string()),
        }
    }
}
