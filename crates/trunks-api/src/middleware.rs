//! Request middleware: initialization gate and authentication.
//!
//! Each layer inserts a typed value into the request extensions; handlers extract it with
//! `axum::Extension`. Missing values mean the route was mounted without the right layer — a
//! programming error that fails closed (403/401), never a panic.

use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use trunks_core::Error;
use trunks_core::identity::{Principal, UserPrincipal};
use trunks_types::ServerMode;
use trunks_types::error::messages;

use crate::error::ApiError;
use crate::state::AppState;

fn bearer(req: &Request) -> Option<String> {
    req.headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|t| t.trim().to_owned())
}

/// 403 until the installation is initialized; inserts the `ServerMode`.
pub async fn require_initialized(
    State(st): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(mode) = st.mode.current() else {
        return ApiError(Error::NotInitialized).into_response();
    };
    req.extensions_mut().insert(mode);
    next.run(req).await
}

/// REST authentication: inserts a `UserPrincipal` (dev → open).
pub async fn user_auth(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let mode = req.extensions().get::<ServerMode>().copied().unwrap_or(ServerMode::Enterprise);
    match st.identity.authenticate_user(mode, bearer(&req).as_deref()).await {
        Ok(p) => {
            req.extensions_mut().insert(p);
            next.run(req).await
        }
        Err(e) => ApiError(e).into_response(),
    }
}

/// MCP authentication: inserts a `Principal` (dev → open). User tokens are rejected.
pub async fn mcp_client_auth(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let mode = req.extensions().get::<ServerMode>().copied().unwrap_or(ServerMode::Enterprise);
    match st.identity.authenticate_mcp_client(mode, bearer(&req).as_deref()).await {
        Ok(p) => {
            req.extensions_mut().insert::<Principal>(p);
            next.run(req).await
        }
        Err(e) => ApiError(e).into_response(),
    }
}

/// 403 unless the caller is an admin (no-op in development mode).
pub async fn require_admin(req: Request, next: Next) -> Response {
    match req.extensions().get::<UserPrincipal>() {
        Some(p) if p.is_admin() => next.run(req).await,
        Some(_) => ApiError(Error::Forbidden { reason: messages::USER_NOT_AUTHORIZED.into() })
            .into_response(),
        None => ApiError(Error::Unauthorized { reason: messages::MISSING_ACCESS_TOKEN.into() })
            .into_response(),
    }
}

/// 403 unless running in enterprise mode.
pub async fn require_enterprise(req: Request, next: Next) -> Response {
    match req.extensions().get::<ServerMode>() {
        Some(m) if m.is_enterprise() => next.run(req).await,
        _ => {
            ApiError(Error::Forbidden { reason: messages::ENTERPRISE_ONLY.into() }).into_response()
        }
    }
}
