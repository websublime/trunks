#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! Trunks data access layer.
//!
//! `sqlx` over SQLite and PostgreSQL with explicit SQL and no ORM (decision D11,
//! `docs/06-analise-db.md`). Rules:
//! - one SQL string per operation, written once with `$N` placeholders (SQLite accepts them);
//! - row structs (`rows::*`) derive `FromRow` and never leave this crate as-is;
//! - per-dialect migrations in `migrations/{sqlite,postgres}` with the same numbering;
//! - every repository test runs against both backends.

pub mod error;
pub mod migrate;
pub mod repos;
pub mod rows;

pub use error::{Error, Result};

use std::str::FromStr;
use std::time::Duration;

use snafu::ResultExt;
use sqlx::postgres::PgPoolOptions;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{PgPool, SqlitePool};

/// An open database: one of the two supported backends.
#[derive(Debug, Clone)]
pub enum Db {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

/// Run `$body` with `p` bound to the active pool, whichever backend is open.
/// Both arms must evaluate to the same type.
#[macro_export]
macro_rules! with_pool {
    ($db:expr, |$p:ident| $body:expr) => {
        match $db {
            $crate::Db::Sqlite($p) => $body,
            $crate::Db::Postgres($p) => $body,
        }
    };
}

impl Db {
    /// Opens the database named by `url`:
    /// - `sqlite://<path>` (or `sqlite::memory:`) — the file is created if missing, WAL + 5 s busy timeout;
    /// - `postgres://user:pass@host:port/db` — pool of `max_connections`.
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self> {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            let pool = PgPoolOptions::new()
                .max_connections(max_connections)
                .acquire_timeout(Duration::from_secs(10))
                .connect(url)
                .await
                .context(error::ConnectSnafu { backend: "postgres" })?;
            Ok(Self::Postgres(pool))
        } else if url.starts_with("sqlite:") {
            let opts = SqliteConnectOptions::from_str(url)
                .context(error::ConnectSnafu { backend: "sqlite" })?
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .busy_timeout(Duration::from_secs(5))
                .foreign_keys(true);
            let pool = SqlitePoolOptions::new()
                .max_connections(max_connections)
                .connect_with(opts)
                .await
                .context(error::ConnectSnafu { backend: "sqlite" })?;
            Ok(Self::Sqlite(pool))
        } else {
            error::UnsupportedUrlSnafu { url: redact(url) }.fail()
        }
    }

    /// Human-readable backend name for logs and diagnostics.
    #[must_use]
    pub fn backend(&self) -> &'static str {
        match self {
            Self::Sqlite(_) => "sqlite",
            Self::Postgres(_) => "postgres",
        }
    }

    /// Applies pending migrations for the active dialect (see `migrate`).
    pub async fn migrate(&self) -> Result<migrate::Applied> {
        migrate::run(self).await
    }

    /// Closes the pool gracefully.
    pub async fn close(&self) {
        with_pool!(self, |p| p.close().await);
    }
}

/// Strips credentials from a database URL before it reaches an error message or a log line.
#[must_use]
pub fn redact(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => match rest.rsplit_once('@') {
            Some((_, host)) => format!("{scheme}://***@{host}"),
            None => url.to_owned(),
        },
        None => url.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials() {
        assert_eq!(redact("postgres://u:p@h:5432/db"), "postgres://***@h:5432/db");
        assert_eq!(redact("sqlite://trunks.db"), "sqlite://trunks.db");
    }
}
