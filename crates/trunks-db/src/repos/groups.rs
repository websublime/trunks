//! `tool_groups` writes. Lists are stored as JSON arrays of names.

use chrono::Utc;
use serde_json::Value;
use sqlx::types::Json;

use crate::rows::ToolGroupRow;
use crate::{Db, Result, with_pool};

const INSERT: &str = "INSERT INTO tool_groups (created_at, updated_at, name, description, included_tools, included_servers, excluded_tools)
     VALUES ($1, $1, $2, $3, $4, $5, $6)
     RETURNING id, name, description, included_tools, included_servers, excluded_tools";
const UPDATE: &str = "UPDATE tool_groups SET description = $1, included_tools = $2, included_servers = $3, excluded_tools = $4, updated_at = $5
     WHERE name = $6 AND deleted_at IS NULL";
const DELETE: &str = "DELETE FROM tool_groups WHERE name = $1 AND deleted_at IS NULL";

fn list(v: &[String]) -> Json<Value> {
    Json(Value::from(v.to_vec()))
}

/// Fails with a unique-violation error on a duplicate name.
pub async fn create(
    db: &Db,
    name: &str,
    description: &str,
    included_tools: &[String],
    included_servers: &[String],
    excluded_tools: &[String],
) -> Result<ToolGroupRow> {
    let now = Utc::now();
    Ok(with_pool!(db, |p| sqlx::query_as(INSERT)
        .bind(now)
        .bind(name)
        .bind(description)
        .bind(list(included_tools))
        .bind(list(included_servers))
        .bind(list(excluded_tools))
        .fetch_one(p)
        .await?))
}

/// Full replacement of every field. Returns `false` when there is no such group.
pub async fn update(
    db: &Db,
    name: &str,
    description: &str,
    included_tools: &[String],
    included_servers: &[String],
    excluded_tools: &[String],
) -> Result<bool> {
    let now = Utc::now();
    let n = with_pool!(db, |p| sqlx::query(UPDATE)
        .bind(description)
        .bind(list(included_tools))
        .bind(list(included_servers))
        .bind(list(excluded_tools))
        .bind(now)
        .bind(name)
        .execute(p)
        .await?
        .rows_affected());
    Ok(n > 0)
}

pub async fn delete(db: &Db, name: &str) -> Result<bool> {
    let n = with_pool!(db, |p| sqlx::query(DELETE).bind(name).execute(p).await?.rows_affected());
    Ok(n > 0)
}
