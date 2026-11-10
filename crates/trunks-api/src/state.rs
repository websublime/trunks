//! Shared application state handed to every handler.

use std::sync::Arc;

use trunks_core::catalog::Catalog;
use trunks_core::dashboard::RuntimeInfo;
use trunks_core::gateway::GatewayState;
use trunks_core::groups::GroupService;
use trunks_core::identity::IdentityService;
use trunks_core::login::LoginService;
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
    pub groups: GroupService,
    pub identity: IdentityService,
    /// Browser logins and sessions (OIDC and token → cookie).
    pub login: LoginService,
    /// Public base URL (`TRUNKS_PUBLIC_URL`): used for the `Secure` cookie flag and the
    /// same-origin check. `None` = derive from the request `Host`.
    pub public_url: Option<String>,
    pub mode: ModeService,
    /// `None` when metrics are disabled (no `/metrics` route).
    pub metrics: Option<Metrics>,
    /// Facts about the running process for the dashboard.
    pub runtime: RuntimeInfo,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").field("mode", &self.mode).finish_non_exhaustive()
    }
}
