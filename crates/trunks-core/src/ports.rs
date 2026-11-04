//! Persistence ports implemented by `trunks-db`.
//!
//! Only the server-mode port exists in this phase; the registry, identity,
//! group and OAuth ports arrive with their domain modules (see `docs/04-plano.md`).

use std::future::Future;

use trunks_types::ServerMode;

use crate::Result;

/// Persisted operating mode of this installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerConfig {
    pub mode: ServerMode,
    pub initialized: bool,
}

/// Access to the single `server_configs` row.
pub trait ConfigStore: Send + Sync + 'static {
    /// Returns the stored configuration, or `None` when the installation was never initialized.
    fn get(&self) -> impl Future<Output = Result<Option<ServerConfig>>> + Send;

    /// Initializes the installation in `mode`. Returns `false` when it was already initialized
    /// (idempotent, mirrors `MCPJungle`'s `ServerConfigService.Init`).
    fn init(&self, mode: ServerMode) -> impl Future<Output = Result<bool>> + Send;
}
