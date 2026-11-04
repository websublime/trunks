//! `/api/v0` admin endpoints: register/deregister servers, enable/disable, config export.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use trunks_core::Error;
use trunks_core::registry::EntityKind;
use trunks_types::ServerName;
use trunks_types::api::{
    EnableDisableResult, EntityQuery, ForceQuery, RegisterServerInput, RegisterServerResult,
};
use trunks_types::error::messages;

use crate::error::{ApiError, JsonBody};
use crate::state::AppState;

/// Routes behind `require_admin`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/servers", post(register_server))
        .route("/servers/{name}", axum::routing::delete(deregister_server))
        .route("/servers/{name}/enable", post(enable_server))
        .route("/servers/{name}/disable", post(disable_server))
        .route("/server_configs", get(server_configs))
        .route("/tools/enable", post(enable_tools))
        .route("/tools/disable", post(disable_tools))
        .route("/prompts/enable", post(enable_prompts))
        .route("/prompts/disable", post(disable_prompts))
        .route("/resources/enable", post(enable_resources))
        .route("/resources/disable", post(disable_resources))
}

/// Strict boolean: `true|false|1|0` (case-insensitive); anything else is a 400.
fn parse_force(q: &ForceQuery) -> Result<bool, ApiError> {
    match q.force.as_deref().map(str::to_ascii_lowercase).as_deref() {
        None | Some("" | "false" | "0" | "f") => Ok(false),
        Some("true" | "1" | "t") => Ok(true),
        Some(_) => Err(ApiError(Error::invalid_input(messages::INVALID_FORCE_QUERY))),
    }
}

async fn register_server(
    State(st): State<AppState>,
    Query(q): Query<ForceQuery>,
    JsonBody(input): JsonBody<RegisterServerInput>,
) -> Result<(StatusCode, Json<RegisterServerResult>), ApiError> {
    let force = parse_force(&q)?;
    let server = st.registry_service.register(input, force).await?;
    Ok((
        StatusCode::CREATED,
        Json(RegisterServerResult { server: Some(server.to_api()), authorization_required: None }),
    ))
}

async fn deregister_server(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    let name = ServerName::parse(name).map_err(Error::from)?;
    st.registry_service.deregister(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_server(
    st: &AppState,
    name: String,
    enabled: bool,
) -> Result<Json<EnableDisableResult>, ApiError> {
    let name = ServerName::parse(name).map_err(Error::from)?;
    Ok(Json(st.registry_service.set_server_enabled(&name, enabled).await?))
}

async fn enable_server(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<EnableDisableResult>, ApiError> {
    set_server(&st, name, true).await
}

async fn disable_server(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<EnableDisableResult>, ApiError> {
    set_server(&st, name, false).await
}

async fn server_configs(State(st): State<AppState>) -> Json<Vec<RegisterServerInput>> {
    let snap = st.catalog.load();
    Json(snap.servers.values().map(|s| s.to_export()).collect())
}

async fn set_entities(
    st: &AppState,
    kind: EntityKind,
    q: &EntityQuery,
    enabled: bool,
) -> Result<Json<EnableDisableResult>, ApiError> {
    let entity = q
        .entity
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .ok_or_else(|| Error::invalid_input(messages::MISSING_ENTITY_QUERY))?;
    let affected = st.registry_service.set_entities_enabled(kind, entity, enabled).await?;
    let mut out = EnableDisableResult::default();
    match kind {
        EntityKind::Tool => out.tools_affected = affected,
        EntityKind::Prompt => out.prompts_affected = affected,
        EntityKind::Resource => out.resources_affected = affected,
    }
    Ok(Json(out))
}

macro_rules! entity_handlers {
    ($enable:ident, $disable:ident, $kind:expr) => {
        async fn $enable(
            State(st): State<AppState>,
            Query(q): Query<EntityQuery>,
        ) -> Result<Json<EnableDisableResult>, ApiError> {
            set_entities(&st, $kind, &q, true).await
        }
        async fn $disable(
            State(st): State<AppState>,
            Query(q): Query<EntityQuery>,
        ) -> Result<Json<EnableDisableResult>, ApiError> {
            set_entities(&st, $kind, &q, false).await
        }
    };
}

entity_handlers!(enable_tools, disable_tools, EntityKind::Tool);
entity_handlers!(enable_prompts, disable_prompts, EntityKind::Prompt);
entity_handlers!(enable_resources, disable_resources, EntityKind::Resource);
