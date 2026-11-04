//! Persistence ports implemented by `trunks-db`. Object-safe (`async_trait`) so the HTTP
//! layer can hold them as `Arc<dyn …>`.

use async_trait::async_trait;
use trunks_types::{Role, ServerMode};

use crate::Result;
use crate::domain::{
    McpClientRecord, PromptRecord, ResourceRecord, ServerRecord, ToolGroupRecord, ToolRecord,
    UserRecord,
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

/// Read side of the registry.
#[async_trait]
pub trait RegistryStore: Send + Sync + 'static {
    async fn load_snapshot(&self) -> Result<RegistrySnapshot>;
}

/// Users and MCP clients.
#[async_trait]
pub trait IdentityStore: Send + Sync + 'static {
    async fn user_by_token(&self, token: &str) -> Result<Option<UserRecord>>;
    async fn client_by_token(&self, token: &str) -> Result<Option<McpClientRecord>>;
    /// Creates a user with the given (already validated) token. Fails on duplicate username.
    async fn create_user(&self, username: &str, role: Role, token: &str) -> Result<UserRecord>;
}
