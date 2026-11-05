//! `users` and `mcp_clients`: reads and the identity CRUD.

use crate::rows::{McpClientRow, UserRow};
use crate::{Db, Result, with_pool};

const USERS: &str =
    "SELECT id, username, role, access_token FROM users WHERE deleted_at IS NULL ORDER BY username";
const USER_BY_TOKEN: &str = "SELECT id, username, role, access_token FROM users WHERE access_token = $1 AND deleted_at IS NULL";
const CLIENTS: &str = "SELECT id, name, description, access_token, allow_list FROM mcp_clients WHERE deleted_at IS NULL ORDER BY name";
const CLIENT_BY_TOKEN: &str = "SELECT id, name, description, access_token, allow_list FROM mcp_clients WHERE access_token = $1 AND deleted_at IS NULL";

const USER_BY_NAME: &str =
    "SELECT id, username, role, access_token FROM users WHERE username = $1 AND deleted_at IS NULL";
const UPDATE_USER_TOKEN: &str = "UPDATE users SET access_token = $1, updated_at = $2 WHERE username = $3 AND deleted_at IS NULL";
const DELETE_USER: &str = "DELETE FROM users WHERE username = $1 AND deleted_at IS NULL";
const CLIENT_BY_NAME: &str = "SELECT id, name, description, access_token, allow_list FROM mcp_clients WHERE name = $1 AND deleted_at IS NULL";
const INSERT_CLIENT: &str = "INSERT INTO mcp_clients (created_at, updated_at, name, description, access_token, allow_list) VALUES ($1, $1, $2, $3, $4, $5) RETURNING id, name, description, access_token, allow_list";
const UPDATE_CLIENT: &str = "UPDATE mcp_clients SET description = COALESCE($1, description), allow_list = COALESCE($2, allow_list), access_token = COALESCE($3, access_token), updated_at = $4 WHERE name = $5 AND deleted_at IS NULL";
const DELETE_CLIENT: &str = "DELETE FROM mcp_clients WHERE name = $1 AND deleted_at IS NULL";

const INSERT_USER: &str = "INSERT INTO users (created_at, updated_at, username, role, access_token) VALUES ($1, $1, $2, $3, $4) RETURNING id, username, role, access_token";

/// Fails with a unique-violation error when the username or token already exists.
pub async fn create_user(db: &Db, username: &str, role: &str, token: &str) -> Result<UserRow> {
    let now = chrono::Utc::now();
    Ok(with_pool!(db, |p| sqlx::query_as(INSERT_USER)
        .bind(now)
        .bind(username)
        .bind(role)
        .bind(token)
        .fetch_one(p)
        .await?))
}

pub async fn users(db: &Db) -> Result<Vec<UserRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(USERS).fetch_all(p).await?))
}

pub async fn user_by_token(db: &Db, token: &str) -> Result<Option<UserRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(USER_BY_TOKEN).bind(token).fetch_optional(p).await?))
}

pub async fn clients(db: &Db) -> Result<Vec<McpClientRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(CLIENTS).fetch_all(p).await?))
}

pub async fn client_by_token(db: &Db, token: &str) -> Result<Option<McpClientRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(CLIENT_BY_TOKEN).bind(token).fetch_optional(p).await?))
}

pub async fn user_by_name(db: &Db, username: &str) -> Result<Option<UserRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(USER_BY_NAME).bind(username).fetch_optional(p).await?))
}

pub async fn update_user_token(db: &Db, username: &str, token: &str) -> Result<bool> {
    let now = chrono::Utc::now();
    let n = with_pool!(db, |p| sqlx::query(UPDATE_USER_TOKEN)
        .bind(token)
        .bind(now)
        .bind(username)
        .execute(p)
        .await?
        .rows_affected());
    Ok(n > 0)
}

pub async fn delete_user(db: &Db, username: &str) -> Result<bool> {
    let n = with_pool!(db, |p| sqlx::query(DELETE_USER)
        .bind(username)
        .execute(p)
        .await?
        .rows_affected());
    Ok(n > 0)
}

pub async fn client_by_name(db: &Db, name: &str) -> Result<Option<McpClientRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(CLIENT_BY_NAME).bind(name).fetch_optional(p).await?))
}

/// Fails with a unique-violation error when the name or token already exists.
pub async fn create_client(
    db: &Db,
    name: &str,
    description: &str,
    token: &str,
    allow_list: &[String],
) -> Result<McpClientRow> {
    let now = chrono::Utc::now();
    let allow = sqlx::types::Json(serde_json::Value::from(allow_list.to_vec()));
    Ok(with_pool!(db, |p| sqlx::query_as(INSERT_CLIENT)
        .bind(now)
        .bind(name)
        .bind(description)
        .bind(token)
        .bind(allow)
        .fetch_one(p)
        .await?))
}

/// `None` fields keep their current value (`COALESCE`).
pub async fn update_client(
    db: &Db,
    name: &str,
    description: Option<&str>,
    allow_list: Option<&[String]>,
    token: Option<&str>,
) -> Result<bool> {
    let now = chrono::Utc::now();
    let allow = allow_list.map(|l| sqlx::types::Json(serde_json::Value::from(l.to_vec())));
    let n = with_pool!(db, |p| sqlx::query(UPDATE_CLIENT)
        .bind(description)
        .bind(allow)
        .bind(token)
        .bind(now)
        .bind(name)
        .execute(p)
        .await?
        .rows_affected());
    Ok(n > 0)
}

pub async fn delete_client(db: &Db, name: &str) -> Result<bool> {
    let n =
        with_pool!(db, |p| sqlx::query(DELETE_CLIENT).bind(name).execute(p).await?.rows_affected());
    Ok(n > 0)
}
