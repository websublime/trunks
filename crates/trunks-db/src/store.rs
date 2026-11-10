//! `Store`: the `trunks-core` ports implemented over `Db`.

use async_trait::async_trait;
use secrecy::{ExposeSecret as _, SecretString};
use trunks_core::access::{Grant, SubjectKind, TargetKind, TeamRole};
use trunks_core::domain::{
    McpClientRecord, PromptRecord, ResourceRecord, ServerRecord, ToolGroupRecord, ToolRecord,
    UpstreamConfig, UserRecord,
};
use trunks_core::ports::{
    AccessStore, ConfigStore, Discovered, GrantRecord, IdentityStore, LoginState, NewServer,
    OAuthCredentials, OAuthStore, PendingAuthorization, RegistrySnapshot, RegistryStore,
    ServerConfig, SessionStore, TeamMemberRecord, TeamRecord, ToolGroupSpec,
};
use trunks_types::{GroupName, ResourceUri, Role, ServerMode, ServerName, SessionMode, Transport};

use crate::repos::oauth::{self, OAuthPendingWrite, OAuthTokenWrite};
use crate::repos::registry::{NewPromptRow, NewResourceRow, NewServerRow, NewToolRow};
use crate::repos::{access, catalog, config, groups, identity, registry, sessions};
use crate::rows::{
    BareGrantRow, GrantRow, McpClientRow, OAuthPendingRow, OAuthTokenRow, PromptRow, ResourceRow,
    ServerRow, TeamMemberRow, TeamRow, ToolGroupRow, ToolRow, UserRow,
};
use crate::{Db, Result};

#[derive(Debug, Clone)]
pub struct Store(pub Db);

fn internal(reason: String) -> trunks_core::Error {
    trunks_core::Error::Internal { reason }
}

fn string_list(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

impl TryFrom<ServerRow> for ServerRecord {
    type Error = trunks_core::Error;
    fn try_from(r: ServerRow) -> trunks_core::Result<Self> {
        let transport: Transport = serde_json::from_value(serde_json::Value::String(
            r.transport.clone(),
        ))
        .map_err(|_| {
            internal(format!("unknown transport '{}' for server '{}'", r.transport, r.name))
        })?;
        let session_mode = match r.session_mode.as_deref() {
            None | Some("" | "stateless") => SessionMode::Stateless,
            Some("stateful") => SessionMode::Stateful,
            Some(other) => {
                return Err(internal(format!(
                    "unknown session_mode '{other}' for server '{}'",
                    r.name
                )));
            }
        };
        Ok(Self {
            id: r.id,
            name: ServerName::parse(r.name)?,
            enabled: r.enabled,
            description: r.description.unwrap_or_default(),
            config: UpstreamConfig::from_json(transport, &r.config.0)?,
            session_mode,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

impl From<ToolRow> for ToolRecord {
    fn from(r: ToolRow) -> Self {
        Self {
            id: r.id,
            server_id: r.server_id,
            name: r.name,
            enabled: r.enabled,
            description: r.description.unwrap_or_default(),
            input_schema: r.input_schema.map(|j| j.0),
            annotations: r.annotations.map(|j| j.0),
        }
    }
}

impl From<PromptRow> for PromptRecord {
    fn from(r: PromptRow) -> Self {
        Self {
            id: r.id,
            server_id: r.server_id,
            name: r.name,
            enabled: r.enabled,
            description: r.description.unwrap_or_default(),
            arguments: r.arguments.map(|j| j.0),
        }
    }
}

impl TryFrom<ResourceRow> for ResourceRecord {
    type Error = trunks_core::Error;
    fn try_from(r: ResourceRow) -> trunks_core::Result<Self> {
        Ok(Self {
            id: r.id,
            server_id: r.server_id,
            uri: ResourceUri::parse(&r.uri)?,
            original_uri: r.original_uri,
            name: r.name,
            enabled: r.enabled,
            description: r.description.unwrap_or_default(),
            mime_type: r.mime_type,
            annotations: r.annotations.map(|j| j.0),
            meta: r.meta.map(|j| j.0),
        })
    }
}

impl From<ToolGroupRow> for ToolGroupRecord {
    fn from(r: ToolGroupRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            description: r.description.unwrap_or_default(),
            included_tools: string_list(r.included_tools.as_ref().map(|j| &j.0)),
            included_servers: string_list(r.included_servers.as_ref().map(|j| &j.0)),
            excluded_tools: string_list(r.excluded_tools.as_ref().map(|j| &j.0)),
        }
    }
}

impl TryFrom<UserRow> for UserRecord {
    type Error = trunks_core::Error;
    fn try_from(r: UserRow) -> trunks_core::Result<Self> {
        let role = match r.role.as_str() {
            "admin" => Role::Admin,
            "user" => Role::User,
            other => {
                return Err(internal(format!("unknown role '{other}' for user '{}'", r.username)));
            }
        };
        Ok(Self { id: r.id, username: r.username, role })
    }
}

impl From<McpClientRow> for McpClientRecord {
    fn from(r: McpClientRow) -> Self {
        let owner = match (r.owner_kind.as_deref().and_then(SubjectKind::parse), r.owner_id) {
            (Some(k), Some(id)) => Some((k, id)),
            _ => None,
        };
        Self {
            id: r.id,
            name: r.name,
            description: r.description.unwrap_or_default(),
            allow_list: string_list(Some(&r.allow_list.0)),
            owner,
        }
    }
}

impl From<TeamRow> for TeamRecord {
    fn from(r: TeamRow) -> Self {
        Self { id: r.id, name: r.name, description: r.description.unwrap_or_default() }
    }
}

fn team_role(s: &str) -> TeamRole {
    TeamRole::parse(s).unwrap_or_default()
}

fn grant_from(r: &BareGrantRow) -> Option<Grant> {
    Some(Grant {
        target_kind: TargetKind::parse(&r.target_kind)?,
        target_name: r.target_name.clone(),
    })
}

#[async_trait]
impl ConfigStore for Store {
    async fn get(&self) -> trunks_core::Result<Option<ServerConfig>> {
        let Some(row) = config::get(&self.0).await? else { return Ok(None) };
        let mode = ServerMode::parse_lenient(&row.mode)
            .ok_or_else(|| internal(format!("unknown server mode '{}' in database", row.mode)))?;
        Ok(Some(ServerConfig { mode, initialized: row.initialized }))
    }

    async fn init(&self, mode: ServerMode) -> trunks_core::Result<bool> {
        Ok(config::init(&self.0, mode).await?)
    }
}

#[async_trait]
impl RegistryStore for Store {
    async fn load_snapshot(&self) -> trunks_core::Result<RegistrySnapshot> {
        let db = &self.0;
        let servers = catalog::servers(db)
            .await?
            .into_iter()
            .map(ServerRecord::try_from)
            .collect::<trunks_core::Result<Vec<_>>>()?;
        let tools = catalog::tools(db).await?.into_iter().map(ToolRecord::from).collect();
        let prompts = catalog::prompts(db).await?.into_iter().map(PromptRecord::from).collect();
        let resources = catalog::resources(db)
            .await?
            .into_iter()
            .map(ResourceRecord::try_from)
            .collect::<trunks_core::Result<Vec<_>>>()?;
        let groups =
            catalog::tool_groups(db).await?.into_iter().map(ToolGroupRecord::from).collect();
        Ok(RegistrySnapshot { servers, tools, prompts, resources, groups })
    }

    async fn create_server(
        &self,
        server: &NewServer,
        discovered: &Discovered,
        replace: bool,
    ) -> trunks_core::Result<ServerRecord> {
        let row = NewServerRow {
            name: server.name.to_string(),
            transport: server.config.transport().as_str().to_owned(),
            description: server.description.clone(),
            config: server.config.to_json(),
            session_mode: server.session_mode.as_str().to_owned(),
        };
        let tools: Vec<NewToolRow> = discovered
            .tools
            .iter()
            .map(|t| NewToolRow {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
                annotations: t.annotations.clone(),
            })
            .collect();
        let prompts: Vec<NewPromptRow> = discovered
            .prompts
            .iter()
            .map(|p| NewPromptRow {
                name: p.name.clone(),
                description: p.description.clone(),
                arguments: p.arguments.clone(),
            })
            .collect();
        let resources: Vec<NewResourceRow> = discovered
            .resources
            .iter()
            .map(|r| NewResourceRow {
                uri: r.uri.to_string(),
                original_uri: r.original_uri.clone(),
                name: r.name.clone(),
                description: r.description.clone(),
                mime_type: r.mime_type.clone(),
                annotations: r.annotations.clone(),
                meta: r.meta.clone(),
            })
            .collect();
        let inserted =
            registry::create_server(&self.0, &row, &tools, &prompts, &resources, replace).await?;
        ServerRecord::try_from(inserted)
    }

    async fn delete_server(&self, name: &ServerName) -> trunks_core::Result<bool> {
        Ok(registry::delete_server(&self.0, name.as_str()).await?)
    }

    async fn set_server_enabled(
        &self,
        name: &ServerName,
        enabled: bool,
    ) -> trunks_core::Result<Option<bool>> {
        Ok(registry::set_server_enabled(&self.0, name.as_str(), enabled).await?)
    }

    async fn set_tools_enabled(
        &self,
        server_id: i64,
        name: Option<&str>,
        enabled: bool,
    ) -> trunks_core::Result<Vec<String>> {
        Ok(registry::set_tools_enabled(&self.0, server_id, name, enabled).await?)
    }

    async fn set_prompts_enabled(
        &self,
        server_id: i64,
        name: Option<&str>,
        enabled: bool,
    ) -> trunks_core::Result<Vec<String>> {
        Ok(registry::set_prompts_enabled(&self.0, server_id, name, enabled).await?)
    }

    async fn set_resources_enabled(
        &self,
        server_id: i64,
        uri: Option<&str>,
        enabled: bool,
    ) -> trunks_core::Result<Vec<String>> {
        Ok(registry::set_resources_enabled(&self.0, server_id, uri, enabled).await?)
    }

    async fn create_group(&self, spec: &ToolGroupSpec) -> trunks_core::Result<ToolGroupRecord> {
        Ok(groups::create(
            &self.0,
            spec.name.as_str(),
            &spec.description,
            &spec.included_tools,
            &spec.included_servers,
            &spec.excluded_tools,
        )
        .await
        .map(ToolGroupRecord::from)?)
    }

    async fn update_group(&self, spec: &ToolGroupSpec) -> trunks_core::Result<bool> {
        Ok(groups::update(
            &self.0,
            spec.name.as_str(),
            &spec.description,
            &spec.included_tools,
            &spec.included_servers,
            &spec.excluded_tools,
        )
        .await?)
    }

    async fn delete_group(&self, name: &GroupName) -> trunks_core::Result<bool> {
        Ok(groups::delete(&self.0, name.as_str()).await?)
    }
}

#[async_trait]
impl IdentityStore for Store {
    async fn user_by_token(&self, token: &str) -> trunks_core::Result<Option<UserRecord>> {
        identity::user_by_token(&self.0, token).await?.map(UserRecord::try_from).transpose()
    }

    async fn client_by_token(&self, token: &str) -> trunks_core::Result<Option<McpClientRecord>> {
        Ok(identity::client_by_token(&self.0, token).await?.map(McpClientRecord::from))
    }

    async fn create_user(
        &self,
        username: &str,
        role: Role,
        token: &str,
    ) -> trunks_core::Result<UserRecord> {
        let row = identity::create_user(&self.0, username, role.as_str(), token).await?;
        UserRecord::try_from(row)
    }

    async fn users(&self) -> trunks_core::Result<Vec<UserRecord>> {
        identity::users(&self.0).await?.into_iter().map(UserRecord::try_from).collect()
    }

    async fn user_by_name(&self, username: &str) -> trunks_core::Result<Option<UserRecord>> {
        identity::user_by_name(&self.0, username).await?.map(UserRecord::try_from).transpose()
    }

    async fn update_user_token(&self, username: &str, token: &str) -> trunks_core::Result<bool> {
        Ok(identity::update_user_token(&self.0, username, token).await?)
    }

    async fn delete_user(&self, username: &str) -> trunks_core::Result<bool> {
        Ok(identity::delete_user(&self.0, username).await?)
    }

    async fn clients(&self) -> trunks_core::Result<Vec<McpClientRecord>> {
        Ok(identity::clients(&self.0).await?.into_iter().map(McpClientRecord::from).collect())
    }

    async fn client_by_name(&self, name: &str) -> trunks_core::Result<Option<McpClientRecord>> {
        Ok(identity::client_by_name(&self.0, name).await?.map(McpClientRecord::from))
    }

    async fn create_client(
        &self,
        name: &str,
        description: &str,
        token: &str,
        allow_list: &[String],
        owner: Option<(SubjectKind, i64)>,
    ) -> trunks_core::Result<McpClientRecord> {
        let owner = owner.map(|(k, id)| (k.as_str(), id));
        Ok(identity::create_client(&self.0, name, description, token, allow_list, owner)
            .await
            .map(McpClientRecord::from)?)
    }

    async fn clients_owned_by(
        &self,
        subjects: &[(SubjectKind, i64)],
    ) -> trunks_core::Result<Vec<McpClientRecord>> {
        let mut out = Vec::new();
        for (k, id) in subjects {
            out.extend(
                identity::clients_owned_by(&self.0, k.as_str(), *id)
                    .await?
                    .into_iter()
                    .map(McpClientRecord::from),
            );
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out.dedup_by(|a, b| a.id == b.id);
        Ok(out)
    }

    async fn user_by_id(&self, id: i64) -> trunks_core::Result<Option<UserRecord>> {
        identity::user_by_id(&self.0, id).await?.map(UserRecord::try_from).transpose()
    }

    async fn update_client(
        &self,
        name: &str,
        description: Option<&str>,
        allow_list: Option<&[String]>,
        token: Option<&str>,
    ) -> trunks_core::Result<bool> {
        Ok(identity::update_client(&self.0, name, description, allow_list, token).await?)
    }

    async fn delete_client(&self, name: &str) -> trunks_core::Result<bool> {
        Ok(identity::delete_client(&self.0, name).await?)
    }
}

/// Convenience: a `Result<T>` from this crate mapped into the domain error.
#[allow(dead_code)]
fn lift<T>(r: Result<T>) -> trunks_core::Result<T> {
    r.map_err(Into::into)
}

fn secret(v: Option<String>) -> Option<SecretString> {
    v.filter(|s| !s.is_empty()).map(SecretString::from)
}

fn expose(v: Option<&SecretString>) -> Option<String> {
    v.map(|s| s.expose_secret().to_owned())
}

impl From<OAuthTokenRow> for OAuthCredentials {
    fn from(r: OAuthTokenRow) -> Self {
        Self {
            client_id: r.client_id.filter(|s| !s.is_empty()),
            client_secret: secret(r.client_secret),
            redirect_uri: r.redirect_uri,
            scopes: string_list(r.scopes.as_ref().map(|j| &j.0)),
            access_token: secret(r.access_token),
            token_type: r.token_type,
            refresh_token: secret(r.refresh_token),
            scope: r.scope.filter(|s| !s.is_empty()),
            expires_at: r.expires_at,
            issuer: r.issuer,
        }
    }
}

impl TryFrom<OAuthPendingRow> for PendingAuthorization {
    type Error = trunks_core::Error;
    fn try_from(r: OAuthPendingRow) -> trunks_core::Result<Self> {
        let transport: Transport = serde_json::from_value(serde_json::Value::String(
            r.transport.clone(),
        ))
        .map_err(|_| internal(format!("unknown transport '{}' in pending session", r.transport)))?;
        Ok(Self {
            session_id: r.session_id,
            server_name: ServerName::parse(r.server_name)?,
            transport,
            server_input: r.server_input.0,
            force: r.force,
            redirect_uri: r.redirect_uri.unwrap_or_default(),
            client_id: r.client_id.filter(|s| !s.is_empty()),
            client_secret: secret(r.client_secret),
            scopes: string_list(r.scopes.as_ref().map(|j| &j.0)),
            state: r.state,
            code_verifier: SecretString::from(r.code_verifier),
            expires_at: r.expires_at,
            initiated_by: r.initiated_by,
        })
    }
}

#[async_trait]
impl OAuthStore for Store {
    async fn credentials(
        &self,
        server: &ServerName,
    ) -> trunks_core::Result<Option<OAuthCredentials>> {
        Ok(oauth::token_by_server(&self.0, server.as_str()).await?.map(OAuthCredentials::from))
    }

    async fn save_credentials(
        &self,
        server: &ServerName,
        c: &OAuthCredentials,
    ) -> trunks_core::Result<()> {
        let w = OAuthTokenWrite {
            client_id: c.client_id.clone(),
            client_secret: expose(c.client_secret.as_ref()),
            redirect_uri: c.redirect_uri.clone(),
            scopes: c.scopes.clone(),
            access_token: expose(c.access_token.as_ref()),
            token_type: c.token_type.clone(),
            refresh_token: expose(c.refresh_token.as_ref()),
            scope: c.scope.clone(),
            expires_at: c.expires_at,
            issuer: c.issuer.clone(),
        };
        Ok(oauth::upsert_token(&self.0, server.as_str(), Transport::StreamableHttp.as_str(), &w)
            .await?)
    }

    async fn clear_tokens(&self, server: &ServerName) -> trunks_core::Result<()> {
        Ok(oauth::clear_token_fields(&self.0, server.as_str()).await?)
    }

    async fn delete_oauth(&self, server: &ServerName) -> trunks_core::Result<()> {
        oauth::delete_token(&self.0, server.as_str()).await?;
        Ok(oauth::delete_pending_by_server(&self.0, server.as_str()).await?)
    }

    async fn pending(&self, session_id: &str) -> trunks_core::Result<Option<PendingAuthorization>> {
        oauth::pending_by_session(&self.0, session_id)
            .await?
            .map(PendingAuthorization::try_from)
            .transpose()
    }

    async fn pending_by_state(
        &self,
        state: &str,
    ) -> trunks_core::Result<Option<PendingAuthorization>> {
        oauth::pending_by_state(&self.0, state)
            .await?
            .map(PendingAuthorization::try_from)
            .transpose()
    }

    async fn pending_all(&self) -> trunks_core::Result<Vec<PendingAuthorization>> {
        oauth::pending_sessions(&self.0)
            .await?
            .into_iter()
            .map(PendingAuthorization::try_from)
            .collect()
    }

    async fn save_pending(&self, p: &PendingAuthorization) -> trunks_core::Result<()> {
        let w = OAuthPendingWrite {
            session_id: p.session_id.clone(),
            server_name: p.server_name.to_string(),
            transport: p.transport.as_str().to_owned(),
            server_input: p.server_input.clone(),
            force: p.force,
            redirect_uri: Some(p.redirect_uri.clone()),
            client_id: p.client_id.clone(),
            client_secret: expose(p.client_secret.as_ref()),
            scopes: p.scopes.clone(),
            state: p.state.clone(),
            code_verifier: p.code_verifier.expose_secret().to_owned(),
            expires_at: p.expires_at,
            initiated_by: p.initiated_by.clone(),
        };
        Ok(oauth::replace_pending(&self.0, &w).await?)
    }

    async fn delete_pending(&self, session_id: &str) -> trunks_core::Result<()> {
        Ok(oauth::delete_pending_by_session(&self.0, session_id).await?)
    }

    async fn purge_expired_pending(&self) -> trunks_core::Result<u64> {
        Ok(oauth::purge_expired_pending(&self.0).await?)
    }
}

#[async_trait]
impl AccessStore for Store {
    async fn teams(&self) -> trunks_core::Result<Vec<TeamRecord>> {
        Ok(access::teams(&self.0).await?.into_iter().map(TeamRecord::from).collect())
    }

    async fn team_by_name(&self, name: &str) -> trunks_core::Result<Option<TeamRecord>> {
        Ok(access::team_by_name(&self.0, name).await?.map(TeamRecord::from))
    }

    async fn team_by_id(&self, id: i64) -> trunks_core::Result<Option<TeamRecord>> {
        Ok(access::team_by_id(&self.0, id).await?.map(TeamRecord::from))
    }

    async fn create_team(&self, name: &str, description: &str) -> trunks_core::Result<TeamRecord> {
        Ok(access::create_team(&self.0, name, description).await.map(TeamRecord::from)?)
    }

    async fn update_team(&self, name: &str, description: &str) -> trunks_core::Result<bool> {
        Ok(access::update_team(&self.0, name, description).await?)
    }

    async fn delete_team(&self, name: &str) -> trunks_core::Result<bool> {
        Ok(access::delete_team(&self.0, name).await?)
    }

    async fn members(&self, team_id: i64) -> trunks_core::Result<Vec<TeamMemberRecord>> {
        Ok(access::members(&self.0, team_id)
            .await?
            .into_iter()
            .map(|r: TeamMemberRow| TeamMemberRecord {
                team_id: r.team_id,
                user_id: r.user_id,
                username: r.username,
                role: team_role(&r.role),
            })
            .collect())
    }

    async fn teams_of_user(
        &self,
        user_id: i64,
    ) -> trunks_core::Result<Vec<(TeamRecord, TeamRole)>> {
        Ok(access::teams_of_user(&self.0, user_id)
            .await?
            .into_iter()
            .map(|r| {
                (
                    TeamRecord {
                        id: r.id,
                        name: r.name,
                        description: r.description.unwrap_or_default(),
                    },
                    team_role(&r.role),
                )
            })
            .collect())
    }

    async fn add_member(
        &self,
        team_id: i64,
        user_id: i64,
        role: TeamRole,
    ) -> trunks_core::Result<()> {
        Ok(access::add_member(&self.0, team_id, user_id, role.as_str()).await?)
    }

    async fn remove_member(&self, team_id: i64, user_id: i64) -> trunks_core::Result<bool> {
        Ok(access::remove_member(&self.0, team_id, user_id).await?)
    }

    async fn effective_grants_for_user(&self, user_id: i64) -> trunks_core::Result<Vec<Grant>> {
        Ok(access::grants_for_user(&self.0, user_id).await?.iter().filter_map(grant_from).collect())
    }

    async fn grants_for_subject(
        &self,
        kind: SubjectKind,
        id: i64,
    ) -> trunks_core::Result<Vec<Grant>> {
        Ok(access::grants_for_subject(&self.0, kind.as_str(), id)
            .await?
            .iter()
            .filter_map(grant_from)
            .collect())
    }

    async fn grants(
        &self,
        target: Option<(TargetKind, &str)>,
    ) -> trunks_core::Result<Vec<GrantRecord>> {
        let target = target.map(|(k, n)| (k.as_str(), n));
        Ok(access::grants(&self.0, target)
            .await?
            .into_iter()
            .filter_map(|r: GrantRow| {
                Some(GrantRecord {
                    subject_kind: SubjectKind::parse(&r.subject_kind)?,
                    subject_id: r.subject_id,
                    subject_name: r.subject_name,
                    target_kind: TargetKind::parse(&r.target_kind)?,
                    target_name: r.target_name,
                })
            })
            .collect())
    }

    async fn add_grant(
        &self,
        subject: (SubjectKind, i64),
        target: (TargetKind, &str),
    ) -> trunks_core::Result<bool> {
        Ok(access::add_grant(
            &self.0,
            (subject.0.as_str(), subject.1),
            (target.0.as_str(), target.1),
        )
        .await?)
    }

    async fn remove_grant(
        &self,
        subject: (SubjectKind, i64),
        target: (TargetKind, &str),
    ) -> trunks_core::Result<bool> {
        Ok(access::remove_grant(
            &self.0,
            (subject.0.as_str(), subject.1),
            (target.0.as_str(), target.1),
        )
        .await?)
    }
}

#[async_trait]
impl SessionStore for Store {
    async fn user_by_oidc(
        &self,
        issuer: &str,
        subject: &str,
    ) -> trunks_core::Result<Option<UserRecord>> {
        sessions::user_by_oidc(&self.0, issuer, subject)
            .await?
            .map(UserRecord::try_from)
            .transpose()
    }

    async fn create_oidc_user(
        &self,
        username: &str,
        role: Role,
        token: &str,
        issuer: &str,
        subject: &str,
    ) -> trunks_core::Result<UserRecord> {
        let row =
            sessions::create_oidc_user(&self.0, username, role.as_str(), token, issuer, subject)
                .await?;
        UserRecord::try_from(row)
    }

    async fn set_user_role(&self, user_id: i64, role: Role) -> trunks_core::Result<()> {
        Ok(sessions::set_user_role(&self.0, user_id, role.as_str()).await?)
    }

    async fn sync_oidc_memberships(
        &self,
        user_id: i64,
        team_ids: &[i64],
    ) -> trunks_core::Result<()> {
        Ok(sessions::sync_oidc_memberships(&self.0, user_id, team_ids).await?)
    }

    async fn create_session(
        &self,
        id_hash: &str,
        user_id: i64,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> trunks_core::Result<()> {
        Ok(sessions::create_session(&self.0, id_hash, user_id, expires_at).await?)
    }

    async fn session_user(&self, id_hash: &str) -> trunks_core::Result<Option<UserRecord>> {
        sessions::session_user(&self.0, id_hash).await?.map(UserRecord::try_from).transpose()
    }

    async fn delete_session(&self, id_hash: &str) -> trunks_core::Result<()> {
        Ok(sessions::delete_session(&self.0, id_hash).await?)
    }

    async fn purge_expired_sessions(&self) -> trunks_core::Result<u64> {
        Ok(sessions::purge_expired(&self.0).await?)
    }

    async fn save_login_state(&self, st: &LoginState) -> trunks_core::Result<()> {
        Ok(sessions::save_login_state(
            &self.0,
            &st.state,
            &st.nonce,
            st.code_verifier.expose_secret(),
            &st.return_to,
            st.expires_at,
        )
        .await?)
    }

    async fn take_login_state(&self, state: &str) -> trunks_core::Result<Option<LoginState>> {
        let now = chrono::Utc::now();
        Ok(sessions::take_login_state(&self.0, state).await?.filter(|r| r.expires_at > now).map(
            |r| LoginState {
                state: r.state,
                nonce: r.nonce,
                code_verifier: SecretString::from(r.code_verifier),
                return_to: r.return_to,
                expires_at: r.expires_at,
            },
        ))
    }
}
