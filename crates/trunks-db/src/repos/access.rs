//! Teams, memberships and grants (D14).

use chrono::Utc;

use crate::rows::{BareGrantRow, GrantRow, TeamMemberRow, TeamRow, UserTeamRow};
use crate::{Db, Result, with_pool};

const TEAMS: &str =
    "SELECT id, name, description FROM teams WHERE deleted_at IS NULL ORDER BY name";
const TEAM_BY_NAME: &str =
    "SELECT id, name, description FROM teams WHERE name = $1 AND deleted_at IS NULL";
const TEAM_ID_BY_NAME: &str = "SELECT id FROM teams WHERE name = $1 AND deleted_at IS NULL";
const TEAM_BY_ID: &str =
    "SELECT id, name, description FROM teams WHERE id = $1 AND deleted_at IS NULL";
const INSERT_TEAM: &str = "INSERT INTO teams (created_at, updated_at, name, description) VALUES ($1, $1, $2, $3) RETURNING id, name, description";
const UPDATE_TEAM: &str =
    "UPDATE teams SET description = $1, updated_at = $2 WHERE name = $3 AND deleted_at IS NULL";
const MEMBERS: &str = "SELECT m.team_id, m.user_id, u.username, m.role FROM team_members m JOIN users u ON u.id = m.user_id
                       WHERE m.team_id = $1 AND u.deleted_at IS NULL ORDER BY u.username";
const TEAMS_OF_USER: &str = "SELECT t.id, t.name, t.description, m.role FROM team_members m JOIN teams t ON t.id = m.team_id
                             WHERE m.user_id = $1 AND t.deleted_at IS NULL ORDER BY t.name";
const DELETE_MEMBER: &str = "DELETE FROM team_members WHERE team_id = $1 AND user_id = $2";
const UPDATE_MEMBER_ROLE: &str =
    "UPDATE team_members SET role = $1, updated_at = $2 WHERE team_id = $3 AND user_id = $4";
const INSERT_MEMBER: &str = "INSERT INTO team_members (created_at, updated_at, team_id, user_id, role) VALUES ($1, $1, $2, $3, $4)";
const GRANTS_FOR_USER: &str = "SELECT target_kind, target_name FROM access_grants
    WHERE (subject_kind = 'user' AND subject_id = $1)
       OR (subject_kind = 'team' AND subject_id IN (SELECT team_id FROM team_members WHERE user_id = $1))";
const GRANTS_FOR_SUBJECT: &str = "SELECT target_kind, target_name FROM access_grants WHERE subject_kind = $1 AND subject_id = $2";
const GRANTS_JOINED: &str = "SELECT g.subject_kind, g.subject_id,
        COALESCE(u.username, t.name, '') AS subject_name, g.target_kind, g.target_name
    FROM access_grants g
    LEFT JOIN users u ON g.subject_kind = 'user' AND u.id = g.subject_id
    LEFT JOIN teams t ON g.subject_kind = 'team' AND t.id = g.subject_id
    WHERE ($1 IS NULL OR g.target_kind = $1) AND ($2 IS NULL OR g.target_name = $2)
    ORDER BY g.target_kind, g.target_name, g.subject_kind, subject_name";
const GRANT_EXISTS: &str = "SELECT id FROM access_grants WHERE subject_kind = $1 AND subject_id = $2 AND target_kind = $3 AND target_name = $4";
const INSERT_GRANT: &str = "INSERT INTO access_grants (created_at, updated_at, subject_kind, subject_id, target_kind, target_name, permission) VALUES ($1, $1, $2, $3, $4, $5, 'use')";
const DELETE_GRANT: &str = "DELETE FROM access_grants WHERE subject_kind = $1 AND subject_id = $2 AND target_kind = $3 AND target_name = $4";

pub async fn teams(db: &Db) -> Result<Vec<TeamRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(TEAMS).fetch_all(p).await?))
}

pub async fn team_by_name(db: &Db, name: &str) -> Result<Option<TeamRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(TEAM_BY_NAME).bind(name).fetch_optional(p).await?))
}

pub async fn team_by_id(db: &Db, id: i64) -> Result<Option<TeamRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(TEAM_BY_ID).bind(id).fetch_optional(p).await?))
}

pub async fn create_team(db: &Db, name: &str, description: &str) -> Result<TeamRow> {
    let now = Utc::now();
    Ok(with_pool!(db, |p| sqlx::query_as(INSERT_TEAM)
        .bind(now)
        .bind(name)
        .bind(description)
        .fetch_one(p)
        .await?))
}

pub async fn update_team(db: &Db, name: &str, description: &str) -> Result<bool> {
    let now = Utc::now();
    let n = with_pool!(db, |p| sqlx::query(UPDATE_TEAM)
        .bind(description)
        .bind(now)
        .bind(name)
        .execute(p)
        .await?
        .rows_affected());
    Ok(n > 0)
}

/// Team, memberships and grants in one transaction.
pub async fn delete_team(db: &Db, name: &str) -> Result<bool> {
    with_pool!(db, |p| {
        let mut tx = p.begin().await?;
        let id: Option<i64> =
            sqlx::query_scalar(TEAM_ID_BY_NAME).bind(name).fetch_optional(&mut *tx).await?;
        let Some(id) = id else {
            tx.rollback().await?;
            return Ok(false);
        };
        let result: Result<()> = async {
            sqlx::query("DELETE FROM team_members WHERE team_id = $1").bind(id).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM access_grants WHERE subject_kind = 'team' AND subject_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE mcp_clients SET owner_kind = NULL, owner_id = NULL WHERE owner_kind = 'team' AND owner_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM teams WHERE id = $1").bind(id).execute(&mut *tx).await?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => {
                tx.commit().await?;
                Ok(true)
            }
            Err(e) => {
                let _ = tx.rollback().await;
                Err(e)
            }
        }
    })
}

pub async fn members(db: &Db, team_id: i64) -> Result<Vec<TeamMemberRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(MEMBERS).bind(team_id).fetch_all(p).await?))
}

pub async fn teams_of_user(db: &Db, user_id: i64) -> Result<Vec<UserTeamRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(TEAMS_OF_USER).bind(user_id).fetch_all(p).await?))
}

/// Upsert of the membership role.
pub async fn add_member(db: &Db, team_id: i64, user_id: i64, role: &str) -> Result<()> {
    let now = Utc::now();
    with_pool!(db, |p| {
        let updated = sqlx::query(UPDATE_MEMBER_ROLE)
            .bind(role)
            .bind(now)
            .bind(team_id)
            .bind(user_id)
            .execute(p)
            .await?
            .rows_affected();
        if updated == 0 {
            sqlx::query(INSERT_MEMBER)
                .bind(now)
                .bind(team_id)
                .bind(user_id)
                .bind(role)
                .execute(p)
                .await?;
        }
        Ok::<(), crate::Error>(())
    })
}

pub async fn remove_member(db: &Db, team_id: i64, user_id: i64) -> Result<bool> {
    let n = with_pool!(db, |p| sqlx::query(DELETE_MEMBER)
        .bind(team_id)
        .bind(user_id)
        .execute(p)
        .await?
        .rows_affected());
    Ok(n > 0)
}

pub async fn grants_for_user(db: &Db, user_id: i64) -> Result<Vec<BareGrantRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(GRANTS_FOR_USER).bind(user_id).fetch_all(p).await?))
}

pub async fn grants_for_subject(db: &Db, kind: &str, id: i64) -> Result<Vec<BareGrantRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(GRANTS_FOR_SUBJECT)
        .bind(kind)
        .bind(id)
        .fetch_all(p)
        .await?))
}

pub async fn grants(db: &Db, target: Option<(&str, &str)>) -> Result<Vec<GrantRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(GRANTS_JOINED)
        .bind(target.map(|t| t.0))
        .bind(target.map(|t| t.1))
        .fetch_all(p)
        .await?))
}

pub async fn add_grant(db: &Db, subject: (&str, i64), target: (&str, &str)) -> Result<bool> {
    let now = Utc::now();
    with_pool!(db, |p| {
        let existing: Option<i64> = sqlx::query_scalar(GRANT_EXISTS)
            .bind(subject.0)
            .bind(subject.1)
            .bind(target.0)
            .bind(target.1)
            .fetch_optional(p)
            .await?;
        if existing.is_some() {
            return Ok(false);
        }
        sqlx::query(INSERT_GRANT)
            .bind(now)
            .bind(subject.0)
            .bind(subject.1)
            .bind(target.0)
            .bind(target.1)
            .execute(p)
            .await?;
        Ok::<bool, crate::Error>(true)
    })
}

pub async fn remove_grant(db: &Db, subject: (&str, i64), target: (&str, &str)) -> Result<bool> {
    let n = with_pool!(db, |p| sqlx::query(DELETE_GRANT)
        .bind(subject.0)
        .bind(subject.1)
        .bind(target.0)
        .bind(target.1)
        .execute(p)
        .await?
        .rows_affected());
    Ok(n > 0)
}
