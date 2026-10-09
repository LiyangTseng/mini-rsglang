//! `/health`, `/health/ready` and `/metrics` (API-02). These three routes
//! have no upstream equivalent — upstream's `api_server.py` defines none of
//! them — so their shape is this phase's own design, not a parity target.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::metrics::METRICS_CONTENT_TYPE;

use super::AppState;

/// `GET /health`: process liveness only. Always 200 once the HTTP listener
/// itself is up — this route never touches the engine, unlike every
/// upstream-parity route and `/health/ready` below.
pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// `GET /health/ready`: 200 once the readiness handshake has been received
/// and `AppState::set_engine` called (`state.engine()` is `Ok`), 503
/// `{"status":"starting"}` until then. This is the front-half/end-to-end
/// split Phase 7's cold-start scenario measures.
pub async fn ready(State(state): State<AppState>) -> Response {
    match state.engine() {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ready" }))).into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "starting" })),
        )
            .into_response(),
    }
}

/// `GET /metrics`: the Prometheus text exposition for this server's own
/// `ServerMetrics`. Served even before the engine is set — `/metrics` must
/// show every series at 0 from the very first scrape, not 503 — so this
/// handler never calls `state.engine()?` the way every upstream-parity
/// route does. When the engine is set, its `dispatch_stats()` feeds
/// `rsg_late_tokens_dropped_total`; while unset, that series simply stays
/// at whatever `ServerMetrics::new()` initialized it to (0).
pub async fn metrics(State(state): State<AppState>) -> Response {
    let engine = state.engine().ok();
    let dispatch = engine.as_ref().map(|e| e.dispatch_stats());
    let writer_queue_depth = engine.as_ref().map(|e| e.writer_queue_depth());
    let body = state.metrics().render(dispatch, writer_queue_depth);
    ([(CONTENT_TYPE, METRICS_CONTENT_TYPE)], body).into_response()
}
