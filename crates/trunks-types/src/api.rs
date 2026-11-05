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

/// `POST /api/v0/upstream_oauth/sessions/{id}/complete` body: the authorization response.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompleteOAuthRequest {
    pub code: String,
    pub state: String,
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

/// A tool group: the body of `POST /api/v0/tool-groups`, of `PUT /tool-groups/{name}`, the items
/// of `GET /tool-groups`, and the `-c` file format of the CLI.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolGroup {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Canonical tool names.
    #[serde(default)]
    pub included_tools: Vec<String>,
    /// Server names: every enabled tool of each server.
    #[serde(default)]
    pub included_servers: Vec<String>,
    /// Canonical tool names removed from the result. Exclusion always wins.
    #[serde(default)]
    pub excluded_tools: Vec<String>,
}

/// `POST /tool-groups` and `GET /tool-groups/{name}`: the group plus its MCP endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolGroupWithEndpoints {
    #[serde(flatten)]
    pub group: ToolGroup,
    /// Path of the group's MCP endpoint (`/v0/groups/{name}/mcp`), relative to the server.
    pub streamable_http_endpoint: String,
}

/// `PUT /tool-groups/{name}` response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateToolGroupResponse {
    pub name: String,
    pub old: ToolGroup,
    pub new: ToolGroup,
}

/// `GET /api/v0/users/whoami` and the items of `GET /api/v0/users`. Never carries a token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    pub username: String,
    pub role: Role,
}

/// `POST /api/v0/users` body (enterprise, admin). Role is always `user`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateUserRequest {
    pub username: String,
    /// Custom token (≥ 8 chars, no whitespace); generated when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
}

/// `PUT /api/v0/users/{username}` body: rotates the token (custom or generated).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateUserRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
}

/// Response of `POST /users` and `PUT /users/{username}`: the only places a user token is
/// ever serialized.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserWithToken {
    pub username: String,
    pub role: Role,
    pub access_token: String,
}

/// Who owns an MCP client (D14): a user or a team, by name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Owner {
    /// `user` or `team`
    pub kind: String,
    pub name: String,
}

/// An MCP client as listed by `GET /api/v0/clients`. Never carries the token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpClient {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Server names this client may reach; `["*"]` means every server, `[]` none. With an
    /// owner it narrows the owner's grants instead (empty = everything the owner can).
    #[serde(default)]
    pub allow_list: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Owner>,
}

/// A team (D14).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Team {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamMember {
    pub username: String,
    /// `maintainer` or `member`
    pub role: String,
}

/// `GET /api/v0/teams/{name}`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamDetail {
    #[serde(flatten)]
    pub team: Team,
    pub members: Vec<TeamMember>,
}

/// `POST /api/v0/teams/{name}/members`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddTeamMemberRequest {
    pub username: String,
    /// `maintainer` or `member` (default)
    #[serde(default)]
    pub role: Option<String>,
}

/// A grant: subject → target → `use` (D14). Body of `POST/DELETE /api/v0/grants`, items of
/// `GET /api/v0/grants` and of `GET /{servers,tool-groups}/{n}/access`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrantSpec {
    /// `user` or `team`
    pub subject_kind: String,
    pub subject: String,
    /// `server` or `tool_group`
    pub target_kind: String,
    /// A server name, a tool group name, or `*` (every server).
    pub target: String,
}

/// Query of `GET /api/v0/grants`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrantsQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// What the caller may use (`GET /api/v0/users/whoami`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessView {
    pub all: bool,
    #[serde(default)]
    pub servers: Vec<String>,
    #[serde(default)]
    pub tool_groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamMembership {
    pub name: String,
    pub role: String,
}

/// `GET /api/v0/users/whoami`: the user plus teams and effective access.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WhoAmI {
    pub username: String,
    pub role: Role,
    #[serde(default)]
    pub teams: Vec<TeamMembership>,
    pub access: AccessView,
}

/// `POST /api/v0/clients` body (enterprise). Admins may set any `owner` or none; a regular
/// user's client is owned by that user unless `owner` names a team they maintain.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateMcpClientRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub allow_list: Vec<String>,
    /// Custom token (≥ 8 chars, no whitespace); generated when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Owner>,
}

/// `PUT /api/v0/clients/{name}` body. Every field is optional; `rotate_token` generates a
/// new token (`access_token` sets a custom one and wins).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateMcpClientRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_list: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default)]
    pub rotate_token: bool,
}

/// Response of `POST /clients` and `PUT /clients/{name}`. `access_token` is present on
/// creation and whenever the token changed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpClientWithToken {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub allow_list: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Owner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
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
