//! `users` and `mcp_clients` reads.

use crate::rows::{McpClientRow, UserRow};
use crate::{Db, Result, with_pool};

const USERS: &str =
    "SELECT id, username, role, access_token FROM users WHERE deleted_at IS NULL ORDER BY username";
const USER_BY_TOKEN: &str = "SELECT id, username, role, access_token FROM users WHERE access_token = $1 AND deleted_at IS NULL";
const CLIENTS: &str = "SELECT id, name, description, access_token, allow_list FROM mcp_clients WHERE deleted_at IS NULL ORDER BY name";
const CLIENT_BY_TOKEN: &str = "SELECT id, name, description, access_token, allow_list FROM mcp_clients WHERE access_token = $1 AND deleted_at IS NULL";

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
