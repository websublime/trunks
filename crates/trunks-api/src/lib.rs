#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! HTTP layer: axum router, middleware and handlers.
//!
//! Route groups and their middleware follow `docs/02-especificacao.md` §5.

pub mod error;
pub mod mcp;
pub mod middleware;
pub mod routes;
pub mod state;

use axum::Router;
use axum::middleware::from_fn;
use axum::middleware::from_fn_with_state;
use axum::routing::get;

pub use state::AppState;

/// Builds the full application router.
pub fn router(state: AppState) -> Router {
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

    let mut app = Router::new()
        .merge(routes::public::router())
        .merge(routes::auth::router())
        .merge(mcp::router(&state))
        .nest("/api/v0", api_v0)
        .nest("/api/dashboard", dashboard);
    if state.metrics.is_some() {
        app = app.route("/metrics", get(routes::public::metrics));
    }
    app.layer(tower_http::trace::TraceLayer::new_for_http()).with_state(state)
}
