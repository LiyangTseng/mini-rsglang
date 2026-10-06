//! `/health`, `/health/ready` and `/metrics` (API-02). These three routes
//! have no upstream equivalent — upstream's `api_server.py` defines none of
//! them — so their shape is this phase's own design, not a parity target.

use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};

use crate::metrics::METRICS_CONTENT_TYPE;

use super::AppState;

/// `GET /metrics`: the Prometheus text exposition for this server's own
/// `ServerMetrics`. Served even before the engine is set — `/metrics` must
/// show every series at 0 from the very first scrape, not 503 — so this
/// handler never calls `state.engine()?` the way every upstream-parity
/// route does. When the engine is set, its `dispatch_stats()` feeds
/// `rsg_late_tokens_dropped_total`; while unset, that series simply stays
/// at whatever `ServerMetrics::new()` initialized it to (0).
pub async fn metrics(State(state): State<AppState>) -> Response {
    let dispatch = state.engine().ok().map(|e| e.dispatch_stats());
    let body = state.metrics().render(dispatch);
    ([(CONTENT_TYPE, METRICS_CONTENT_TYPE)], body).into_response()
}
