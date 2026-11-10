//! Upstream OAuth: pending authorization sessions and stored credentials.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;

use crate::rows::{OAuthPendingRow, OAuthTokenRow};
use crate::{Db, Result, with_pool};

// One literal SQL string per operation (D11); sqlx requires `&'static str` for unaudited SQL.
const TOKEN_BY_SERVER: &str = "SELECT id, server_name, transport, client_id, client_secret, redirect_uri, scopes, access_token, token_type, refresh_token, scope, expires_at, issuer
     FROM upstream_oauth_tokens WHERE server_name = $1 AND deleted_at IS NULL";
const PENDING_ALL: &str = "SELECT id, session_id, server_name, transport, server_input, force, redirect_uri, client_id, client_secret, scopes, state, code_verifier, expires_at, initiated_by
     FROM upstream_oauth_pending_sessions WHERE deleted_at IS NULL ORDER BY id";
const PENDING_BY_SESSION: &str = "SELECT id, session_id, server_name, transport, server_input, force, redirect_uri, client_id, client_secret, scopes, state, code_verifier, expires_at, initiated_by
     FROM upstream_oauth_pending_sessions WHERE session_id = $1 AND deleted_at IS NULL";
const PENDING_BY_STATE: &str = "SELECT id, session_id, server_name, transport, server_input, force, redirect_uri, client_id, client_secret, scopes, state, code_verifier, expires_at, initiated_by
     FROM upstream_oauth_pending_sessions WHERE state = $1 AND deleted_at IS NULL";

/// Everything persisted for one upstream's OAuth client and tokens.
#[derive(Debug, Clone, Default)]
pub struct OAuthTokenWrite {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub redirect_uri: Option<String>,
    pub scopes: Vec<String>,
    pub access_token: Option<String>,
    pub token_type: Option<String>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub issuer: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthPendingWrite {
    pub session_id: String,
    pub server_name: String,
    pub transport: String,
    pub server_input: Value,
    pub force: bool,
    pub redirect_uri: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub scopes: Vec<String>,
    pub state: String,
    pub code_verifier: String,
    pub expires_at: DateTime<Utc>,
    pub initiated_by: Option<String>,
}

pub async fn token_by_server(db: &Db, server_name: &str) -> Result<Option<OAuthTokenRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(TOKEN_BY_SERVER)
        .bind(server_name)
        .fetch_optional(p)
        .await?))
}

/// Inserts or fully replaces the row of `server_name` (one transaction).
pub async fn upsert_token(
    db: &Db,
    server_name: &str,
    transport: &str,
    w: &OAuthTokenWrite,
) -> Result<()> {
    let now = Utc::now();
    let scopes = Json(Value::from(w.scopes.clone()));
    let insert = "INSERT INTO upstream_oauth_tokens (created_at, updated_at, server_name, transport, client_id, client_secret, redirect_uri, scopes,
             access_token, token_type, refresh_token, scope, expires_at, issuer)
         VALUES ($1, $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)";
    with_pool!(db, |p| {
        let mut tx = p.begin().await?;
        sqlx::query("DELETE FROM upstream_oauth_tokens WHERE server_name = $1")
            .bind(server_name)
            .execute(&mut *tx)
            .await?;
        sqlx::query(insert)
            .bind(now)
            .bind(server_name)
            .bind(transport)
            .bind(&w.client_id)
            .bind(&w.client_secret)
            .bind(&w.redirect_uri)
            .bind(scopes)
            .bind(&w.access_token)
            .bind(&w.token_type)
            .bind(&w.refresh_token)
            .bind(&w.scope)
            .bind(w.expires_at)
            .bind(&w.issuer)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok::<(), crate::Error>(())
    })
}

/// Clears only the token fields; the client registration metadata stays.
pub async fn clear_token_fields(db: &Db, server_name: &str) -> Result<()> {
    let now = Utc::now();
    with_pool!(db, |p| {
        sqlx::query("UPDATE upstream_oauth_tokens SET access_token = NULL, token_type = NULL, refresh_token = NULL, scope = NULL, expires_at = NULL, updated_at = $1 WHERE server_name = $2")
            .bind(now)
            .bind(server_name)
            .execute(p)
            .await?;
        Ok::<(), crate::Error>(())
    })
}

pub async fn delete_token(db: &Db, server_name: &str) -> Result<()> {
    with_pool!(db, |p| {
        sqlx::query("DELETE FROM upstream_oauth_tokens WHERE server_name = $1")
            .bind(server_name)
            .execute(p)
            .await?;
        Ok::<(), crate::Error>(())
    })
}

pub async fn pending_sessions(db: &Db) -> Result<Vec<OAuthPendingRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(PENDING_ALL).fetch_all(p).await?))
}

pub async fn pending_by_session(db: &Db, session_id: &str) -> Result<Option<OAuthPendingRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(PENDING_BY_SESSION)
        .bind(session_id)
        .fetch_optional(p)
        .await?))
}

pub async fn pending_by_state(db: &Db, state: &str) -> Result<Option<OAuthPendingRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(PENDING_BY_STATE).bind(state).fetch_optional(p).await?))
}

/// Replaces any pending session of the same server (one transaction).
pub async fn replace_pending(db: &Db, w: &OAuthPendingWrite) -> Result<()> {
    let now = Utc::now();
    let scopes = Json(Value::from(w.scopes.clone()));
    let insert = "INSERT INTO upstream_oauth_pending_sessions (created_at, updated_at, session_id, server_name, transport, server_input, force,
             redirect_uri, client_id, client_secret, scopes, state, code_verifier, expires_at, initiated_by)
         VALUES ($1, $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)";
    with_pool!(db, |p| {
        let mut tx = p.begin().await?;
        sqlx::query("DELETE FROM upstream_oauth_pending_sessions WHERE server_name = $1")
            .bind(&w.server_name)
            .execute(&mut *tx)
            .await?;
        sqlx::query(insert)
            .bind(now)
            .bind(&w.session_id)
            .bind(&w.server_name)
            .bind(&w.transport)
            .bind(Json(w.server_input.clone()))
            .bind(w.force)
            .bind(&w.redirect_uri)
            .bind(&w.client_id)
            .bind(&w.client_secret)
            .bind(scopes)
            .bind(&w.state)
            .bind(&w.code_verifier)
            .bind(w.expires_at)
            .bind(&w.initiated_by)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok::<(), crate::Error>(())
    })
}

pub async fn delete_pending_by_session(db: &Db, session_id: &str) -> Result<()> {
    with_pool!(db, |p| {
        sqlx::query("DELETE FROM upstream_oauth_pending_sessions WHERE session_id = $1")
            .bind(session_id)
            .execute(p)
            .await?;
        Ok::<(), crate::Error>(())
    })
}

pub async fn delete_pending_by_server(db: &Db, server_name: &str) -> Result<()> {
    with_pool!(db, |p| {
        sqlx::query("DELETE FROM upstream_oauth_pending_sessions WHERE server_name = $1")
            .bind(server_name)
            .execute(p)
            .await?;
        Ok::<(), crate::Error>(())
    })
}

/// Removes sessions past their `expires_at`. Returns how many were deleted.
pub async fn purge_expired_pending(db: &Db) -> Result<u64> {
    let now = Utc::now();
    Ok(with_pool!(db, |p| sqlx::query(
        "DELETE FROM upstream_oauth_pending_sessions WHERE expires_at < $1"
    )
    .bind(now)
    .execute(p)
    .await?
    .rows_affected()))
}
