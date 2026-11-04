//! `Store`: the `trunks-core` ports implemented over `Db`.

use async_trait::async_trait;
use trunks_core::domain::{
    McpClientRecord, PromptRecord, ResourceRecord, ServerRecord, ToolGroupRecord, ToolRecord,
    UpstreamConfig, UserRecord,
};
use trunks_core::ports::{
    ConfigStore, IdentityStore, RegistrySnapshot, RegistryStore, ServerConfig,
};
use trunks_types::{ResourceUri, Role, ServerMode, ServerName, SessionMode, Transport};

use crate::repos::{catalog, config, identity};
use crate::rows::{
    McpClientRow, PromptRow, ResourceRow, ServerRow, ToolGroupRow, ToolRow, UserRow,
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
        Self {
            id: r.id,
            name: r.name,
            description: r.description.unwrap_or_default(),
            allow_list: string_list(Some(&r.allow_list.0)),
        }
    }
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
}

/// Convenience: a `Result<T>` from this crate mapped into the domain error.
#[allow(dead_code)]
fn lift<T>(r: Result<T>) -> trunks_core::Result<T> {
    r.map_err(Into::into)
}
