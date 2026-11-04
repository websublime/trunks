//! REST API types (`/api/v0`). Field names are the wire contract — stable within 1.x.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CanonicalName, ResourceUri, Role, ServerMode, ServerName, SessionMode, Transport};

/// `GET /health`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Health {
    pub status: String,
}

/// `GET /metadata`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerMetadata {
    pub version: String,
}

/// `POST /init` body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InitRequest {
    pub mode: ServerMode,
}

/// `POST /init` response. `admin_access_token` is returned exactly once, in enterprise mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InitResponse {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin_access_token: Option<String>,
}

/// A registered upstream server as seen by any authenticated user.
/// Credentials never appear here: `env_keys` and `header_keys` list names only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServer {
    pub name: ServerName,
    pub transport: Transport,
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub session_mode: SessionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

/// A tool exposed by the gateway. `name` is canonical (`<server>__<tool>`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tool {
    pub name: CanonicalName,
    pub server: ServerName,
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
}

/// A prompt exposed by the gateway. `name` is canonical (`<server>__<prompt>`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Prompt {
    pub name: CanonicalName,
    pub server: ServerName,
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

/// A resource exposed by the gateway under a `trunks://res/...` URI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Resource {
    pub uri: ResourceUri,
    pub server: ServerName,
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

/// `GET /api/v0/users/whoami`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    pub username: String,
    pub role: Role,
}

/// Query string of `GET /api/v0/tools|prompts|resources`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ByServerQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Query string of `GET /api/v0/tool|prompt`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ByNameQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `POST /api/v0/tools/invoke` — `name` plus the tool's arguments as the rest of the object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolInvokeRequest {
    pub name: CanonicalName,
    #[serde(flatten, default)]
    pub arguments: serde_json::Map<String, Value>,
}

/// `POST /api/v0/tools/invoke` response — the MCP `CallToolResult` as JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolInvokeResult {
    #[serde(default, rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
    #[serde(default, rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    pub content: Vec<Value>,
    #[serde(default, rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
}

/// `POST /api/v0/prompts/render`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptRenderRequest {
    pub name: CanonicalName,
    #[serde(default)]
    pub arguments: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptRenderResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<Value>,
}

/// `POST /api/v0/resources/get` and `/resources/read`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceRequest {
    pub uri: ResourceUri,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceReadResult {
    pub contents: Vec<Value>,
}
