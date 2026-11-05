//! Persistence ports implemented by `trunks-db`. Object-safe (`async_trait`) so the HTTP
//! layer can hold them as `Arc<dyn …>`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret as _, SecretString};
use serde_json::Value;
use trunks_types::{GroupName, ResourceUri, Role, ServerMode, ServerName, SessionMode, Transport};

use crate::Result;
use crate::access::{Grant, SubjectKind, TargetKind, TeamRole};
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
    async fn user_by_id(&self, id: i64) -> Result<Option<UserRecord>>;
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
        owner: Option<(SubjectKind, i64)>,
    ) -> Result<McpClientRecord>;
    /// Updates only the given fields. Returns `false` when there is no such client.
    async fn update_client(
        &self,
        name: &str,
        description: Option<&str>,
        allow_list: Option<&[String]>,
        token: Option<&str>,
    ) -> Result<bool>;
    /// Clients owned by any of the given subjects.
    async fn clients_owned_by(
        &self,
        subjects: &[(SubjectKind, i64)],
    ) -> Result<Vec<McpClientRecord>>;
    /// Returns `false` when there is no such client.
    async fn delete_client(&self, name: &str) -> Result<bool>;
}

/// Stored OAuth client registration and tokens of one upstream (`upstream_oauth_tokens`).
#[derive(Debug, Clone, Default)]
pub struct OAuthCredentials {
    pub client_id: Option<String>,
    pub client_secret: Option<SecretString>,
    pub redirect_uri: Option<String>,
    pub scopes: Vec<String>,
    pub access_token: Option<SecretString>,
    pub token_type: Option<String>,
    pub refresh_token: Option<SecretString>,
    /// Space-separated granted scope as returned by the token endpoint.
    pub scope: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub issuer: Option<String>,
}

impl OAuthCredentials {
    /// Whether an access token is stored (the connection should use the OAuth client).
    #[must_use]
    pub fn has_tokens(&self) -> bool {
        self.access_token.as_ref().is_some_and(|t| !t.expose_secret().is_empty())
    }
}

/// A pending authorization (`upstream_oauth_pending_sessions`): everything needed to finish
/// `register` after the browser round-trip, across a restart.
#[derive(Debug, Clone)]
pub struct PendingAuthorization {
    pub session_id: String,
    pub server_name: ServerName,
    pub transport: Transport,
    /// The original `RegisterServerInput` as JSON (includes credentials).
    pub server_input: Value,
    pub force: bool,
    pub redirect_uri: String,
    pub client_id: Option<String>,
    pub client_secret: Option<SecretString>,
    pub scopes: Vec<String>,
    /// OAuth `state` (CSRF token), generated by the OAuth library.
    pub state: String,
    pub code_verifier: SecretString,
    pub expires_at: DateTime<Utc>,
    pub initiated_by: Option<String>,
}

#[async_trait]
pub trait OAuthStore: Send + Sync + 'static {
    async fn credentials(&self, server: &ServerName) -> Result<Option<OAuthCredentials>>;
    /// Inserts or replaces the whole credential row.
    async fn save_credentials(&self, server: &ServerName, creds: &OAuthCredentials) -> Result<()>;
    /// Clears only the token fields (client registration stays).
    async fn clear_tokens(&self, server: &ServerName) -> Result<()>;
    /// Removes credentials and pending sessions of the server.
    async fn delete_oauth(&self, server: &ServerName) -> Result<()>;

    async fn pending(&self, session_id: &str) -> Result<Option<PendingAuthorization>>;
    async fn pending_by_state(&self, state: &str) -> Result<Option<PendingAuthorization>>;
    /// Every pending session (expired ones included, until purged).
    async fn pending_all(&self) -> Result<Vec<PendingAuthorization>>;
    /// Replaces any pending session of the same server.
    async fn save_pending(&self, pending: &PendingAuthorization) -> Result<()>;
    async fn delete_pending(&self, session_id: &str) -> Result<()>;
    async fn purge_expired_pending(&self) -> Result<u64>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRecord {
    pub id: i64,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamMemberRecord {
    pub team_id: i64,
    pub user_id: i64,
    pub username: String,
    pub role: TeamRole,
}

/// A grant joined to its subject's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRecord {
    pub subject_kind: SubjectKind,
    pub subject_id: i64,
    pub subject_name: String,
    pub target_kind: TargetKind,
    pub target_name: String,
}

/// Teams, memberships and grants (D14).
#[async_trait]
pub trait AccessStore: Send + Sync + 'static {
    async fn teams(&self) -> Result<Vec<TeamRecord>>;
    async fn team_by_name(&self, name: &str) -> Result<Option<TeamRecord>>;
    async fn team_by_id(&self, id: i64) -> Result<Option<TeamRecord>>;
    async fn create_team(&self, name: &str, description: &str) -> Result<TeamRecord>;
    async fn update_team(&self, name: &str, description: &str) -> Result<bool>;
    /// Deletes the team, its memberships and its grants. Returns `false` when absent.
    async fn delete_team(&self, name: &str) -> Result<bool>;

    async fn members(&self, team_id: i64) -> Result<Vec<TeamMemberRecord>>;
    /// Teams of a user with the user's role in each.
    async fn teams_of_user(&self, user_id: i64) -> Result<Vec<(TeamRecord, TeamRole)>>;
    /// Inserts or updates the membership.
    async fn add_member(&self, team_id: i64, user_id: i64, role: TeamRole) -> Result<()>;
    async fn remove_member(&self, team_id: i64, user_id: i64) -> Result<bool>;

    /// Grants whose subject is the user **or any of the user's teams**.
    async fn effective_grants_for_user(&self, user_id: i64) -> Result<Vec<Grant>>;
    async fn grants_for_subject(&self, kind: SubjectKind, id: i64) -> Result<Vec<Grant>>;
    /// Every grant (optionally only those on one target), joined to subject names.
    async fn grants(&self, target: Option<(TargetKind, &str)>) -> Result<Vec<GrantRecord>>;
    /// Idempotent. Returns `true` when a new row was inserted.
    async fn add_grant(
        &self,
        subject: (SubjectKind, i64),
        target: (TargetKind, &str),
    ) -> Result<bool>;
    /// Idempotent. Returns `true` when a row was removed.
    async fn remove_grant(
        &self,
        subject: (SubjectKind, i64),
        target: (TargetKind, &str),
    ) -> Result<bool>;
}
