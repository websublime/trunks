//! Conformance matrix: the same black-box scenarios against the real binary for every
//! combination of operating mode and database backend (`docs/02-especificacao.md` §12).
//!
//! The PostgreSQL cells need `TRUNKS_TEST_PG_URL` (a server where databases may be created)
//! and are skipped without it — unless `TRUNKS_TEST_REQUIRE_PG` is set, as in CI.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::too_many_lines)]

mod development;
mod enterprise;
mod gateway;

use gateway::Backend;

#[tokio::test]
async fn development_on_sqlite() {
    development::scenario(Backend::Sqlite).await;
}

#[tokio::test]
async fn development_on_postgres() {
    development::scenario(Backend::Postgres).await;
}

#[tokio::test]
async fn enterprise_on_sqlite() {
    enterprise::scenario(Backend::Sqlite).await;
}

#[tokio::test]
async fn enterprise_on_postgres() {
    enterprise::scenario(Backend::Postgres).await;
}
