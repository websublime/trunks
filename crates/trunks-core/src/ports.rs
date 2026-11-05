//! Persistence ports implemented by `trunks-db`. Object-safe (`async_trait`) so the HTTP
//! layer can hold them as `Arc<dyn …>`.

use async_trait::async_trait;
use serde_json::Value;
use trunks_types::{GroupName, ResourceUri, Role, ServerMode, ServerName, SessionMode};

use crate::Result;
use crate::domain::{
    McpClientRecord, PromptRecord, ResourceRecord, ServerRecord, ToolGroupRecord, ToolRecord,
    UpstreamConfig, UserRecord,
};

/// Persisted operating mode of this installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerConfig {
    pub mode: ServerMode,
    pub initialized: bool,
}

/// Access to the single `server_configs` row.
#[async_trait]
pub trait ConfigStore: Send + Sync + 'static {
    /// Returns the stored configuration, or `None` when the installation was never initialized.
    async fn get(&self) -> Result<Option<ServerConfig>>;

    /// Initializes the installation in `mode`. Returns `false` when it was already initialized
    /// (idempotent).
    async fn init(&self, mode: ServerMode) -> Result<bool>;
}

/// Everything the catalog is built from.
#[derive(Debug, Clone, Default)]
pub struct RegistrySnapshot {
    pub servers: Vec<ServerRecord>,
    pub tools: Vec<ToolRecord>,
    pub prompts: Vec<PromptRecord>,
    pub resources: Vec<ResourceRecord>,
    pub groups: Vec<ToolGroupRecord>,
}

/// A server to persist (validated, connection already verified).
#[derive(Debug, Clone)]
pub struct NewServer {
    pub name: ServerName,
    pub description: String,
    pub config: UpstreamConfig,
    pub session_mode: SessionMode,
}

#[derive(Debug, Clone)]
pub struct NewTool {
    pub name: String,
    pub description: String,
    pub input_schema: Option<Value>,
    pub annotations: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct NewPrompt {
    pub name: String,
    pub description: String,
    pub arguments: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct NewResource {
    pub uri: ResourceUri,
    pub original_uri: String,
    pub name: String,
    pub description: String,
    pub mime_type: Option<String>,
    pub annotations: Option<Value>,
    pub meta: Option<Value>,
}

/// What `register` discovered on the upstream, persisted together with the server.
#[derive(Debug, Clone, Default)]
pub struct Discovered {
    pub tools: Vec<NewTool>,
    pub prompts: Vec<NewPrompt>,
    pub resources: Vec<NewResource>,
}

/// Registry persistence. Every write runs in one transaction and never touches the catalog:
/// the service rebuilds it afterwards.
#[async_trait]
pub trait RegistryStore: Send + Sync + 'static {
    async fn load_snapshot(&self) -> Result<RegistrySnapshot>;

    /// Inserts the server and its entities. With `replace`, an existing server of the same name
    /// (and its entities) is deleted in the same transaction first; without it, a duplicate name
    /// is an `InvalidInput` error.
    async fn create_server(
        &self,
        server: &NewServer,
        discovered: &Discovered,
        replace: bool,
    ) -> Result<ServerRecord>;

    /// Deletes the server and its entities. Returns `false` when there was no such server.
    async fn delete_server(&self, name: &ServerName) -> Result<bool>;

    /// Returns `None` when the server does not exist, otherwise whether the flag changed.
    async fn set_server_enabled(&self, name: &ServerName, enabled: bool) -> Result<Option<bool>>;

    /// Flips the tools of `server_id` (all of them, or only `name`) that are not already in
    /// that state. Returns the bare names that changed.
    async fn set_tools_enabled(
        &self,
        server_id: i64,
        name: Option<&str>,
        enabled: bool,
    ) -> Result<Vec<String>>;

    async fn set_prompts_enabled(
        &self,
        server_id: i64,
        name: Option<&str>,
        enabled: bool,
    ) -> Result<Vec<String>>;

    /// Same for resources, selected by public `trunks://` URI. Returns the URIs that changed.
    async fn set_resources_enabled(
        &self,
        server_id: i64,
        uri: Option<&str>,
        enabled: bool,
    ) -> Result<Vec<String>>;

    /// Inserts a tool group (validated). A duplicate name is an `InvalidInput` error.
    async fn create_group(&self, spec: &ToolGroupSpec) -> Result<ToolGroupRecord>;
    /// Replaces every field of the group named `spec.name`, empty lists included. Returns
    /// `false` when there is no such group.
    async fn update_group(&self, spec: &ToolGroupSpec) -> Result<bool>;
    /// Returns `false` when there was no such group.
    async fn delete_group(&self, name: &GroupName) -> Result<bool>;
}

/// A tool group as written (validated against the catalog by `groups::GroupService`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolGroupSpec {
    pub name: GroupName,
    pub description: String,
    pub included_tools: Vec<String>,
    pub included_servers: Vec<String>,
    pub excluded_tools: Vec<String>,
}

/// Users and MCP clients. Tokens are compared exact-match (plaintext in v1, see §4.9).
#[async_trait]
pub trait IdentityStore: Send + Sync + 'static {
    async fn user_by_token(&self, token: &str) -> Result<Option<UserRecord>>;
    async fn client_by_token(&self, token: &str) -> Result<Option<McpClientRecord>>;
    /// Creates a user with the given (already validated) token. Fails on duplicate username.
    async fn create_user(&self, username: &str, role: Role, token: &str) -> Result<UserRecord>;

    async fn users(&self) -> Result<Vec<UserRecord>>;
    async fn user_by_name(&self, username: &str) -> Result<Option<UserRecord>>;
    /// Returns `false` when there is no such user.
    async fn update_user_token(&self, username: &str, token: &str) -> Result<bool>;
    /// Returns `false` when there is no such user.
    async fn delete_user(&self, username: &str) -> Result<bool>;

    async fn clients(&self) -> Result<Vec<McpClientRecord>>;
    async fn client_by_name(&self, name: &str) -> Result<Option<McpClientRecord>>;
    async fn create_client(
        &self,
        name: &str,
        description: &str,
        token: &str,
        allow_list: &[String],
    ) -> Result<McpClientRecord>;
    /// Updates only the given fields. Returns `false` when there is no such client.
    async fn update_client(
        &self,
        name: &str,
        description: Option<&str>,
        allow_list: Option<&[String]>,
        token: Option<&str>,
    ) -> Result<bool>;
    /// Returns `false` when there is no such client.
    async fn delete_client(&self, name: &str) -> Result<bool>;
}
