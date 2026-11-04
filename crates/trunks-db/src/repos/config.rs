//! `server_configs`: the single row holding the operating mode. Implements the `ConfigStore` port.

use trunks_types::ServerMode;

use crate::rows::ServerConfigRow;
use crate::{Db, Result, with_pool};

const GET: &str =
    "SELECT id, mode, initialized FROM server_configs WHERE deleted_at IS NULL ORDER BY id LIMIT 1";
const INSERT: &str = "INSERT INTO server_configs (created_at, updated_at, mode, initialized) VALUES ($1, $1, $2, $3)";

pub async fn get(db: &Db) -> Result<Option<ServerConfigRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(GET).fetch_optional(p).await?))
}

/// Inserts the configuration row when none exists. Returns `true` when it was created.
pub async fn init(db: &Db, mode: ServerMode) -> Result<bool> {
    if let Some(row) = get(db).await? {
        if row.initialized {
            return Ok(false);
        }
    }
    let now = chrono::Utc::now();
    with_pool!(db, |p| sqlx::query(INSERT)
        .bind(now)
        .bind(mode.as_str())
        .bind(true)
        .execute(p)
        .await
        .map(|r| r.rows_affected())?);
    Ok(true)
}
