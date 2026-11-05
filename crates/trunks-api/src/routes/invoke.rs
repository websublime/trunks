//! `/api/v0` invocation endpoints: tools/invoke, prompts/render, resources/get, resources/read.

use axum::extract::State;
use axum::routing::post;
use axum::{Extension, Json, Router};
use trunks_core::Error;
use trunks_core::gateway::Invoker;
use trunks_core::identity::UserPrincipal;
use trunks_types::api::{
    PromptRenderRequest, PromptRenderResult, Resource, ResourceReadResult, ResourceRequest,
    ToolInvokeRequest, ToolInvokeResult,
};

use crate::error::{ApiError, JsonBody};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tools/invoke", post(invoke_tool))
        .route("/prompts/render", post(render_prompt))
        .route("/resources/get", post(get_resource))
        .route("/resources/read", post(read_resource))
}

fn to_value<T: serde::Serialize>(v: &T) -> Result<serde_json::Value, ApiError> {
    serde_json::to_value(v).map_err(|e| ApiError(Error::Internal { reason: e.to_string() }))
}

/// Access is checked **before** any upstream call; a denied tool reads as "not found" so the
/// API does not reveal what exists.
async fn invoke_tool(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    JsonBody(body): JsonBody<ToolInvokeRequest>,
) -> Result<Json<ToolInvokeResult>, ApiError> {
    if !p.may_use_tool(&st.catalog.load(), &body.name) {
        return Err(ApiError(Error::not_found(format!("tool '{}'", body.name))));
    }
    let args = if body.arguments.is_empty() { None } else { Some(body.arguments) };
    let r = Invoker(st.gateway.clone()).call_tool(&body.name, args).await?;
    let content = r.content.iter().map(to_value).collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ToolInvokeResult {
        meta: r.meta.as_ref().map(to_value).transpose()?,
        is_error: r.is_error,
        content,
        structured_content: r.structured_content,
    }))
}

async fn render_prompt(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    JsonBody(body): JsonBody<PromptRenderRequest>,
) -> Result<Json<PromptRenderResult>, ApiError> {
    if !p.may_use_server(body.name.server()) {
        return Err(ApiError(Error::not_found(format!("prompt '{}'", body.name))));
    }
    let args = if body.arguments.is_empty() { None } else { Some(body.arguments) };
    let r = Invoker(st.gateway.clone()).get_prompt(&body.name, args).await?;
    let messages = r.messages.iter().map(to_value).collect::<Result<Vec<_>, _>>()?;
    Ok(Json(PromptRenderResult { description: r.description, messages }))
}

async fn get_resource(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    JsonBody(body): JsonBody<ResourceRequest>,
) -> Result<Json<Resource>, ApiError> {
    let snap = st.catalog.load();
    let r = snap
        .resources
        .get(&body.uri)
        .filter(|_| p.may_use_server(body.uri.server()))
        .ok_or_else(|| Error::not_found(format!("resource '{}'", body.uri)))?;
    Ok(Json(r.to_api()))
}

async fn read_resource(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    JsonBody(body): JsonBody<ResourceRequest>,
) -> Result<Json<ResourceReadResult>, ApiError> {
    if !p.may_use_server(body.uri.server()) {
        return Err(ApiError(Error::not_found(format!("resource '{}'", body.uri))));
    }
    let r = Invoker(st.gateway.clone()).read_resource(&body.uri).await?;
    let contents = r.contents.iter().map(to_value).collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ResourceReadResult { contents }))
}
