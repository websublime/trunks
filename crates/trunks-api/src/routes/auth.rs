//! `/auth/*` — browser login (D16). Public routes; they only do something in enterprise mode.
//!
//! - `GET  /auth/config`   what the web app needs before anyone is logged in
//! - `GET  /auth/login`    302 to the identity provider (OIDC)
//! - `GET  /auth/callback` IdP redirect target: verifies, opens a session, 302 to `return_to`
//! - `POST /auth/token`    exchanges a Trunks access token for a session cookie
//! - `POST /auth/logout`   closes the session

use axum::extract::{Query, State};
use axum::http::header::{LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use trunks_core::Error;
use trunks_types::ServerMode;
use trunks_types::error::messages;

use crate::error::{ApiError, JsonBody};
use crate::middleware::{SESSION_COOKIE, session_cookie};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/config", get(config))
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/token", post(token_login))
        .route("/auth/logout", post(logout))
}

/// `GET /auth/config`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthConfig {
    /// `None` until the server is initialized.
    pub mode: Option<ServerMode>,
    /// Whether any login is needed (enterprise mode).
    pub login_required: bool,
    /// Whether `GET /auth/login` is available.
    pub oidc: bool,
}

async fn config(State(st): State<AppState>) -> Json<AuthConfig> {
    let mode = st.mode.current();
    let enterprise = mode.is_some_and(ServerMode::is_enterprise);
    Json(AuthConfig {
        mode,
        login_required: enterprise,
        oidc: enterprise && st.login.oidc_enabled(),
    })
}

fn enterprise(st: &AppState) -> Result<(), ApiError> {
    match st.mode.current() {
        Some(m) if m.is_enterprise() => Ok(()),
        Some(_) => Err(ApiError(Error::Forbidden { reason: messages::ENTERPRISE_ONLY.into() })),
        None => Err(ApiError(Error::NotInitialized)),
    }
}

/// `Secure` when the public URL (or, failing that, the forwarded scheme) is https.
fn secure(st: &AppState, headers: &HeaderMap) -> bool {
    match st.public_url.as_deref() {
        Some(u) => u.starts_with("https://"),
        None => headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|p| p.eq_ignore_ascii_case("https")),
    }
}

fn cookie(value: &str, max_age: i64, secure: bool) -> HeaderValue {
    let mut c =
        format!("{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}");
    if secure {
        c.push_str("; Secure");
    }
    HeaderValue::from_str(&c).unwrap_or_else(|_| HeaderValue::from_static(""))
}

#[derive(Debug, Default, Deserialize)]
struct LoginQuery {
    #[serde(default)]
    return_to: Option<String>,
}

async fn login(
    State(st): State<AppState>,
    Query(q): Query<LoginQuery>,
) -> Result<Response, ApiError> {
    enterprise(&st)?;
    if !st.login.oidc_enabled() {
        return Err(ApiError(Error::not_found(messages::OIDC_NOT_CONFIGURED)));
    }
    let url = st.login.begin_oidc(q.return_to.as_deref()).await?;
    let location = HeaderValue::from_str(&url).map_err(|_| Error::Internal {
        reason: "the authorization URL is not a valid header".into(),
    })?;
    Ok((StatusCode::FOUND, [(LOCATION, location)]).into_response())
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

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn failure(status: StatusCode, reason: &str) -> Response {
    let body = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Trunks — Sign-in failed</title>\
         <style>body{{font-family:system-ui,sans-serif;background:#0b0b0c;color:#e6e6e6;display:grid;place-items:center;height:100vh;margin:0}}\
         main{{max-width:30rem;padding:2rem;border:1px solid #2a2a2e;border-radius:12px}}h1{{font-size:1.25rem;margin:0 0 .5rem}}\
         p{{margin:.25rem 0;color:#b5b5b8}}a{{color:#c8ff00}}</style></head>\
         <body><main><h1>Sign-in failed</h1><p>{}</p><p><a href=\"/auth/login\">Try again</a></p></main></body></html>",
        escape(reason)
    );
    (status, Html(body)).into_response()
}

async fn callback(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Response {
    if let Err(e) = enterprise(&st) {
        return e.into_response();
    }
    if let Some(err) = q.error {
        let desc = q.error_description.unwrap_or_default();
        return failure(
            StatusCode::BAD_REQUEST,
            &format!("The identity provider returned '{err}'. {desc}"),
        );
    }
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return failure(StatusCode::BAD_REQUEST, "The callback carried no code and state.");
    };
    match st.login.finish_oidc(&code, &state).await {
        Ok((session, return_to)) => {
            let location =
                HeaderValue::from_str(&return_to).unwrap_or_else(|_| HeaderValue::from_static("/"));
            (
                StatusCode::FOUND,
                [
                    (LOCATION, location),
                    (
                        SET_COOKIE,
                        cookie(&session, st.login.session_ttl_seconds(), secure(&st, &headers)),
                    ),
                ],
            )
                .into_response()
        }
        Err(e) => {
            let status = match &e {
                Error::Forbidden { .. } => StatusCode::FORBIDDEN,
                Error::Unauthorized { .. } => StatusCode::UNAUTHORIZED,
                _ => StatusCode::BAD_GATEWAY,
            };
            tracing::warn!(error = %e, "OIDC login failed");
            failure(status, &e.to_string())
        }
    }
}

#[derive(Debug, Deserialize)]
struct TokenLogin {
    access_token: String,
}

#[derive(Debug, Serialize)]
struct LoggedIn {
    username: String,
    role: trunks_types::Role,
}

async fn token_login(
    State(st): State<AppState>,
    headers: HeaderMap,
    JsonBody(body): JsonBody<TokenLogin>,
) -> Result<Response, ApiError> {
    enterprise(&st)?;
    let (session, user) = st.login.login_with_token(body.access_token.trim()).await?;
    Ok((
        StatusCode::OK,
        [(SET_COOKIE, cookie(&session, st.login.session_ttl_seconds(), secure(&st, &headers)))],
        Json(LoggedIn { username: user.username, role: user.role }),
    )
        .into_response())
}

async fn logout(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(value) = session_cookie(&headers) {
        st.login.logout(&value).await?;
    }
    Ok((StatusCode::NO_CONTENT, [(SET_COOKIE, cookie("", 0, secure(&st, &headers)))])
        .into_response())
}
