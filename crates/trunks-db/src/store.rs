//! `Store`: the `trunks-core` ports implemented over `Db`.

use async_trait::async_trait;
use trunks_core::domain::{
    McpClientRecord, PromptRecord, ResourceRecord, ServerRecord, ToolGroupRecord, ToolRecord,
    UpstreamConfig, UserRecord,
};
use trunks_core::ports::{
    ConfigStore, Discovered, IdentityStore, NewServer, RegistrySnapshot, RegistryStore,
    ServerConfig, ToolGroupSpec,
};
use trunks_types::{GroupName, ResourceUri, Role, ServerMode, ServerName, SessionMode, Transport};

use crate::repos::registry::{NewPromptRow, NewResourceRow, NewServerRow, NewToolRow};
use crate::repos::{catalog, config, groups, identity, registry};
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
    ) -> trunks_core::Result<McpClientRecord> {
        Ok(identity::create_client(&self.0, name, description, token, allow_list)
            .await
            .map(McpClientRecord::from)?)
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
