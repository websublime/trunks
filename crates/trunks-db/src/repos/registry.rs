//! Registry writes: register/deregister a server with its entities, enable/disable flags.
//! Every multi-row change runs in one transaction.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;

use crate::rows::ServerRow;
use crate::{Db, Result, with_pool};

/// Column-level view of a server to insert (secrets already serialized into `config`).
#[derive(Debug, Clone)]
pub struct NewServerRow {
    pub name: String,
    pub transport: String,
    pub description: String,
    pub config: Value,
    pub session_mode: String,
}

#[derive(Debug, Clone)]
pub struct NewToolRow {
    pub name: String,
    pub description: String,
    pub input_schema: Option<Value>,
    pub annotations: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct NewPromptRow {
    pub name: String,
    pub description: String,
    pub arguments: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct NewResourceRow {
    pub uri: String,
    pub original_uri: String,
    pub name: String,
    pub description: String,
    pub mime_type: Option<String>,
    pub annotations: Option<Value>,
    pub meta: Option<Value>,
}

const SERVER_ID_BY_NAME: &str = "SELECT id FROM mcp_servers WHERE name = $1 AND deleted_at IS NULL";
const SERVER_ENABLED_BY_NAME: &str =
    "SELECT enabled FROM mcp_servers WHERE name = $1 AND deleted_at IS NULL";
const INSERT_SERVER: &str = "INSERT INTO mcp_servers (created_at, updated_at, name, transport, enabled, description, config, session_mode)
     VALUES ($1, $1, $2, $3, $4, $5, $6, $7)
     RETURNING id, name, transport, enabled, description, config, session_mode, created_at, updated_at";
const INSERT_TOOL: &str = "INSERT INTO tools (created_at, updated_at, name, enabled, description, input_schema, annotations, server_id)
     VALUES ($1, $1, $2, $3, $4, $5, $6, $7)";
const INSERT_PROMPT: &str =
    "INSERT INTO prompts (created_at, updated_at, name, enabled, description, arguments, server_id)
     VALUES ($1, $1, $2, $3, $4, $5, $6)";
const INSERT_RESOURCE: &str = "INSERT INTO resources (created_at, updated_at, uri, original_uri, name, enabled, description, mime_type, annotations, meta, server_id)
     VALUES ($1, $1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";
const DELETE_TOOLS: &str = "DELETE FROM tools WHERE server_id = $1";
const DELETE_PROMPTS: &str = "DELETE FROM prompts WHERE server_id = $1";
const DELETE_RESOURCES: &str = "DELETE FROM resources WHERE server_id = $1";
const DELETE_SERVER: &str = "DELETE FROM mcp_servers WHERE id = $1";
const UPDATE_SERVER_ENABLED: &str =
    "UPDATE mcp_servers SET enabled = $1, updated_at = $2 WHERE name = $3 AND deleted_at IS NULL";
const UPDATE_TOOLS_ENABLED: &str = "UPDATE tools SET enabled = $1, updated_at = $2
     WHERE server_id = $3 AND enabled <> $1 AND deleted_at IS NULL AND ($4 IS NULL OR name = $4)
     RETURNING name";
const UPDATE_PROMPTS_ENABLED: &str = "UPDATE prompts SET enabled = $1, updated_at = $2
     WHERE server_id = $3 AND enabled <> $1 AND deleted_at IS NULL AND ($4 IS NULL OR name = $4)
     RETURNING name";
const UPDATE_RESOURCES_ENABLED: &str = "UPDATE resources SET enabled = $1, updated_at = $2
     WHERE server_id = $3 AND enabled <> $1 AND deleted_at IS NULL AND ($4 IS NULL OR uri = $4)
     RETURNING uri";

/// Deletes the entities of server `$id` and the server row itself, on an open transaction.
/// A macro (not a function) so the same text compiles against either backend's transaction.
macro_rules! delete_entities {
    ($tx:ident, $id:expr) => {{
        sqlx::query(DELETE_TOOLS).bind($id).execute(&mut *$tx).await?;
        sqlx::query(DELETE_PROMPTS).bind($id).execute(&mut *$tx).await?;
        sqlx::query(DELETE_RESOURCES).bind($id).execute(&mut *$tx).await?;
        sqlx::query(DELETE_SERVER).bind($id).execute(&mut *$tx).await?;
    }};
}
fn json(v: Option<&Value>) -> Option<Json<Value>> {
    v.cloned().map(Json)
}

/// Inserts the server and its entities in one transaction. With `replace`, a live server of
/// the same name is removed first (same transaction). Without it, a duplicate name fails on
/// the unique constraint.
pub async fn create_server(
    db: &Db,
    server: &NewServerRow,
    tools: &[NewToolRow],
    prompts: &[NewPromptRow],
    resources: &[NewResourceRow],
    replace: bool,
) -> Result<ServerRow> {
    let now: DateTime<Utc> = Utc::now();
    with_pool!(db, |p| {
        let mut tx = p.begin().await?;
        let result: Result<ServerRow> = async {
            if replace {
                let existing: Option<i64> = sqlx::query_scalar(SERVER_ID_BY_NAME)
                    .bind(&server.name)
                    .fetch_optional(&mut *tx)
                    .await?;
                if let Some(id) = existing {
                    delete_entities!(tx, id);
                }
            }
            let row: ServerRow = sqlx::query_as(INSERT_SERVER)
                .bind(now)
                .bind(&server.name)
                .bind(&server.transport)
                .bind(true)
                .bind(&server.description)
                .bind(Json(server.config.clone()))
                .bind(&server.session_mode)
                .fetch_one(&mut *tx)
                .await?;
            for t in tools {
                sqlx::query(INSERT_TOOL)
                    .bind(now)
                    .bind(&t.name)
                    .bind(true)
                    .bind(&t.description)
                    .bind(json(t.input_schema.as_ref()))
                    .bind(json(t.annotations.as_ref()))
                    .bind(row.id)
                    .execute(&mut *tx)
                    .await?;
            }
            for pr in prompts {
                sqlx::query(INSERT_PROMPT)
                    .bind(now)
                    .bind(&pr.name)
                    .bind(true)
                    .bind(&pr.description)
                    .bind(json(pr.arguments.as_ref()))
                    .bind(row.id)
                    .execute(&mut *tx)
                    .await?;
            }
            for r in resources {
                sqlx::query(INSERT_RESOURCE)
                    .bind(now)
                    .bind(&r.uri)
                    .bind(&r.original_uri)
                    .bind(&r.name)
                    .bind(true)
                    .bind(&r.description)
                    .bind(&r.mime_type)
                    .bind(json(r.annotations.as_ref()))
                    .bind(json(r.meta.as_ref()))
                    .bind(row.id)
                    .execute(&mut *tx)
                    .await?;
            }
            Ok(row)
        }
        .await;
        // Release the write lock deterministically: a transaction dropped with a pending
        // rollback can make the next `begin()` on another pooled connection fail with
        // SQLITE_BUSY instead of waiting.
        match result {
            Ok(row) => {
                tx.commit().await?;
                Ok(row)
            }
            Err(e) => {
                let _ = tx.rollback().await;
                Err(e)
            }
        }
    })
}

/// Hard-deletes the server and its entities. Returns `false` when it did not exist.
pub async fn delete_server(db: &Db, name: &str) -> Result<bool> {
    with_pool!(db, |p| {
        let mut tx = p.begin().await?;
        let existing: Option<i64> =
            sqlx::query_scalar(SERVER_ID_BY_NAME).bind(name).fetch_optional(&mut *tx).await?;
        let Some(id) = existing else {
            tx.rollback().await?;
            return Ok(false);
        };
        let result: Result<()> = async {
            delete_entities!(tx, id);
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

/// `None` = no such server; `Some(changed)` otherwise.
pub async fn set_server_enabled(db: &Db, name: &str, enabled: bool) -> Result<Option<bool>> {
    let now = Utc::now();
    with_pool!(db, |p| {
        let current: Option<bool> =
            sqlx::query_scalar(SERVER_ENABLED_BY_NAME).bind(name).fetch_optional(p).await?;
        match current {
            None => Ok(None),
            Some(c) if c == enabled => Ok(Some(false)),
            Some(_) => {
                sqlx::query(UPDATE_SERVER_ENABLED)
                    .bind(enabled)
                    .bind(now)
                    .bind(name)
                    .execute(p)
                    .await?;
                Ok(Some(true))
            }
        }
    })
}

/// Flips every tool of the server (or only `name`) not already in that state; returns the
/// bare names that changed.
pub async fn set_tools_enabled(
    db: &Db,
    server_id: i64,
    name: Option<&str>,
    enabled: bool,
) -> Result<Vec<String>> {
    flip(db, UPDATE_TOOLS_ENABLED, server_id, name, enabled).await
}

pub async fn set_prompts_enabled(
    db: &Db,
    server_id: i64,
    name: Option<&str>,
    enabled: bool,
) -> Result<Vec<String>> {
    flip(db, UPDATE_PROMPTS_ENABLED, server_id, name, enabled).await
}

/// Selected by public URI; returns the URIs that changed.
pub async fn set_resources_enabled(
    db: &Db,
    server_id: i64,
    uri: Option<&str>,
    enabled: bool,
) -> Result<Vec<String>> {
    flip(db, UPDATE_RESOURCES_ENABLED, server_id, uri, enabled).await
}

async fn flip(
    db: &Db,
    sql: &str,
    server_id: i64,
    key: Option<&str>,
    enabled: bool,
) -> Result<Vec<String>> {
    let now = Utc::now();
    Ok(with_pool!(db, |p| sqlx::query_scalar(sql)
        .bind(enabled)
        .bind(now)
        .bind(server_id)
        .bind(key)
        .fetch_all(p)
        .await?))
}
