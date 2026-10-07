//! `GET /v1/models` and the multi-method `/v1` route — simple,
//! deterministic responses mirroring upstream's `ModelCard`/`ModelList` and
//! `v1_root`
//! (`vendor/mini-sglang/python/minisgl/server/api_server.py:86-97, 250-252,
//! 313-316`).

use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use serde_json::{Value, json};

use super::{ApiError, AppState};

/// Field order is exactly id, object, created, owned_by, root, mirroring
/// upstream's `ModelCard` Pydantic model's declaration order.
#[derive(Serialize)]
struct ModelCard {
    id: String,
    object: &'static str,
    created: u64,
    owned_by: &'static str,
    root: String,
}

#[derive(Serialize)]
pub(crate) struct ModelList {
    object: &'static str,
    data: Vec<ModelCard>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// `GET /v1/models`: not-ready parity requires the engine check to happen
/// first, same as every other upstream-parity route. `pub(crate)` (not
/// `pub`): its return type exposes the crate-private `ModelList`.
pub(crate) async fn list_models(
    State(state): State<AppState>,
) -> Result<Json<ModelList>, ApiError> {
    state.engine()?;
    let model = state.model().to_string();
    Ok(Json(ModelList {
        object: "list",
        data: vec![ModelCard {
            id: model.clone(),
            object: "model",
            created: now_unix(),
            owned_by: "mini-sglang",
            root: model,
        }],
    }))
}

/// `/v1` on GET, POST, HEAD and OPTIONS (upstream's
/// `@app.api_route("/v1", methods=["GET", "POST", "HEAD", "OPTIONS"])`).
pub async fn v1_root(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    state.engine()?;
    Ok(Json(json!({ "status": "ok" })))
}
