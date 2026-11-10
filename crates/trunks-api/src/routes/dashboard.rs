//! `/api/dashboard/*` — read-only aggregated, sanitized views for the web app (D15), plus the
//! OAuth browser callback. In enterprise mode the reads sit behind `user_auth` and hold only
//! what the caller may use (D14), exactly like the `/api/v0` reads; the views about the
//! installation itself (diagnostics, authorization sessions) are for administrators. The
//! callback is public (the IdP redirects the browser there; `state` is unguessable and
//! single-use).

use axum::extract::{Path, Query, State};
use axum::middleware::from_fn;
use axum::response::Html;
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde::Deserialize;
use trunks_core::Error;
use trunks_core::dashboard::{
    self, Diagnostics, GroupSummary, OAuthSessionStatus, Overview, ServerDetail, ServerSummary,
};
use trunks_core::identity::UserPrincipal;
use trunks_types::api::{ByServerQuery, Prompt, Resource, Tool};
use trunks_types::{GroupName, ServerName};

use crate::error::ApiError;
use crate::middleware;
use crate::state::AppState;

/// Reads (behind `user_auth` in enterprise), each limited to what the caller may use.
pub fn router() -> Router<AppState> {
    let admin = Router::new()
        .route("/diagnostics", get(diagnostics))
        .route("/oauth/session/{id}", get(oauth_session))
        .route_layer(from_fn(middleware::require_admin));
    Router::new()
        .route("/overview", get(overview))
        .route("/servers", get(servers))
        .route("/servers/{name}", get(server))
        .route("/tools", get(tools))
        .route("/prompts", get(prompts))
        .route("/resources", get(resources))
        .route("/tool-groups", get(groups))
        .route("/tool-groups/{name}", get(group))
        .merge(admin)
}

/// The browser callback (public).
pub fn callback_router() -> Router<AppState> {
    Router::new().route("/oauth/callback", get(oauth_callback))
}

async fn overview(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
) -> Result<Json<Overview>, ApiError> {
    let snap = st.catalog.load();
    let extras = st.registry_service.dashboard_extras().await?;
    Ok(Json(dashboard::overview(
        &snap,
        &st.runtime,
        st.gateway.sessions.stateful_active(),
        &extras,
        &p,
    )))
}

async fn servers(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
) -> Result<Json<Vec<ServerSummary>>, ApiError> {
    let snap = st.catalog.load();
    let extras = st.registry_service.dashboard_extras().await?;
    Ok(Json(dashboard::servers(&snap, &extras, &p)))
}

async fn server(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Path(name): Path<String>,
) -> Result<Json<ServerDetail>, ApiError> {
    let name = ServerName::parse(name).map_err(Error::from)?;
    let snap = st.catalog.load();
    let record = p.visible_server(&snap, &name)?;
    let extras = st.registry_service.dashboard_extras().await?;
    Ok(Json(dashboard::server_detail(&snap, record, &extras, &p)))
}

#[derive(Debug, Default, Deserialize)]
struct ToolsQuery {
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

async fn tools(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Query(q): Query<ToolsQuery>,
) -> Result<Json<Vec<Tool>>, ApiError> {
    let filter = match q.server.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => Some(ServerName::parse(s).map_err(Error::from)?),
        None => None,
    };
    let snap = st.catalog.load();
    Ok(Json(
        snap.tools
            .values()
            .filter(|t| filter.as_ref().is_none_or(|f| &t.server == f))
            .filter(|t| q.enabled.is_none_or(|e| t.enabled == e))
            .filter(|t| p.may_use_tool(&snap, &t.canonical))
            .map(|t| t.to_api())
            .collect(),
    ))
}

fn server_filter(q: &ByServerQuery) -> Result<Option<ServerName>, ApiError> {
    Ok(match q.server.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => Some(ServerName::parse(s).map_err(Error::from)?),
        None => None,
    })
}

async fn prompts(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Query(q): Query<ByServerQuery>,
) -> Result<Json<Vec<Prompt>>, ApiError> {
    let filter = server_filter(&q)?;
    let snap = st.catalog.load();
    Ok(Json(
        snap.prompts
            .values()
            .filter(|x| filter.as_ref().is_none_or(|f| &x.server == f))
            .filter(|x| p.may_use_server(&x.server))
            .map(|x| x.to_api())
            .collect(),
    ))
}

async fn resources(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Query(q): Query<ByServerQuery>,
) -> Result<Json<Vec<Resource>>, ApiError> {
    let filter = server_filter(&q)?;
    let snap = st.catalog.load();
    Ok(Json(
        snap.resources
            .values()
            .filter(|r| filter.as_ref().is_none_or(|f| &r.server == f))
            .filter(|r| p.may_use_server(&r.server))
            .map(|r| r.to_api())
            .collect(),
    ))
}

async fn groups(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
) -> Json<Vec<GroupSummary>> {
    let snap = st.catalog.load();
    Json(
        snap.groups
            .values()
            .filter(|g| p.may_use_group(&g.name))
            .map(dashboard::group_summary)
            .collect(),
    )
}

async fn group(
    State(st): State<AppState>,
    Extension(p): Extension<UserPrincipal>,
    Path(name): Path<String>,
) -> Result<Json<GroupSummary>, ApiError> {
    let name = GroupName::parse(name).map_err(Error::from)?;
    let snap = st.catalog.load();
    let g = snap
        .groups
        .get(&name)
        .filter(|g| p.may_use_group(&g.name))
        .ok_or_else(|| Error::not_found(format!("tool group '{name}'")))?;
    Ok(Json(dashboard::group_summary(g)))
}

async fn diagnostics(State(st): State<AppState>) -> Result<Json<Diagnostics>, ApiError> {
    let extras = st.registry_service.dashboard_extras().await?;
    Ok(Json(dashboard::diagnostics(&st.runtime, &extras)))
}

async fn oauth_session(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<OAuthSessionStatus>, ApiError> {
    Ok(Json(st.registry_service.oauth_session_status(&id).await?))
}

#[derive(Debug, Default, Deserialize)]
struct CallbackQuery {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Trunks — {title}</title>\
         <style>body{{font-family:system-ui,sans-serif;background:#0b0b0c;color:#e6e6e6;display:grid;place-items:center;height:100vh;margin:0}}\
         main{{max-width:28rem;padding:2rem;border:1px solid #2a2a2e;border-radius:12px}}h1{{font-size:1.25rem;margin:0 0 .5rem}}\
         p{{margin:.25rem 0;color:#b5b5b8}}code{{color:#c8ff00}}</style></head>\
         <body><main><h1>{title}</h1>{body}</main>\
         <script>try{{window.opener&&window.opener.postMessage({{source:'trunks-oauth',title:{title:?}}},'*')}}catch(e){{}}</script></body></html>"
    ))
}

/// The browser lands here after authorizing at the upstream's authorization server. The
/// backend completes the exchange and registers the server; the page only reports.
async fn oauth_callback(
    State(st): State<AppState>,
    Query(q): Query<CallbackQuery>,
) -> Html<String> {
    if let Some(err) = q.error {
        let desc = q.error_description.unwrap_or_default();
        return page(
            "Authorization failed",
            &format!(
                "<p>The authorization server returned <code>{}</code>.</p><p>{}</p>",
                escape(&err),
                escape(&desc)
            ),
        );
    }
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return page(
            "Authorization failed",
            "<p>The callback carried no <code>code</code> and <code>state</code>.</p>",
        );
    };
    match st.registry_service.complete_oauth_by_state(&code, &state).await {
        Ok(server) => page(
            "Authorization received",
            &format!(
                "<p>Server <code>{}</code> is registered.</p><p>You can close this window.</p>",
                escape(server.name.as_str())
            ),
        ),
        Err(e) => page("Authorization failed", &format!("<p>{}</p>", escape(&e.to_string()))),
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
