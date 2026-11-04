//! `users` and `mcp_clients` reads.

use crate::rows::{McpClientRow, UserRow};
use crate::{Db, Result, with_pool};

const USERS: &str =
    "SELECT id, username, role, access_token FROM users WHERE deleted_at IS NULL ORDER BY username";
const USER_BY_TOKEN: &str = "SELECT id, username, role, access_token FROM users WHERE access_token = $1 AND deleted_at IS NULL";
const CLIENTS: &str = "SELECT id, name, description, access_token, allow_list FROM mcp_clients WHERE deleted_at IS NULL ORDER BY name";
const CLIENT_BY_TOKEN: &str = "SELECT id, name, description, access_token, allow_list FROM mcp_clients WHERE access_token = $1 AND deleted_at IS NULL";

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
