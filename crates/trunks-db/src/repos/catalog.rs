//! Reads used to build the in-memory catalog.

use crate::rows::{PromptRow, ResourceRow, ServerRow, ToolGroupRow, ToolRow};
use crate::{Db, Result, with_pool};

const SERVERS: &str =
    "SELECT id, name, transport, enabled, description, config, session_mode, created_at, updated_at
                       FROM mcp_servers WHERE deleted_at IS NULL ORDER BY name";
const TOOLS: &str = "SELECT id, server_id, name, enabled, description, input_schema, annotations
                     FROM tools WHERE deleted_at IS NULL ORDER BY server_id, name";
const PROMPTS: &str = "SELECT id, server_id, name, enabled, description, arguments
                       FROM prompts WHERE deleted_at IS NULL ORDER BY server_id, name";
const RESOURCES: &str = "SELECT id, server_id, uri, original_uri, name, enabled, description, mime_type, annotations, meta
                         FROM resources WHERE deleted_at IS NULL ORDER BY server_id, uri";
const GROUPS: &str =
    "SELECT id, name, description, included_tools, included_servers, excluded_tools
                      FROM tool_groups WHERE deleted_at IS NULL ORDER BY name";
const TOOLS_BY_SERVER: &str = "SELECT id, server_id, name, enabled, description, input_schema, annotations
                               FROM tools WHERE server_id = $1 AND deleted_at IS NULL ORDER BY name";

/// All live servers (enabled and disabled), ordered by name.
pub async fn servers(db: &Db) -> Result<Vec<ServerRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(SERVERS).fetch_all(p).await?))
}

pub async fn tools(db: &Db) -> Result<Vec<ToolRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(TOOLS).fetch_all(p).await?))
}

pub async fn tools_by_server(db: &Db, server_id: i64) -> Result<Vec<ToolRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(TOOLS_BY_SERVER).bind(server_id).fetch_all(p).await?))
}

pub async fn prompts(db: &Db) -> Result<Vec<PromptRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(PROMPTS).fetch_all(p).await?))
}

pub async fn resources(db: &Db) -> Result<Vec<ResourceRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(RESOURCES).fetch_all(p).await?))
}

pub async fn tool_groups(db: &Db) -> Result<Vec<ToolGroupRow>> {
    Ok(with_pool!(db, |p| sqlx::query_as(GROUPS).fetch_all(p).await?))
}
