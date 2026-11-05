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
        error::InvalidInputSnafu { reason: messages::INVALID_TOKEN_FORMAT }
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

    // ---- users

    pub async fn users(&self) -> Result<Vec<UserRecord>> {
        self.store.users().await
    }

    /// Creates a regular user (role `user`). Returns the token once.
    pub async fn create_user(
        &self,
        username: &str,
        custom_token: Option<&str>,
    ) -> Result<(UserRecord, String)> {
        let username = validate_identity_name(username, messages::USERNAME_REQUIRED)?;
        if self.store.user_by_name(username).await?.is_some() {
            return Err(Error::invalid_input(format!("user '{username}' already exists")));
        }
        let token = resolve_token(custom_token)?;
        let user = self.store.create_user(username, Role::User, &token).await?;
        Ok((user, token))
    }

    /// Rotates a user's token (custom or generated). Returns the new token once.
    pub async fn update_user_token(
        &self,
        username: &str,
        custom_token: Option<&str>,
    ) -> Result<(UserRecord, String)> {
        let user = self
            .store
            .user_by_name(username)
            .await?
            .ok_or_else(|| Error::not_found(format!("user '{username}'")))?;
        let token = resolve_token(custom_token)?;
        self.store.update_user_token(username, &token).await?;
        Ok((user, token))
    }

    /// Deletes a user; the admin cannot be deleted. Idempotent for unknown names.
    pub async fn delete_user(&self, username: &str) -> Result<()> {
        if let Some(u) = self.store.user_by_name(username).await?
            && u.role == Role::Admin
        {
            return Err(Error::invalid_input(messages::ADMIN_CANNOT_BE_DELETED));
        }
        self.store.delete_user(username).await?;
        Ok(())
    }

    // ---- MCP clients

    pub async fn clients(&self) -> Result<Vec<McpClientRecord>> {
        self.store.clients().await
    }

    /// Creates an MCP client. Returns the token once.
    pub async fn create_client(
        &self,
        name: &str,
        description: &str,
        allow_list: &[String],
        custom_token: Option<&str>,
    ) -> Result<(McpClientRecord, String)> {
        let name = validate_identity_name(name, messages::NAME_REQUIRED)?;
        validate_allow_list(allow_list)?;
        if self.store.client_by_name(name).await?.is_some() {
            return Err(Error::invalid_input(format!("MCP client '{name}' already exists")));
        }
        let token = resolve_token(custom_token)?;
        let client = self.store.create_client(name, description, &token, allow_list).await?;
        Ok((client, token))
    }

    /// Updates the given fields; `new_token` is `Some` to rotate (custom or generated).
    /// Returns the updated record and the new token, if any.
    pub async fn update_client(
        &self,
        name: &str,
        description: Option<&str>,
        allow_list: Option<&[String]>,
        new_token: Option<Option<&str>>,
    ) -> Result<(McpClientRecord, Option<String>)> {
        if self.store.client_by_name(name).await?.is_none() {
            return Err(Error::not_found(format!("MCP client '{name}'")));
        }
        if let Some(list) = allow_list {
            validate_allow_list(list)?;
        }
        let token = new_token.map(resolve_token).transpose()?;
        self.store.update_client(name, description, allow_list, token.as_deref()).await?;
        let client = self
            .store
            .client_by_name(name)
            .await?
            .ok_or_else(|| Error::not_found(format!("MCP client '{name}'")))?;
        Ok((client, token))
    }

    /// Idempotent.
    pub async fn delete_client(&self, name: &str) -> Result<()> {
        self.store.delete_client(name).await?;
        Ok(())
    }
}

/// Identity names: non-empty, no whitespace, at most 100 characters.
fn validate_identity_name<'a>(name: &'a str, required_msg: &str) -> Result<&'a str> {
    let name = name.trim();
    ensure!(!name.is_empty(), error::InvalidInputSnafu { reason: required_msg });
    ensure!(
        name.chars().count() <= 100 && !name.chars().any(char::is_whitespace),
        error::InvalidInputSnafu {
            reason: "name must be at most 100 characters long and contain no whitespace"
        }
    );
    Ok(name)
}

/// `"*"` or valid server names only.
fn validate_allow_list(list: &[String]) -> Result<()> {
    for entry in list {
        if entry != "*" {
            ServerName::parse(entry.clone()).map_err(|e| {
                Error::invalid_input(format!("invalid allow list entry '{entry}': {e}"))
            })?;
        }
    }
    Ok(())
}

fn resolve_token(custom: Option<&str>) -> Result<String> {
    match custom.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => {
            validate_custom_token(t)?;
            Ok(t.to_owned())
        }
        None => Ok(generate_token()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_names_and_allow_lists() {
        assert_eq!(validate_identity_name("  bob ", "x").unwrap(), "bob");
        assert!(validate_identity_name("", "x").is_err());
        assert!(validate_identity_name("has space", "x").is_err());
        assert!(validate_identity_name(&"a".repeat(101), "x").is_err());
        assert!(validate_allow_list(&["*".into(), "srv-1".into()]).is_ok());
        assert!(validate_allow_list(&["bad__name".into()]).is_err());
        assert!(validate_allow_list(&[]).is_ok());
        assert_eq!(resolve_token(Some("custom-token")).unwrap(), "custom-token");
        assert!(resolve_token(Some("short")).is_err());
        assert_eq!(resolve_token(None).unwrap().len(), 43);
    }

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
