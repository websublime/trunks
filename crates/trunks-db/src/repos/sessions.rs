//! Browser sessions, OIDC identities and in-flight login states (D16).

use chrono::{DateTime, Utc};

use crate::rows::{LoginStateRow, UserRow};
use crate::{Db, Result, with_pool};

const USER_BY_OIDC: &str = "SELECT id, username, role, access_token FROM users WHERE oidc_issuer = $1 AND oidc_subject = $2 AND deleted_at IS NULL";
const INSERT_OIDC_USER: &str = "INSERT INTO users (created_at, updated_at, username, role, access_token, oidc_issuer, oidc_subject)
     VALUES ($1, $1, $2, $3, $4, $5, $6) RETURNING id, username, role, access_token";

pub async fn user_by_oidc(db: &Db, issuer: &str, subject: &str) -> Result<Option<UserRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(USER_BY_OIDC)
        .bind(issuer)
        .bind(subject)
        .fetch_optional(p)
        .await?))
}

pub async fn create_oidc_user(
    db: &Db,
    username: &str,
    role: &str,
    token: &str,
    issuer: &str,
    subject: &str,
) -> Result<UserRow> {
    let now = Utc::now();
    Ok(with_pool!(db, |p| sqlx::query_as(INSERT_OIDC_USER)
        .bind(now)
        .bind(username)
        .bind(role)
        .bind(token)
        .bind(issuer)
        .bind(subject)
        .fetch_one(p)
        .await?))
}

pub async fn set_user_role(db: &Db, user_id: i64, role: &str) -> Result<()> {
    let now = Utc::now();
    with_pool!(db, |p| {
        sqlx::query("UPDATE users SET role = $1, updated_at = $2 WHERE id = $3")
            .bind(role)
            .bind(now)
            .bind(user_id)
            .execute(p)
            .await?;
        Ok::<(), crate::Error>(())
    })
}

/// Claim-synced memberships become exactly `team_ids`; manual rows are left alone.
pub async fn sync_oidc_memberships(db: &Db, user_id: i64, team_ids: &[i64]) -> Result<()> {
    let now = Utc::now();
    with_pool!(db, |p| {
        let mut tx = p.begin().await?;
        let result: Result<()> = async {
            let current: Vec<(i64, String)> =
                sqlx::query_as("SELECT team_id, source FROM team_members WHERE user_id = $1")
                    .bind(user_id)
                    .fetch_all(&mut *tx)
                    .await?;
            for (team_id, source) in &current {
                if source == "oidc" && !team_ids.contains(team_id) {
                    sqlx::query("DELETE FROM team_members WHERE user_id = $1 AND team_id = $2")
                        .bind(user_id)
                        .bind(team_id)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            for team_id in team_ids {
                if !current.iter().any(|(t, _)| t == team_id) {
                    sqlx::query(
                        "INSERT INTO team_members (created_at, updated_at, team_id, user_id, role, source)
                         VALUES ($1, $1, $2, $3, 'member', 'oidc')",
                    )
                    .bind(now)
                    .bind(team_id)
                    .bind(user_id)
                    .execute(&mut *tx)
                    .await?;
                }
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => {
                tx.commit().await?;
                Ok(())
            }
            Err(e) => {
                let _ = tx.rollback().await;
                Err(e)
            }
        }
    })
}

pub async fn create_session(
    db: &Db,
    id_hash: &str,
    user_id: i64,
    expires_at: DateTime<Utc>,
) -> Result<()> {
    let now = Utc::now();
    with_pool!(db, |p| {
        sqlx::query(
            "INSERT INTO sessions (id_hash, user_id, created_at, expires_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(id_hash)
        .bind(user_id)
        .bind(now)
        .bind(expires_at)
        .execute(p)
        .await?;
        Ok::<(), crate::Error>(())
    })
}

pub async fn session_user(db: &Db, id_hash: &str) -> Result<Option<UserRow>> {
    let now = Utc::now();
    let sql = "SELECT u.id, u.username, u.role, u.access_token FROM sessions s JOIN users u ON u.id = s.user_id
               WHERE s.id_hash = $1 AND s.expires_at > $2 AND u.deleted_at IS NULL";
    Ok(with_pool!(db, |p| sqlx::query_as(sql).bind(id_hash).bind(now).fetch_optional(p).await?))
}

pub async fn delete_session(db: &Db, id_hash: &str) -> Result<()> {
    with_pool!(db, |p| {
        sqlx::query("DELETE FROM sessions WHERE id_hash = $1").bind(id_hash).execute(p).await?;
        Ok::<(), crate::Error>(())
    })
}

pub async fn purge_expired(db: &Db) -> Result<u64> {
    let now = Utc::now();
    with_pool!(db, |p| {
        let a = sqlx::query("DELETE FROM sessions WHERE expires_at <= $1")
            .bind(now)
            .execute(p)
            .await?
            .rows_affected();
        let b = sqlx::query("DELETE FROM oidc_login_states WHERE expires_at <= $1")
            .bind(now)
            .execute(p)
            .await?
            .rows_affected();
        Ok::<u64, crate::Error>(a + b)
    })
}

pub async fn save_login_state(
    db: &Db,
    state: &str,
    nonce: &str,
    code_verifier: &str,
    return_to: &str,
    expires_at: DateTime<Utc>,
) -> Result<()> {
    with_pool!(db, |p| {
        sqlx::query(
            "INSERT INTO oidc_login_states (state, nonce, code_verifier, return_to, expires_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(state)
        .bind(nonce)
        .bind(code_verifier)
        .bind(return_to)
        .bind(expires_at)
        .execute(p)
        .await?;
        Ok::<(), crate::Error>(())
    })
}

/// Single use: the row is deleted in the same transaction it is read in.
pub async fn take_login_state(db: &Db, state: &str) -> Result<Option<LoginStateRow>> {
    with_pool!(db, |p| {
        let mut tx = p.begin().await?;
        let row: Option<LoginStateRow> = sqlx::query_as(
            "SELECT state, nonce, code_verifier, return_to, expires_at FROM oidc_login_states WHERE state = $1",
        )
        .bind(state)
        .fetch_optional(&mut *tx)
        .await?;
        if row.is_some() {
            sqlx::query("DELETE FROM oidc_login_states WHERE state = $1")
                .bind(state)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok::<Option<LoginStateRow>, crate::Error>(row)
    })
}
