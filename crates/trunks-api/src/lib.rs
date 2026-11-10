#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! HTTP layer: axum router, middleware and handlers.
//!
//! Route groups and their middleware follow `docs/02-especificacao.md` §5.

pub mod error;
pub mod mcp;
pub mod middleware;
pub mod routes;
mod serve;
pub mod state;

use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware::from_fn;
use axum::middleware::from_fn_with_state;
use axum::routing::get;

pub use serve::serve;
pub use state::AppState;

/// Largest request body accepted, on the REST and on the MCP endpoints (413 beyond it).
pub const BODY_LIMIT: usize = 4 * 1024 * 1024;
/// Added to the request timeout on the REST endpoints, so that an upstream that hits the
/// same limit is reported as such (and its connection released) before the deadline fires.
const REST_DEADLINE_GRACE: Duration = Duration::from_secs(5);

/// Limits of the HTTP layer (`docs/02-especificacao.md` §5).
#[derive(Debug, Clone, Copy)]
pub struct HttpOptions {
    /// Upper bound for one REST request; `None` = unbounded. The MCP endpoints are streams
    /// and are bounded per upstream call instead (`ConnectOptions::call_timeout`).
    pub request_timeout: Option<Duration>,
    /// How long a client may take to send a complete request head (also the idle time
    /// allowed between two requests on a kept-alive HTTP/1 connection). Used by [`serve`].
    pub header_read_timeout: Duration,
    /// Longest pause tolerated while a client sends an MCP request body. REST bodies are
    /// covered by `request_timeout`.
    pub body_idle_timeout: Duration,
    /// How long open connections get to finish once shutdown starts. Used by [`serve`].
    pub shutdown_grace: Duration,
}

impl Default for HttpOptions {
    fn default() -> Self {
        Self {
            request_timeout: Some(Duration::from_mins(5)),
            header_read_timeout: Duration::from_secs(30),
            body_idle_timeout: Duration::from_secs(30),
            shutdown_grace: Duration::from_secs(10),
        }
    }
}

/// The access log: one `INFO` line per response with method, path, status, latency and the
/// kind of principal. The query string is left out — it carries OAuth codes and states.
fn access_log<B>(req: &http::Request<B>) -> tracing::Span {
    tracing::info_span!(
        "request",
        method = %req.method(),
        path = %req.uri().path(),
        principal = tracing::field::Empty,
    )
}

/// Builds the full application router with the default [`HttpOptions`].
pub fn router(state: AppState) -> Router {
    router_with(state, HttpOptions::default())
}

/// Builds the full application router.
pub fn router_with(state: AppState, opts: HttpOptions) -> Router {
    let api_v0 = Router::new()
        .merge(routes::registry::user_router())
        .merge(routes::groups::read_router())
        .merge(routes::invoke::router())
        .merge(
            routes::admin::router()
                .merge(routes::groups::router())
                .merge(routes::access::grants_router())
                .route_layer(from_fn(middleware::require_admin)),
        )
        .merge(
            routes::identities::router()
                .route_layer(from_fn(middleware::require_admin))
                .route_layer(from_fn(middleware::require_enterprise)),
        )
        .merge(
            routes::registry::enterprise_user_router()
                .merge(routes::identities::clients_router())
                .merge(routes::access::teams_router())
                .route_layer(from_fn(middleware::require_enterprise)),
        )
        .layer(from_fn_with_state(state.clone(), middleware::user_auth))
        .layer(from_fn_with_state(state.clone(), middleware::require_initialized));

    // Dashboard reads: open in development, user-authenticated in enterprise (D15).
    let dashboard = routes::dashboard::router()
        .layer(from_fn_with_state(state.clone(), middleware::user_auth))
        .merge(routes::dashboard::callback_router())
        .layer(from_fn_with_state(state.clone(), middleware::require_initialized));

    let mut rest = Router::new()
        .merge(routes::public::router())
        .merge(routes::auth::router())
        .nest("/api/v0", api_v0)
        .nest("/api/dashboard", dashboard)
        .layer(DefaultBodyLimit::max(BODY_LIMIT));
    if let Some(limit) = opts.request_timeout {
        let limit = limit + REST_DEADLINE_GRACE;
        rest = rest.layer(from_fn(move |req, next| middleware::deadline(limit, req, next)));
    }

    let mut app = rest
        .merge(mcp::router(&state).route_layer(tower_http::timeout::RequestBodyTimeoutLayer::new(
            opts.body_idle_timeout,
        )));
    if state.metrics.is_some() {
        app = app.route("/metrics", get(routes::public::metrics));
    }
    app.layer(
        tower_http::trace::TraceLayer::new_for_http()
            .make_span_with(access_log)
            .on_response(tower_http::trace::DefaultOnResponse::new().level(tracing::Level::INFO)),
    )
    .with_state(state)
}
