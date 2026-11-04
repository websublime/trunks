//! REST API types (`/api/v0`). Field names are the wire contract — stable within 1.x.

use std::collections::BTreeMap;

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

/// `POST /api/v0/servers` body, and the items of `GET /api/v0/server_configs` (admin; **with
/// secrets** — it is the export format).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegisterServerInput {
    pub name: String,
    pub transport: Transport,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub session_mode: SessionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_redirect_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub oauth_scopes: Vec<String>,
}

impl RegisterServerInput {
    /// Whether any upstream OAuth field was supplied.
    #[must_use]
    pub fn has_oauth_fields(&self) -> bool {
        self.oauth_redirect_uri.is_some()
            || self.oauth_client_id.is_some()
            || self.oauth_client_secret.is_some()
            || !self.oauth_scopes.is_empty()
    }
}

/// `POST /api/v0/servers` response: `201` with `server`, or `202` with `authorization_required`
/// when the upstream needs an OAuth flow first.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegisterServerResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<McpServer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_required: Option<AuthorizationRequired>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthorizationRequired {
    pub session_id: String,
    pub authorization_url: String,
    pub expires_at: DateTime<Utc>,
}

/// Query string of `POST /api/v0/servers`. `force` is a strict boolean (`true`/`false`, `1`/`0`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForceQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force: Option<String>,
}

/// Query string of `POST /api/v0/{tools,prompts,resources}/{enable,disable}`: a server name,
/// a canonical name (tools/prompts) or a `trunks://` URI (resources).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
}

/// Response of every enable/disable endpoint. Lists are the entities whose **effective** state
/// changed; for a single entity its name is always listed, even when it was already in that state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnableDisableResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_affected: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompts_affected: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources_affected: Vec<String>,
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
