#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! Trunks data access layer.
//!
//! Explicit SQL over SQLite and PostgreSQL — no ORM (decision D11). The concrete
//! driver is chosen by the `docs/05-analise-db.md` analysis; until then this crate
//! only defines its error type and the shape of the connection settings.

pub mod error;

pub use error::{Error, Result};

/// Where the data lives. Resolved by the CLI from flags and environment
/// (`DATABASE_URL`, `POSTGRES_*`, `SQLITE_DB_PATH`).
#[derive(Debug, Clone)]
pub enum Backend {
    /// SQLite file database. WAL + busy timeout are applied on connect.
    Sqlite { path: std::path::PathBuf },
    /// PostgreSQL DSN (`postgres://user:pass@host:port/db`).
    Postgres { dsn: secrecy::SecretString },
}
