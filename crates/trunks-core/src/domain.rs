//! Domain records: what the registry stores, independent of any database row shape.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::Value;
use trunks_types::api;
use trunks_types::{CanonicalName, ResourceUri, ServerName, SessionMode, Transport};

use crate::{Error, Result};

/// How to reach an upstream server. Credentials are secrets.
#[derive(Debug, Clone)]
pub enum UpstreamConfig {
    Stdio {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, SecretString>,
    },
    StreamableHttp {
        url: String,
        bearer_token: Option<SecretString>,
        headers: BTreeMap<String, SecretString>,
    },
    /// Legacy transport, kept only so stored rows can be read (never connected, D4).
    Sse {
        url: String,
        bearer_token: Option<SecretString>,
    },
}

#[derive(Deserialize)]
struct StdioJson {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct HttpJson {
    url: String,
    #[serde(default)]
    bearer_token: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

impl UpstreamConfig {
    /// Parses the stored `config` JSON for the given transport.
    pub fn from_json(transport: Transport, config: &Value) -> Result<Self> {
        let bad = |e: serde_json::Error| Error::Internal {
            reason: format!("invalid stored config for {transport}: {e}"),
        };
        Ok(match transport {
            Transport::Stdio => {
                let j: StdioJson = serde_json::from_value(config.clone()).map_err(bad)?;
                Self::Stdio {
                    command: j.command,
                    args: j.args,
                    env: j.env.into_iter().map(|(k, v)| (k, SecretString::from(v))).collect(),
                }
            }
            Transport::StreamableHttp => {
                let j: HttpJson = serde_json::from_value(config.clone()).map_err(bad)?;
                Self::StreamableHttp {
                    url: j.url,
                    bearer_token: j.bearer_token.filter(|t| !t.is_empty()).map(SecretString::from),
                    headers: j
                        .headers
                        .into_iter()
                        .map(|(k, v)| (k, SecretString::from(v)))
                        .collect(),
                }
            }
            Transport::Sse => {
                let j: HttpJson = serde_json::from_value(config.clone()).map_err(bad)?;
                Self::Sse {
                    url: j.url,
                    bearer_token: j.bearer_token.filter(|t| !t.is_empty()).map(SecretString::from),
                }
            }
        })
    }

    #[must_use]
    pub fn transport(&self) -> Transport {
        match self {
            Self::Stdio { .. } => Transport::Stdio,
            Self::StreamableHttp { .. } => Transport::StreamableHttp,
            Self::Sse { .. } => Transport::Sse,
        }
    }

    /// Full JSON as stored (includes secrets). Only for admin-level export.
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Stdio { command, args, env } => serde_json::json!({
                "command": command, "args": args,
                "env": env.iter().map(|(k, v)| (k.clone(), Value::String(v.expose_secret().to_owned()))).collect::<serde_json::Map<_, _>>(),
            }),
            Self::StreamableHttp { url, bearer_token, headers } => serde_json::json!({
                "url": url,
                "bearer_token": bearer_token.as_ref().map(|t| t.expose_secret().to_owned()),
                "headers": headers.iter().map(|(k, v)| (k.clone(), Value::String(v.expose_secret().to_owned()))).collect::<serde_json::Map<_, _>>(),
            }),
            Self::Sse { url, bearer_token } => serde_json::json!({
                "url": url, "bearer_token": bearer_token.as_ref().map(|t| t.expose_secret().to_owned()),
            }),
        }
    }
}

/// A registered upstream server.
#[derive(Debug, Clone)]
pub struct ServerRecord {
    pub id: i64,
    pub name: ServerName,
    pub enabled: bool,
    pub description: String,
    pub config: UpstreamConfig,
    pub session_mode: SessionMode,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl ServerRecord {
    /// Sanitized API view: never carries credential values.
    #[must_use]
    pub fn to_api(&self) -> api::McpServer {
        let (url, header_keys, command, args, env_keys) = match &self.config {
            UpstreamConfig::Stdio { command, args, env } => {
                (None, vec![], Some(command.clone()), args.clone(), env.keys().cloned().collect())
            }
            UpstreamConfig::StreamableHttp { url, headers, .. } => {
                (Some(url.clone()), headers.keys().cloned().collect(), None, vec![], vec![])
            }
            UpstreamConfig::Sse { url, .. } => (Some(url.clone()), vec![], None, vec![], vec![]),
        };
        api::McpServer {
            name: self.name.clone(),
            transport: self.config.transport(),
            enabled: self.enabled,
            description: self.description.clone(),
            session_mode: self.session_mode,
            url,
            header_keys,
            command,
            args,
            env_keys,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// A tool as stored: bare upstream name plus its server.
#[derive(Debug, Clone)]
pub struct ToolRecord {
    pub id: i64,
    pub server_id: i64,
    pub name: String,
    pub enabled: bool,
    pub description: String,
    pub input_schema: Option<Value>,
    pub annotations: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct PromptRecord {
    pub id: i64,
    pub server_id: i64,
    pub name: String,
    pub enabled: bool,
    pub description: String,
    pub arguments: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ResourceRecord {
    pub id: i64,
    pub server_id: i64,
    pub uri: ResourceUri,
    pub original_uri: String,
    pub name: String,
    pub enabled: bool,
    pub description: String,
    pub mime_type: Option<String>,
    pub annotations: Option<Value>,
    pub meta: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ToolGroupRecord {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub included_tools: Vec<String>,
    pub included_servers: Vec<String>,
    pub excluded_tools: Vec<String>,
}

/// A tool resolved against its server: what the catalog and the gateway work with.
#[derive(Debug, Clone)]
pub struct ToolEntry {
    pub canonical: CanonicalName,
    pub server: ServerName,
    pub enabled: bool,
    pub description: String,
    pub input_schema: Option<Value>,
    pub annotations: Option<Value>,
}

impl ToolEntry {
    #[must_use]
    pub fn to_api(&self) -> api::Tool {
        api::Tool {
            name: self.canonical.clone(),
            server: self.server.clone(),
            enabled: self.enabled,
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
            annotations: self.annotations.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptEntry {
    pub canonical: CanonicalName,
    pub server: ServerName,
    pub enabled: bool,
    pub description: String,
    pub arguments: Option<Value>,
}

impl PromptEntry {
    #[must_use]
    pub fn to_api(&self) -> api::Prompt {
        api::Prompt {
            name: self.canonical.clone(),
            server: self.server.clone(),
            enabled: self.enabled,
            description: self.description.clone(),
            arguments: self.arguments.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResourceEntry {
    pub uri: ResourceUri,
    pub server: ServerName,
    pub original_uri: String,
    pub name: String,
    pub enabled: bool,
    pub description: String,
    pub mime_type: Option<String>,
    pub annotations: Option<Value>,
    pub meta: Option<Value>,
}

impl ResourceEntry {
    #[must_use]
    pub fn to_api(&self) -> api::Resource {
        api::Resource {
            uri: self.uri.clone(),
            server: self.server.clone(),
            name: self.name.clone(),
            enabled: self.enabled,
            description: self.description.clone(),
            mime_type: self.mime_type.clone(),
            annotations: self.annotations.clone(),
            meta: self.meta.clone(),
        }
    }
}

/// A human user.
#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: i64,
    pub username: String,
    pub role: trunks_types::Role,
}

/// A machine identity for MCP clients.
#[derive(Debug, Clone)]
pub struct McpClientRecord {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub allow_list: Vec<String>,
}

impl McpClientRecord {
    /// `"*"` grants every server; otherwise exact name match. Malformed lists grant nothing.
    #[must_use]
    pub fn may_access(&self, server: &ServerName) -> bool {
        self.allow_list.iter().any(|a| a == "*" || a == server.as_str())
    }
}
