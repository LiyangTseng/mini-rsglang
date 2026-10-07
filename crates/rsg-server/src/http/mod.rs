//! Router assembly, shared application state, and the JSON body-parsing
//! helper every handler uses.

pub mod chat;
pub mod error;
pub mod generate;
pub mod health;
pub mod models;
pub mod pyjson;

use std::sync::{Arc, OnceLock};

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};

pub use error::ApiError;

use crate::engine::Engine;
use crate::metrics::ServerMetrics;

/// Oversized-body guard (T-05-01): a body at or above this size is refused
/// with 413 before any JSON parsing happens.
pub const MAX_REQUEST_BODY_BYTES: usize = 16 * 1024 * 1024;

/// The exact content-type upstream's `/generate` stream uses
/// (`api_server.py`'s `StreamingResponse(..., media_type=...)`).
pub const EVENT_STREAM_CONTENT_TYPE: &str = "text/event-stream; charset=utf-8";

struct Inner {
    model: String,
    engine: OnceLock<Arc<Engine>>,
    metrics: ServerMetrics,
}

/// Shared application state passed to every handler.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

impl AppState {
    pub fn new(model: impl Into<String>, metrics: ServerMetrics) -> AppState {
        AppState {
            inner: Arc::new(Inner {
                model: model.into(),
                engine: OnceLock::new(),
                metrics,
            }),
        }
    }

    /// Stores the engine. The engine is set exactly once, at startup; a
    /// second call is ignored and logged at warn.
    pub fn set_engine(&self, engine: Arc<Engine>) {
        if self.inner.engine.set(engine).is_err() {
            tracing::warn!("AppState::set_engine called more than once; ignoring");
        }
    }

    /// Returns the engine, or `Err(ApiError::NotReady)` while unset.
    pub fn engine(&self) -> Result<Arc<Engine>, ApiError> {
        self.inner.engine.get().cloned().ok_or(ApiError::NotReady)
    }

    pub fn model(&self) -> &str {
        &self.inner.model
    }

    /// The per-server metrics this `AppState` was built with.
    pub fn metrics(&self) -> &ServerMetrics {
        &self.inner.metrics
    }
}

/// Parses `body` as JSON, mapping any error to `ApiError::Validation` (422)
/// — FastAPI validates before the handler runs, so a parse failure here
/// must never consume a uid.
pub fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|e| ApiError::Validation(e.to_string()))
}

/// Assembles the axum router: registers every route and applies the
/// request-body size limit.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/generate", post(generate::handler))
        .route("/v1/chat/completions", post(chat::chat_completions))
        .route("/v1/models", get(models::list_models))
        .route(
            "/v1",
            get(models::v1_root)
                .post(models::v1_root)
                .head(models::v1_root)
                .options(models::v1_root),
        )
        .route("/health", get(health::health))
        .route("/health/ready", get(health::ready))
        .route("/metrics", get(health::metrics))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(state)
}

/// Serves `router(state)` on `listener` until the listener itself errors.
pub async fn serve(listener: tokio::net::TcpListener, state: AppState) -> std::io::Result<()> {
    axum::serve(listener, router(state)).await
}
