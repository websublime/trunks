//! Row structs — one per table, column names as in the schema. Internal to this crate.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::FromRow;
use sqlx::types::Json;

#[derive(Debug, Clone, FromRow)]
pub struct ServerRow {
    pub id: i64,
    pub name: String,
    pub transport: String,
    pub enabled: bool,
    pub description: Option<String>,
    pub config: Json<Value>,
    pub session_mode: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ToolRow {
    pub id: i64,
    pub server_id: i64,
    pub name: String,
    pub enabled: bool,
    pub description: Option<String>,
    pub input_schema: Option<Json<Value>>,
    pub annotations: Option<Json<Value>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct PromptRow {
    pub id: i64,
    pub server_id: i64,
    pub name: String,
    pub enabled: bool,
    pub description: Option<String>,
    pub arguments: Option<Json<Value>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ResourceRow {
    pub id: i64,
    pub server_id: i64,
    pub uri: String,
    pub original_uri: String,
    pub name: String,
    pub enabled: bool,
    pub description: Option<String>,
    pub mime_type: Option<String>,
    pub annotations: Option<Json<Value>>,
    pub meta: Option<Json<Value>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ToolGroupRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub included_tools: Option<Json<Value>>,
    pub included_servers: Option<Json<Value>>,
    pub excluded_tools: Option<Json<Value>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ServerConfigRow {
    pub id: i64,
    pub mode: String,
    pub initialized: bool,
}

#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub access_token: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct McpClientRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub access_token: String,
    pub allow_list: Json<Value>,
}

#[derive(Debug, Clone, FromRow)]
pub struct OAuthTokenRow {
    pub id: i64,
    pub server_name: String,
    pub transport: String,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub redirect_uri: Option<String>,
    pub scopes: Option<Json<Value>>,
    pub access_token: Option<String>,
    pub token_type: Option<String>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct OAuthPendingRow {
    pub id: i64,
    pub session_id: String,
    pub server_name: String,
    pub transport: String,
    pub server_input: Json<Value>,
    pub force: bool,
    pub redirect_uri: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub scopes: Option<Json<Value>>,
    pub state: String,
    pub code_verifier: String,
    pub expires_at: DateTime<Utc>,
    pub initiated_by: Option<String>,
}
