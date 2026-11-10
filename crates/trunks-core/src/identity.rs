//! Principals, tokens and the identity service.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore as _;
use snafu::ensure;
use trunks_types::error::messages;
use trunks_types::{CanonicalName, Role, ServerMode, ServerName};

use crate::access::{AccessSet, SubjectKind, TargetKind, TeamRole};
use crate::domain::{McpClientRecord, UserRecord};
use crate::ports::{AccessStore, GrantRecord, IdentityStore, TeamMemberRecord, TeamRecord};
use crate::{Error, Result, error};

/// Who is calling the **MCP** endpoints. Produced by middleware, consumed by the gateway.
/// The access set is resolved once, at authentication (D14).
#[derive(Debug, Clone)]
pub enum Principal {
    /// Development mode: no authentication, everything is visible.
    DevOpen,
    /// Enterprise mode: an MCP client with its effective access.
    McpClient { client: Arc<McpClientRecord>, access: Arc<AccessSet> },
}

impl Principal {
    #[must_use]
    pub fn may_access(&self, server: &ServerName) -> bool {
        match self {
            Self::DevOpen => true,
            Self::McpClient { access, .. } => access.may_use_server(server),
        }
    }

    /// Server access, or access through a granted tool group that resolves to the tool.
    #[must_use]
    pub fn may_use_tool(&self, snap: &crate::catalog::Snapshot, tool: &CanonicalName) -> bool {
        match self {
            Self::DevOpen => true,
            Self::McpClient { access, .. } => access.may_use_tool(snap, tool),
        }
    }

    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::DevOpen => "dev".into(),
            Self::McpClient { client, .. } => client.name.clone(),
        }
    }
}

/// Who is calling the **REST** API, with what they may see and use.
#[derive(Debug, Clone)]
pub enum UserPrincipal {
    DevOpen,
    User { user: Arc<UserRecord>, access: Arc<AccessSet>, teams: Arc<Vec<(TeamRecord, TeamRole)>> },
}

impl UserPrincipal {
    #[must_use]
    pub fn is_admin(&self) -> bool {
        match self {
            Self::DevOpen => true,
            Self::User { user, .. } => user.role == Role::Admin,
        }
    }

    #[must_use]
    pub fn access(&self) -> AccessSet {
        match self {
            Self::DevOpen => AccessSet::everything(),
            Self::User { access, .. } => (**access).clone(),
        }
    }

    #[must_use]
    pub fn may_use_server(&self, server: &ServerName) -> bool {
        match self {
            Self::DevOpen => true,
            Self::User { access, .. } => access.may_use_server(server),
        }
    }

    #[must_use]
    pub fn may_use_tool(&self, snap: &crate::catalog::Snapshot, tool: &CanonicalName) -> bool {
        match self {
            Self::DevOpen => true,
            Self::User { access, .. } => access.may_use_tool(snap, tool),
        }
    }

    #[must_use]
    pub fn may_use_group(&self, group: &trunks_types::GroupName) -> bool {
        match self {
            Self::DevOpen => true,
            Self::User { access, .. } => access.may_use_group(group),
        }
    }

    /// A server the caller may know about: granted directly, or reached through a granted
    /// group that uses one of its tools.
    #[must_use]
    pub fn may_see_server(&self, snap: &crate::catalog::Snapshot, server: &ServerName) -> bool {
        self.may_use_server(server)
            || snap
                .tools
                .values()
                .any(|t| &t.server == server && self.may_use_tool(snap, &t.canonical))
    }

    /// The server, or the same "not found" an unknown name gets: a server outside the
    /// caller's reach must not be distinguishable from one that does not exist.
    pub fn visible_server<'a>(
        &self,
        snap: &'a crate::catalog::Snapshot,
        server: &ServerName,
    ) -> Result<&'a Arc<crate::domain::ServerRecord>> {
        let record = snap.server(server)?;
        if self.may_see_server(snap, server) {
            Ok(record)
        } else {
            Err(Error::not_found(format!("mcp server '{server}'")))
        }
    }

    /// The user's id, `None` in development mode.
    #[must_use]
    pub fn user_id(&self) -> Option<i64> {
        match self {
            Self::DevOpen => None,
            Self::User { user, .. } => Some(user.id),
        }
    }

    /// Subjects this principal owns clients as: itself and the teams it belongs to.
    #[must_use]
    pub fn subjects(&self) -> Vec<(SubjectKind, i64)> {
        match self {
            Self::DevOpen => vec![],
            Self::User { user, teams, .. } => std::iter::once((SubjectKind::User, user.id))
                .chain(teams.iter().map(|(t, _)| (SubjectKind::Team, t.id)))
                .collect(),
        }
    }

    /// Whether this principal may act for `subject` (itself, or a team it maintains).
    #[must_use]
    pub fn may_act_for(&self, subject: (SubjectKind, i64)) -> bool {
        match self {
            Self::DevOpen => true,
            Self::User { user, teams, .. } => {
                user.role == Role::Admin
                    || subject == (SubjectKind::User, user.id)
                    || teams.iter().any(|(t, r)| {
                        subject == (SubjectKind::Team, t.id) && *r == TeamRole::Maintainer
                    })
            }
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
    access: Arc<dyn AccessStore>,
}

impl std::fmt::Debug for IdentityService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IdentityService")
    }
}

impl IdentityService {
    #[must_use]
    pub fn new(store: Arc<dyn IdentityStore>, access: Arc<dyn AccessStore>) -> Self {
        Self { store, access }
    }

    /// Effective access of a user: everything for admins, else the union of own and team grants.
    pub async fn access_of_user(&self, user: &UserRecord) -> Result<AccessSet> {
        if user.role == Role::Admin {
            return Ok(AccessSet::everything());
        }
        let grants = self.access.effective_grants_for_user(user.id).await?;
        Ok(AccessSet::from_grants(&grants))
    }

    /// Effective access of an MCP client: owner's set narrowed by the allow list, or the
    /// allow list alone for ownerless clients.
    pub async fn access_of_client(&self, client: &McpClientRecord) -> Result<AccessSet> {
        let Some((kind, id)) = client.owner else {
            return Ok(AccessSet::from_allow_list(&client.allow_list));
        };
        let owner = match kind {
            SubjectKind::User => match self.store.user_by_id(id).await? {
                Some(u) => self.access_of_user(&u).await?,
                None => AccessSet::nothing(),
            },
            SubjectKind::Team => AccessSet::from_grants(
                &self.access.grants_for_subject(SubjectKind::Team, id).await?,
            ),
        };
        Ok(owner.narrowed_by(&client.allow_list))
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
            Some(u) => self.principal_of(u).await,
            None => Err(Error::Unauthorized {
                reason: format!("{}: no user matches this token", messages::INVALID_ACCESS_TOKEN),
            }),
        }
    }

    /// The REST principal of an already authenticated user (token or session), with the
    /// access set and team memberships resolved now.
    pub async fn principal_of(&self, user: UserRecord) -> Result<UserPrincipal> {
        let access = self.access_of_user(&user).await?;
        let teams = self.access.teams_of_user(user.id).await?;
        Ok(UserPrincipal::User {
            user: Arc::new(user),
            access: Arc::new(access),
            teams: Arc::new(teams),
        })
    }

    pub async fn user_by_name(&self, username: &str) -> Result<Option<UserRecord>> {
        self.store.user_by_name(username).await
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
            Some(c) => {
                let access = self.access_of_client(&c).await?;
                Ok(Principal::McpClient { client: Arc::new(c), access: Arc::new(access) })
            }
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

    /// Deletes a user. The **last** administrator cannot be deleted (no lock-out). Idempotent
    /// for unknown names.
    pub async fn delete_user(&self, username: &str) -> Result<()> {
        if let Some(u) = self.store.user_by_name(username).await?
            && u.role == Role::Admin
        {
            let admins = self.store.users().await?.iter().filter(|x| x.role == Role::Admin).count();
            if admins <= 1 {
                return Err(Error::invalid_input(messages::ADMIN_CANNOT_BE_DELETED));
            }
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
        owner: Option<(SubjectKind, i64)>,
    ) -> Result<(McpClientRecord, String)> {
        let name = validate_identity_name(name, messages::NAME_REQUIRED)?;
        validate_allow_list(allow_list)?;
        if self.store.client_by_name(name).await?.is_some() {
            return Err(Error::invalid_input(format!("MCP client '{name}' already exists")));
        }
        let token = resolve_token(custom_token)?;
        let client = self.store.create_client(name, description, &token, allow_list, owner).await?;
        Ok((client, token))
    }

    pub async fn client_by_name(&self, name: &str) -> Result<Option<McpClientRecord>> {
        self.store.client_by_name(name).await
    }

    pub async fn clients_owned_by(
        &self,
        subjects: &[(SubjectKind, i64)],
    ) -> Result<Vec<McpClientRecord>> {
        self.store.clients_owned_by(subjects).await
    }

    /// Resolves a subject given by kind and name to its id.
    pub async fn subject_id(&self, kind: SubjectKind, name: &str) -> Result<i64> {
        match kind {
            SubjectKind::User => self
                .store
                .user_by_name(name)
                .await?
                .map(|u| u.id)
                .ok_or_else(|| Error::not_found(format!("user '{name}'"))),
            SubjectKind::Team => self
                .access
                .team_by_name(name)
                .await?
                .map(|t| t.id)
                .ok_or_else(|| Error::not_found(format!("team '{name}'"))),
        }
    }

    /// Name of a subject by id (for API views). `None` if it no longer exists.
    pub async fn subject_name(&self, subject: (SubjectKind, i64)) -> Result<Option<String>> {
        Ok(match subject.0 {
            SubjectKind::User => self.store.user_by_id(subject.1).await?.map(|u| u.username),
            SubjectKind::Team => self.access.team_by_id(subject.1).await?.map(|t| t.name),
        })
    }

    // ---- teams

    pub async fn teams(&self) -> Result<Vec<TeamRecord>> {
        self.access.teams().await
    }

    pub async fn team_by_name(&self, name: &str) -> Result<TeamRecord> {
        self.access
            .team_by_name(name)
            .await?
            .ok_or_else(|| Error::not_found(format!("team '{name}'")))
    }

    pub async fn create_team(&self, name: &str, description: &str) -> Result<TeamRecord> {
        let name = validate_identity_name(name, messages::NAME_REQUIRED)?;
        if self.access.team_by_name(name).await?.is_some() {
            return Err(Error::invalid_input(format!("team '{name}' already exists")));
        }
        self.access.create_team(name, description.trim()).await
    }

    pub async fn update_team(&self, name: &str, description: &str) -> Result<TeamRecord> {
        if !self.access.update_team(name, description.trim()).await? {
            return Err(Error::not_found(format!("team '{name}'")));
        }
        self.team_by_name(name).await
    }

    /// Idempotent.
    pub async fn delete_team(&self, name: &str) -> Result<()> {
        self.access.delete_team(name).await?;
        Ok(())
    }

    pub async fn members(&self, team: &TeamRecord) -> Result<Vec<TeamMemberRecord>> {
        self.access.members(team.id).await
    }

    pub async fn add_member(
        &self,
        team: &TeamRecord,
        username: &str,
        role: TeamRole,
    ) -> Result<()> {
        let user = self
            .store
            .user_by_name(username)
            .await?
            .ok_or_else(|| Error::not_found(format!("user '{username}'")))?;
        self.access.add_member(team.id, user.id, role).await
    }

    /// Idempotent.
    pub async fn remove_member(&self, team: &TeamRecord, username: &str) -> Result<()> {
        if let Some(user) = self.store.user_by_name(username).await? {
            self.access.remove_member(team.id, user.id).await?;
        }
        Ok(())
    }

    // ---- grants

    pub async fn grants(&self, target: Option<(TargetKind, &str)>) -> Result<Vec<GrantRecord>> {
        self.access.grants(target).await
    }

    /// Validates the target name (`*` only for servers) and records the grant. Idempotent.
    pub async fn grant(
        &self,
        subject: (SubjectKind, &str),
        target: (TargetKind, &str),
    ) -> Result<bool> {
        let id = self.subject_id(subject.0, subject.1).await?;
        let target_name = validate_target(target)?;
        self.access.add_grant((subject.0, id), (target.0, &target_name)).await
    }

    /// Idempotent.
    pub async fn revoke(
        &self,
        subject: (SubjectKind, &str),
        target: (TargetKind, &str),
    ) -> Result<bool> {
        let id = self.subject_id(subject.0, subject.1).await?;
        let target_name = validate_target(target)?;
        self.access.remove_grant((subject.0, id), (target.0, &target_name)).await
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

fn validate_target((kind, name): (TargetKind, &str)) -> Result<String> {
    let name = name.trim();
    match kind {
        TargetKind::Server if name == "*" => Ok(name.to_owned()),
        TargetKind::Server => Ok(ServerName::parse(name.to_owned())?.to_string()),
        TargetKind::ToolGroup => Ok(trunks_types::GroupName::parse(name)?.to_string()),
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
