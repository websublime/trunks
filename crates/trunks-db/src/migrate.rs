//! Versioned migrations, one directory per dialect, identical numbering.
//!
//! `0001_baseline` is idempotent (`IF NOT EXISTS`) so a database created by a legacy
//! deployment is adopted without changes; `0002_normalize_legacy` rewrites legacy values
//! (URIs, mode aliases); `0003_entity_uniqueness` deduplicates and adds partial unique indexes.

use snafu::ResultExt;
use sqlx::migrate::Migrator;

use crate::{Db, Result, error};

static SQLITE: Migrator = sqlx::migrate!("./migrations/sqlite");
static POSTGRES: Migrator = sqlx::migrate!("./migrations/postgres");

/// sqlx's default bookkeeping table (the migrators are built with the default).
const MIGRATIONS_TABLE: &str = "_sqlx_migrations";

/// What `run` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// Versions applied by this call, in order (empty when already up to date).
    pub versions: Vec<i64>,
    /// Latest version known to the binary.
    pub latest: i64,
}

pub async fn run(db: &Db) -> Result<Applied> {
    let (migrator, before) = match db {
        Db::Sqlite(p) => (&SQLITE, applied_versions(p).await?),
        Db::Postgres(p) => (&POSTGRES, applied_versions(p).await?),
    };
    match db {
        Db::Sqlite(p) => migrator.run(p).await.context(error::MigrateSnafu)?,
        Db::Postgres(p) => migrator.run(p).await.context(error::MigrateSnafu)?,
    }
    let after = match db {
        Db::Sqlite(p) => applied_versions(p).await?,
        Db::Postgres(p) => applied_versions(p).await?,
    };
    let versions = after.iter().copied().filter(|v| !before.contains(v)).collect();
    let latest = migrator.iter().map(|m| m.version).max().unwrap_or(0);
    tracing::info!(backend = db.backend(), ?versions, latest, "migrations applied");
    Ok(Applied { versions, latest })
}

async fn applied_versions<'a, A>(pool: A) -> Result<Vec<i64>>
where
    A: sqlx::Acquire<'a>,
    <A::Database as sqlx::Database>::Connection: sqlx::migrate::Migrate,
{
    use sqlx::migrate::Migrate as _;
    let mut conn = pool.acquire().await?;
    conn.ensure_migrations_table(MIGRATIONS_TABLE).await.context(error::MigrateSnafu)?;
    let list = conn.list_applied_migrations(MIGRATIONS_TABLE).await.context(error::MigrateSnafu)?;
    Ok(list.into_iter().map(|m| m.version).collect())
}
