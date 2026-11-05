//! `/api/v0/tool-groups*` — admin. Groups are read from the catalog and written through
//! `GroupService`; a deleted group's cached MCP service is dropped.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use trunks_core::Error;
use trunks_core::groups::{endpoint, to_api};
use trunks_types::GroupName;
use trunks_types::api::{Tool, ToolGroup, ToolGroupWithEndpoints, UpdateToolGroupResponse};

use crate::error::{ApiError, JsonBody};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tool-groups", get(list_groups).post(create_group))
        .route("/tool-groups/{name}", get(get_group).put(update_group).delete(delete_group))
        .route("/tool-groups/{name}/effective-tools", get(effective_tools))
}

fn parse(name: &str) -> Result<GroupName, ApiError> {
    GroupName::parse(name).map_err(|e| ApiError(Error::from(e)))
}

async fn list_groups(State(st): State<AppState>) -> Json<Vec<ToolGroup>> {
    let snap = st.catalog.load();
    Json(snap.groups.values().map(|g| to_api(g)).collect())
}

async fn create_group(
    State(st): State<AppState>,
    JsonBody(body): JsonBody<ToolGroup>,
) -> Result<(StatusCode, Json<ToolGroupWithEndpoints>), ApiError> {
    let view = st.groups.create(&body).await?;
    Ok((
        StatusCode::CREATED,
        Json(ToolGroupWithEndpoints {
            group: to_api(&view),
            streamable_http_endpoint: endpoint(&view.name),
        }),
    ))
}

async fn get_group(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<ToolGroupWithEndpoints>, ApiError> {
    let name = parse(&name)?;
    let snap = st.catalog.load();
    let view =
        snap.groups.get(&name).ok_or_else(|| Error::not_found(format!("tool group '{name}'")))?;
    Ok(Json(ToolGroupWithEndpoints {
        group: to_api(view),
        streamable_http_endpoint: endpoint(&view.name),
    }))
}

async fn update_group(
    State(st): State<AppState>,
    Path(name): Path<String>,
    JsonBody(body): JsonBody<ToolGroup>,
) -> Result<Json<UpdateToolGroupResponse>, ApiError> {
    let name = parse(&name)?;
    let (old, new) = st.groups.update(&name, &body).await?;
    Ok(Json(UpdateToolGroupResponse { name: name.to_string(), old, new: to_api(&new) }))
}

async fn delete_group(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    let name = parse(&name)?;
    st.groups.delete(&name).await?;
    st.mcp.drop_group(&name);
    Ok(StatusCode::NO_CONTENT)
}

async fn effective_tools(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Vec<Tool>>, ApiError> {
    let name = parse(&name)?;
    let snap = st.catalog.load();
    let view =
        snap.groups.get(&name).ok_or_else(|| Error::not_found(format!("tool group '{name}'")))?;
    Ok(Json(
        view.effective_tools.iter().filter_map(|c| snap.tools.get(c)).map(|t| t.to_api()).collect(),
    ))
}
