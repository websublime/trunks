//! Operating mode: persisted once, cached in a watch channel so middleware never hits the DB.

use std::sync::Arc;

use tokio::sync::watch;
use trunks_types::ServerMode;
use trunks_types::error::messages;

use crate::identity::IdentityService;
use crate::ports::{ConfigStore, ServerConfig};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitOutcome {
    pub mode: ServerMode,
    /// Present only for a fresh enterprise initialization.
    pub admin_access_token: Option<String>,
}

#[derive(Clone)]
pub struct ModeService {
    store: Arc<dyn ConfigStore>,
    identity: IdentityService,
    tx: Arc<watch::Sender<Option<ServerConfig>>>,
    rx: watch::Receiver<Option<ServerConfig>>,
}

impl std::fmt::Debug for ModeService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ModeService({:?})", *self.rx.borrow())
    }
}

impl ModeService {
    /// Reads the persisted state once; later changes go through `init`.
    pub async fn load(store: Arc<dyn ConfigStore>, identity: IdentityService) -> Result<Self> {
        let current = store.get().await?;
        let (tx, rx) = watch::channel(current);
        Ok(Self { store, identity, tx: Arc::new(tx), rx })
    }

    /// `Some(mode)` once initialized.
    #[must_use]
    pub fn current(&self) -> Option<ServerMode> {
        self.rx.borrow().filter(|c| c.initialized).map(|c| c.mode)
    }

    /// Initializes the installation. Enterprise mode also creates the admin user and returns
    /// its token exactly once. Already initialized → `InvalidInput("Server is already initialized")`.
    pub async fn init(&self, mode: ServerMode) -> Result<InitOutcome> {
        if self.current().is_some() {
            return Err(Error::InvalidInput {
                reason: messages::SERVER_ALREADY_INITIALIZED.into(),
            });
        }
        let created = self.store.init(mode).await?;
        if !created {
            // raced with another initializer; refresh and report
            self.refresh().await?;
            return Err(Error::InvalidInput {
                reason: messages::SERVER_ALREADY_INITIALIZED.into(),
            });
        }
        let admin_access_token = if mode.is_enterprise() {
            let (_, token) = self.identity.create_admin().await?;
            Some(token)
        } else {
            None
        };
        self.tx.send_replace(Some(ServerConfig { mode, initialized: true }));
        Ok(InitOutcome { mode, admin_access_token })
    }

    pub async fn refresh(&self) -> Result<()> {
        let current = self.store.get().await?;
        self.tx.send_replace(current);
        Ok(())
    }
}
