//! Shared application state handed to every handler.

use std::sync::Arc;

use trunks_core::catalog::Catalog;
use trunks_core::gateway::GatewayState;
use trunks_core::identity::IdentityService;
use trunks_core::metrics::Metrics;
use trunks_core::mode::ModeService;
use trunks_core::ports::RegistryStore;
use trunks_core::registry::RegistryService;

#[derive(Clone)]
pub struct AppState {
    pub catalog: Catalog,
    pub gateway: Arc<GatewayState>,
    pub mcp: crate::mcp::McpMounts,
    pub registry: Arc<dyn RegistryStore>,
    pub registry_service: RegistryService,
    pub identity: IdentityService,
    pub mode: ModeService,
    /// `None` when metrics are disabled (no `/metrics` route).
    pub metrics: Option<Metrics>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").field("mode", &self.mode).finish_non_exhaustive()
    }
}
