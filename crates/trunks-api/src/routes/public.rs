//! Routes that need neither initialization nor authentication.

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use trunks_types::api::{Health, InitRequest, InitResponse, ServerMetadata};
use trunks_types::error::messages;

use crate::error::{ApiError, JsonBody};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/metadata", get(metadata))
        .route("/init", post(init))
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok".into() })
}

async fn metadata() -> Json<ServerMetadata> {
    Json(ServerMetadata { version: trunks_types::version::version() })
}

/// Initializes the installation. Development → `{status}`; enterprise → `{status, admin_access_token}`.
async fn init(
    State(st): State<AppState>,
    JsonBody(body): JsonBody<InitRequest>,
) -> Result<Json<InitResponse>, ApiError> {
    let outcome = st.mode.init(body.mode).await?;
    let status = if outcome.mode.is_enterprise() {
        messages::INIT_ENTERPRISE_OK
    } else {
        messages::INIT_DEV_OK
    };
    Ok(Json(InitResponse { status: status.into(), admin_access_token: outcome.admin_access_token }))
}

/// `GET /metrics` — mounted only when metrics are enabled.
pub async fn metrics(State(st): State<AppState>) -> Response {
    match &st.metrics {
        Some(m) => (
            [(header::CONTENT_TYPE, "application/openmetrics-text; version=1.0.0; charset=utf-8")],
            m.render(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
