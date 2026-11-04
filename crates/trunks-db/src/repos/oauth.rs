//! Upstream OAuth tokens and pending sessions (reads; writes arrive in Phase 5).

use crate::rows::{OAuthPendingRow, OAuthTokenRow};
use crate::{Db, Result, with_pool};

const TOKEN_BY_SERVER: &str = "SELECT id, server_name, transport, client_id, client_secret, redirect_uri, scopes,
                                      access_token, token_type, refresh_token, scope, expires_at
                               FROM upstream_oauth_tokens WHERE server_name = $1 AND deleted_at IS NULL";
const PENDING: &str =
    "SELECT id, session_id, server_name, transport, server_input, force, redirect_uri, client_id,
                              client_secret, scopes, state, code_verifier, expires_at, initiated_by
                       FROM upstream_oauth_pending_sessions WHERE deleted_at IS NULL ORDER BY id";

pub async fn token_by_server(db: &Db, server_name: &str) -> Result<Option<OAuthTokenRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(TOKEN_BY_SERVER)
        .bind(server_name)
        .fetch_optional(p)
        .await?))
}

pub async fn pending_sessions(db: &Db) -> Result<Vec<OAuthPendingRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(PENDING).fetch_all(p).await?))
}
