//! Request middleware: initialization gate, authentication and the REST deadline.
//!
//! Each layer inserts a typed value into the request extensions; handlers extract it with
//! `axum::Extension`. Missing values mean the route was mounted without the right layer — a
//! programming error that fails closed (403/401), never a panic.

use std::time::Duration;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use trunks_core::Error;
use trunks_core::identity::{Principal, UserPrincipal};
use trunks_types::ApiErrorResponse;
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

/// Fails a REST request that takes longer than `limit` (reading the body included) with
/// `504 {"error": "request timed out"}`. Not for the MCP endpoints, which are streams.
pub async fn deadline(limit: Duration, req: Request, next: Next) -> Response {
    match tokio::time::timeout(limit, next.run(req)).await {
        Ok(response) => response,
        Err(_) => {
            (StatusCode::GATEWAY_TIMEOUT, Json(ApiErrorResponse::new(messages::REQUEST_TIMED_OUT)))
                .into_response()
        }
    }
}

/// Adds who made the request to the access-log span (never the credential itself).
fn log_principal(kind: &'static str) {
    tracing::Span::current().record("principal", kind);
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

/// Name of the browser session cookie.
pub const SESSION_COOKIE: &str = "trunks_session";

/// The value of the session cookie, if the request carries one.
#[must_use]
pub fn session_cookie(headers: &http::HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == SESSION_COOKIE)
        .map(|(_, v)| v.to_owned())
        .filter(|v| !v.is_empty())
}

/// `scheme://host[:port]` of a URL-ish header value, lowercased, without a trailing slash.
fn origin_of(value: &str) -> Option<String> {
    let (scheme, rest) = value.split_once("://")?;
    let host = rest.split('/').next()?;
    (!host.is_empty())
        .then(|| format!("{}://{}", scheme.to_ascii_lowercase(), host.to_ascii_lowercase()))
}

/// CSRF defence for cookie-authenticated requests that change state: the browser's `Origin`
/// must be this server (its public URL, or the `Host` the request was sent to). Requests
/// without an `Origin` are rejected — every browser sends one on unsafe methods.
fn same_origin(req: &Request, public_url: Option<&str>) -> bool {
    let Some(origin) =
        req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()).and_then(origin_of)
    else {
        return false;
    };
    if public_url.and_then(origin_of).is_some_and(|p| p == origin) {
        return true;
    }
    let host =
        req.headers().get(header::HOST).and_then(|v| v.to_str().ok()).map(str::to_ascii_lowercase);
    host.is_some_and(|h| origin.split_once("://").is_some_and(|(_, o)| o == h))
}

/// REST authentication: inserts a `UserPrincipal` (dev → open). In enterprise mode a bearer
/// token wins; otherwise the session cookie is used, with a same-origin check on unsafe methods.
pub async fn user_auth(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let mode = req.extensions().get::<ServerMode>().copied().unwrap_or(ServerMode::Enterprise);
    let bearer = bearer(&req);
    let result = match (mode.is_enterprise(), &bearer, session_cookie(req.headers())) {
        (true, None, Some(cookie)) => {
            let safe = matches!(
                *req.method(),
                http::Method::GET | http::Method::HEAD | http::Method::OPTIONS
            );
            if !safe && !same_origin(&req, st.public_url.as_deref()) {
                return ApiError(Error::Forbidden {
                    reason: messages::CROSS_ORIGIN_REJECTED.into(),
                })
                .into_response();
            }
            st.login.authenticate_session(&cookie).await
        }
        _ => st.identity.authenticate_user(mode, bearer.as_deref()).await,
    };
    match result {
        Ok(p) => {
            log_principal(match &p {
                UserPrincipal::DevOpen => "dev",
                UserPrincipal::User { .. } if p.is_admin() => "admin",
                UserPrincipal::User { .. } => "user",
            });
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
            log_principal(match &p {
                Principal::DevOpen => "dev",
                Principal::McpClient { .. } => "mcp_client",
            });
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

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::middleware::from_fn;
    use axum::routing::get;
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    use super::*;

    async fn get_status(app: Router, path: &str) -> (StatusCode, String) {
        let req = Request::builder().uri(path).body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        let status = res.status();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn the_deadline_answers_504_and_leaves_fast_requests_alone() {
        let limit = Duration::from_millis(50);
        let app = Router::new()
            .route("/fast", get(|| async { "ok" }))
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    "late"
                }),
            )
            .layer(from_fn(move |req, next| deadline(limit, req, next)));

        assert_eq!(get_status(app.clone(), "/fast").await, (StatusCode::OK, "ok".into()));
        let (status, body) = get_status(app, "/slow").await;
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(body, r#"{"error":"request timed out"}"#);
    }
}
