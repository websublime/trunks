#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! HTTP layer: axum router, middleware and handlers.
//!
//! Phase 0 ships the unauthenticated routes only (`/health`, `/metadata`).
//! Every later route is added with its middleware group as described in
//! `docs/02-especificacao.md` §5.

pub mod error;
pub mod routes;

use axum::Router;

/// Builds the full application router.
pub fn router() -> Router {
    Router::new()
        .merge(routes::public::router())
        .layer(tower_http::trace::TraceLayer::new_for_http())
}
