//! `/api/v0` read endpoints over the catalog, filtered by the caller's access (D14): admins and
//! development mode see everything; a user sees the servers/tools/groups granted to them.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Extension, Json, Router};
use trunks_core::Error;
use trunks_core::identity::UserPrincipal;
use trunks_types::api::{
    AccessView, ByNameQuery, ByServerQuery, GrantSpec, McpServer, Prompt, Resource, TeamMembership,
    Tool, WhoAmI,
};
use trunks_types::error::messages;
use trunks_types::{CanonicalName, ServerName};

use crate::error::ApiError;
use crate::state::AppState;

/// Routes available to any authenticated user (anyone in development mode).
pub fn user_router() -> Router<AppState> {
    Router::new()
        .route("/servers", get(list_servers))
        .route("/tools", get(list_tools))
        .route("/tool", get(get_tool))
        .route("/prompts", get(list_prompts))
        .route("/prompt", get(get_prompt))
        .route("/resources", get(list_resources))
}

/// Enterprise-only user routes.
pub fn enterprise_user_router() -> Router<AppState> {
    Router::new().route("/users/whoami", get(whoami))
}

fn server_filter(q: &ByServerQuery) -> Result<Option<ServerName>, ApiError> {
    Ok(match q.server.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => Some(ServerName::parse(s).map_err(Error::from)?),
        None => None,
    })
}

async fn list_servers(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
) -> Json<Vec<McpServer>> {
    let snap = st.catalog.load();
    Json(
        snap.servers
            .values()
            .filter(|s| p.may_see_server(&snap, &s.name))
            .map(|s| s.to_api())
            .collect(),
    )
}

async fn list_tools(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Query(q): Query<ByServerQuery>,
) -> Result<Json<Vec<Tool>>, ApiError> {
    let filter = server_filter(&q)?;
    let snap = st.catalog.load();
    if let Some(name) = &filter {
        p.visible_server(&snap, name)?;
    }
    Ok(Json(
        snap.tools
            .values()
            .filter(|t| filter.as_ref().is_none_or(|f| &t.server == f))
            .filter(|t| p.may_use_tool(&snap, &t.canonical))
            .map(|t| t.to_api())
            .collect(),
    ))
}

async fn get_tool(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Query(q): Query<ByNameQuery>,
) -> Result<Json<Tool>, ApiError> {
    let name = q
        .name
        .as_deref()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| Error::invalid_input(messages::MISSING_NAME_QUERY))?;
    let canonical = CanonicalName::parse(name).map_err(Error::from)?;
    let snap = st.catalog.load();
    let tool = snap
        .tools
        .get(&canonical)
        .filter(|_| p.may_use_tool(&snap, &canonical))
        .ok_or_else(|| Error::not_found(format!("tool '{canonical}'")))?;
    Ok(Json(tool.to_api()))
}

async fn list_prompts(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Query(q): Query<ByServerQuery>,
) -> Result<Json<Vec<Prompt>>, ApiError> {
    let filter = server_filter(&q)?;
    let snap = st.catalog.load();
    if let Some(name) = &filter {
        p.visible_server(&snap, name)?;
    }
    Ok(Json(
        snap.prompts
            .values()
            .filter(|x| filter.as_ref().is_none_or(|f| &x.server == f))
            .filter(|x| p.may_use_server(&x.server))
            .map(|x| x.to_api())
            .collect(),
    ))
}

async fn get_prompt(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Query(q): Query<ByNameQuery>,
) -> Result<Json<Prompt>, ApiError> {
    let name = q
        .name
        .as_deref()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| Error::invalid_input(messages::MISSING_NAME_QUERY))?;
    let canonical = CanonicalName::parse(name).map_err(Error::from)?;
    let snap = st.catalog.load();
    let prompt = snap
        .prompts
        .get(&canonical)
        .filter(|_| p.may_use_server(canonical.server()))
        .ok_or_else(|| Error::not_found(format!("prompt '{canonical}'")))?;
    Ok(Json(prompt.to_api()))
}

async fn list_resources(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Query(q): Query<ByServerQuery>,
) -> Result<Json<Vec<Resource>>, ApiError> {
    let filter = server_filter(&q)?;
    let snap = st.catalog.load();
    if let Some(name) = &filter {
        p.visible_server(&snap, name)?;
    }
    Ok(Json(
        snap.resources
            .values()
            .filter(|r| filter.as_ref().is_none_or(|f| &r.server == f))
            .filter(|r| p.may_use_server(&r.server))
            .map(|r| r.to_api())
            .collect(),
    ))
}

async fn whoami(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
) -> Result<Json<WhoAmI>, ApiError> {
    match &p {
        UserPrincipal::User { user, access, teams } => Ok(Json(WhoAmI {
            grants: st
                .identity
                .grants_behind(user, teams)
                .await?
                .iter()
                .map(|g| GrantSpec {
                    subject_kind: g.subject_kind.as_str().to_owned(),
                    subject: g.subject_name.clone(),
                    target_kind: g.target_kind.as_str().to_owned(),
                    target: g.target_name.clone(),
                })
                .collect(),
            username: user.username.clone(),
            role: user.role,
            teams: teams
                .iter()
                .map(|(t, r)| TeamMembership { name: t.name.clone(), role: r.as_str().to_owned() })
                .collect(),
            access: AccessView {
                all: access.all,
                servers: access.servers.iter().map(ToString::to_string).collect(),
                tool_groups: access.tool_groups.iter().map(ToString::to_string).collect(),
            },
        })),
        UserPrincipal::DevOpen => {
            Err(ApiError(Error::Forbidden { reason: messages::ENTERPRISE_ONLY.into() }))
        }
    }
}
