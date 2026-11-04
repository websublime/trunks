//! Principals, tokens and the identity service.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore as _;
use snafu::ensure;
use trunks_types::error::messages;
use trunks_types::{Role, ServerMode, ServerName};

use crate::domain::{McpClientRecord, UserRecord};
use crate::ports::IdentityStore;
use crate::{Error, Result, error};

/// Who is calling the **MCP** endpoints. Produced by middleware, consumed by the gateway.
#[derive(Debug, Clone)]
pub enum Principal {
    /// Development mode: no authentication, everything is visible.
    DevOpen,
    /// Enterprise mode: an MCP client with its allow-list.
    McpClient(Arc<McpClientRecord>),
}

impl Principal {
    #[must_use]
    pub fn may_access(&self, server: &ServerName) -> bool {
        match self {
            Self::DevOpen => true,
            Self::McpClient(c) => c.may_access(server),
        }
    }

    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::DevOpen => "dev".into(),
            Self::McpClient(c) => c.name.clone(),
        }
    }
}

/// Who is calling the **REST** API.
#[derive(Debug, Clone)]
pub enum UserPrincipal {
    DevOpen,
    User(Arc<UserRecord>),
}

impl UserPrincipal {
    #[must_use]
    pub fn is_admin(&self) -> bool {
        match self {
            Self::DevOpen => true,
            Self::User(u) => u.role == Role::Admin,
        }
    }
}

/// 32 random bytes, base64url without padding (43 chars).
#[must_use]
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// A caller-supplied token: at least 8 characters, no whitespace.
pub fn validate_custom_token(token: &str) -> Result<()> {
    ensure!(
        token.chars().count() >= 8 && !token.chars().any(char::is_whitespace),
        error::InvalidInputSnafu {
            reason: "access token must be at least 8 characters long and contain no whitespace"
        }
    );
    Ok(())
}

#[derive(Clone)]
pub struct IdentityService {
    store: Arc<dyn IdentityStore>,
}

impl std::fmt::Debug for IdentityService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IdentityService")
    }
}

impl IdentityService {
    #[must_use]
    pub fn new(store: Arc<dyn IdentityStore>) -> Self {
        Self { store }
    }

    /// Resolves a REST caller. In development mode everyone is `DevOpen`.
    pub async fn authenticate_user(
        &self,
        mode: ServerMode,
        bearer: Option<&str>,
    ) -> Result<UserPrincipal> {
        if !mode.is_enterprise() {
            return Ok(UserPrincipal::DevOpen);
        }
        let token = bearer
            .filter(|t| !t.is_empty())
            .ok_or_else(|| Error::Unauthorized { reason: messages::MISSING_ACCESS_TOKEN.into() })?;
        match self.store.user_by_token(token).await? {
            Some(u) => Ok(UserPrincipal::User(Arc::new(u))),
            None => Err(Error::Unauthorized {
                reason: format!("{}: no user matches this token", messages::INVALID_ACCESS_TOKEN),
            }),
        }
    }

    /// Resolves an MCP caller. User tokens are **not** accepted here.
    pub async fn authenticate_mcp_client(
        &self,
        mode: ServerMode,
        bearer: Option<&str>,
    ) -> Result<Principal> {
        if !mode.is_enterprise() {
            return Ok(Principal::DevOpen);
        }
        let token = bearer.filter(|t| !t.is_empty()).ok_or_else(|| Error::Unauthorized {
            reason: messages::MISSING_MCP_CLIENT_TOKEN.into(),
        })?;
        match self.store.client_by_token(token).await? {
            Some(c) => Ok(Principal::McpClient(Arc::new(c))),
            None => Err(Error::Unauthorized { reason: messages::INVALID_MCP_CLIENT_TOKEN.into() }),
        }
    }

    /// Creates the initial administrator with a generated token. Returns the token once.
    pub async fn create_admin(&self) -> Result<(UserRecord, String)> {
        let token = generate_token();
        let user = self.store.create_user("admin", Role::Admin, &token).await?;
        Ok((user, token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        let t = generate_token();
        assert_eq!(t.len(), 43);
        assert!(validate_custom_token(&t).is_ok());
        assert!(validate_custom_token("short").is_err());
        assert!(validate_custom_token("has space 123").is_err());
        assert!(validate_custom_token("exactly8").is_ok());
    }
}
