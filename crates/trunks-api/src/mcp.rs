//! MCP endpoints: `/mcp` (global) and `/v0/groups/{name}/mcp` (per group), served by
//! `rmcp::StreamableHttpService<GatewayHandler>` behind `mcp_client_auth`.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Request, State};
use axum::middleware::from_fn_with_state;
use axum::response::{IntoResponse, Response};
use dashmap::DashMap;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tower::ServiceExt as _;
use trunks_core::Error;
use trunks_core::gateway::{GatewayHandler, GatewayState, Scope};
use trunks_types::GroupName;

use crate::error::ApiError;
use crate::middleware;
use crate::state::AppState;

type McpService = StreamableHttpService<GatewayHandler, LocalSessionManager>;

/// Settings for the downstream MCP transport.
#[derive(Debug, Clone, Default)]
pub struct McpOptions {
    /// `Host` values accepted (DNS-rebinding protection). Empty = loopback only (rmcp default); `["*"]` disables.
    pub allowed_hosts: Vec<String>,
}

#[derive(Clone)]
pub struct McpMounts {
    gateway: Arc<GatewayState>,
    opts: McpOptions,
    global: McpService,
    groups: Arc<DashMap<GroupName, McpService>>,
}

impl std::fmt::Debug for McpMounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpMounts").field("groups", &self.groups.len()).finish_non_exhaustive()
    }
}

impl McpMounts {
    #[must_use]
    pub fn new(gateway: Arc<GatewayState>, opts: McpOptions) -> Self {
        let global = Self::service(&gateway, &opts, Scope::Global);
        Self { gateway, opts, global, groups: Arc::new(DashMap::new()) }
    }

    fn service(gateway: &Arc<GatewayState>, opts: &McpOptions, scope: Scope) -> McpService {
        let mut config = StreamableHttpServerConfig::default();
        config.legacy_session_mode = true;
        // rmcp's own default today; pinned here because the limit is part of our contract.
        config.max_request_body_bytes = crate::BODY_LIMIT;
        if opts.allowed_hosts.iter().any(|h| h == "*") {
            config.allowed_hosts = vec![];
        } else if !opts.allowed_hosts.is_empty() {
            config.allowed_hosts.clone_from(&opts.allowed_hosts);
        }
        let gw = gateway.clone();
        StreamableHttpService::new(
            move || Ok(GatewayHandler::new(scope.clone(), gw.clone())),
            Arc::new(LocalSessionManager::default()),
            config,
        )
    }

    fn group(&self, name: &GroupName) -> McpService {
        self.groups
            .entry(name.clone())
            .or_insert_with(|| Self::service(&self.gateway, &self.opts, Scope::Group(name.clone())))
            .clone()
    }

    /// Forget a group's cached service (after it is deleted or renamed).
    pub fn drop_group(&self, name: &GroupName) {
        self.groups.remove(name);
    }
}

async fn global(State(st): State<AppState>, req: Request) -> Response {
    match st.mcp.global.clone().oneshot(req).await {
        Ok(r) => r.into_response(),
        Err(e) => ApiError(Error::Internal { reason: e.to_string() }).into_response(),
    }
}

async fn group(State(st): State<AppState>, Path(name): Path<String>, req: Request) -> Response {
    let Ok(name) = GroupName::parse(&name) else {
        return ApiError(Error::not_found(format!("tool group: {name}"))).into_response();
    };
    if !st.catalog.load().groups.contains_key(&name) {
        return ApiError(Error::not_found(format!("tool group: {name}"))).into_response();
    }
    match st.mcp.group(&name).oneshot(req).await {
        Ok(r) => r.into_response(),
        Err(e) => ApiError(Error::Internal { reason: e.to_string() }).into_response(),
    }
}

pub fn router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/mcp", axum::routing::any(global))
        .route("/v0/groups/{name}/mcp", axum::routing::any(group))
        // route_layer: a layer() here would also wrap the fallback and turn 404s into 403s
        .route_layer(from_fn_with_state(state.clone(), middleware::mcp_client_auth))
        .route_layer(from_fn_with_state(state.clone(), middleware::require_initialized))
}
