//! Read-only, **sanitized** views for the web app (`/api/dashboard`, D15).
//!
//! Everything here is derived from the catalog snapshot plus a few runtime facts; nothing in
//! these structs can carry a credential: `config_summary` lists header/env **names** only,
//! commands are truncated, OAuth state is reduced to a status.
//!
//! Every view is built **for a viewer** (D14 applied to D15): it holds only what that caller
//! may use, counts included, so what is outside their reach looks exactly like what does not
//! exist. Administrators (and development mode) see everything.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use trunks_types::api::{Prompt, Resource, Tool, ToolGroup};
use trunks_types::{ServerMode, ServerName, SessionMode, Transport};

use crate::catalog::{GroupView, Snapshot};
use crate::domain::{ServerRecord, UpstreamConfig};
use crate::identity::UserPrincipal;
use crate::ports::PendingAuthorization;
use crate::sessions::UpstreamHealth;

/// Facts about the running process that the catalog does not know.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeInfo {
    pub mode: ServerMode,
    pub version: String,
    pub started_at: DateTime<Utc>,
    pub database_backend: String,
    /// Credentials stripped.
    pub database_url: String,
    pub migrations_latest: Option<i64>,
    pub bind_host: String,
    pub bind_port: u16,
    pub allowed_hosts: Vec<String>,
    pub upstream_init_timeout_sec: u64,
    pub session_idle_timeout_sec: u64,
    pub metrics_enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Counts {
    pub servers: usize,
    pub servers_enabled: usize,
    pub tools: usize,
    pub tools_enabled: usize,
    pub prompts: usize,
    pub resources: usize,
    pub tool_groups: usize,
    pub pending_oauth: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionStats {
    pub stateful_active: u64,
}

/// `GET /api/dashboard/overview`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Overview {
    pub mode: ServerMode,
    pub version: String,
    pub uptime_seconds: i64,
    /// Of what the viewer may use.
    pub counts: Counts,
    /// A fact about the whole process: present for administrators only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<SessionStats>,
    pub metrics_enabled: bool,
}

/// Connection settings without any secret.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header_keys: Vec<String>,
    /// Truncated to 80 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_keys: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OAuthStatus {
    #[default]
    None,
    Pending,
    Authorized,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthSummary {
    pub status: OAuthStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityCounts {
    pub tools: usize,
    pub tools_enabled: usize,
    pub prompts: usize,
    pub resources: usize,
}

/// Items of `GET /api/dashboard/servers`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerSummary {
    pub name: ServerName,
    pub transport: Transport,
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
    pub session_mode: SessionMode,
    pub config_summary: ConfigSummary,
    pub counts: EntityCounts,
    pub oauth: OAuthSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

/// `GET /api/dashboard/servers/{name}`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerDetail {
    #[serde(flatten)]
    pub summary: ServerSummary,
    pub tools: Vec<Tool>,
    pub prompts: Vec<Prompt>,
    pub resources: Vec<Resource>,
}

/// Items of `GET /api/dashboard/tool-groups`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupSummary {
    #[serde(flatten)]
    pub group: ToolGroup,
    pub streamable_http_endpoint: String,
    pub effective_tools: Vec<String>,
    /// `true` when the definition resolves to no enabled tool.
    pub degraded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnreachableServer {
    pub name: ServerName,
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingOAuth {
    pub session_id: String,
    pub server_name: ServerName,
    pub expires_at: DateTime<Utc>,
}

/// `GET /api/dashboard/diagnostics`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostics {
    pub database: DatabaseInfo,
    pub bind: BindInfo,
    pub allowed_hosts: Vec<String>,
    pub upstream_init_timeout_sec: u64,
    pub session_idle_timeout_sec: u64,
    pub metrics_enabled: bool,
    pub servers_unreachable: Vec<UnreachableServer>,
    pub pending_oauth: Vec<PendingOAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseInfo {
    pub backend: String,
    pub url_redacted: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migrations_latest: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindInfo {
    pub host: String,
    pub port: u16,
}

/// `GET /api/dashboard/oauth/session/{id}`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OAuthSessionStatus {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<ServerName>,
    pub status: OAuthSessionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OAuthSessionState {
    Pending,
    Expired,
    Completed,
    Unknown,
}

const COMMAND_MAX: usize = 80;

#[must_use]
pub fn config_summary(config: &UpstreamConfig) -> ConfigSummary {
    match config {
        UpstreamConfig::Stdio { command, args, env } => ConfigSummary {
            command: Some(truncate(command, COMMAND_MAX)),
            args: args.clone(),
            env_keys: env.keys().cloned().collect(),
            ..Default::default()
        },
        UpstreamConfig::StreamableHttp { url, headers, .. } => ConfigSummary {
            url: Some(url.clone()),
            header_keys: headers.keys().cloned().collect(),
            ..Default::default()
        },
        UpstreamConfig::Sse { url, .. } => {
            ConfigSummary { url: Some(url.clone()), ..Default::default() }
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Inputs that are not in the snapshot: pending authorizations (keyed by server), which
/// servers hold OAuth tokens, and the gateway's health notes.
#[derive(Debug, Default)]
pub struct Extras {
    pub pending: BTreeMap<ServerName, PendingAuthorization>,
    pub authorized: Vec<ServerName>,
    pub health: BTreeMap<ServerName, UpstreamHealth>,
}

impl Extras {
    fn oauth_for(&self, name: &ServerName) -> OAuthSummary {
        if let Some(p) = self.pending.get(name)
            && p.expires_at > Utc::now()
        {
            return OAuthSummary {
                status: OAuthStatus::Pending,
                session_id: Some(p.session_id.clone()),
                expires_at: Some(p.expires_at),
            };
        }
        if self.authorized.contains(name) {
            return OAuthSummary { status: OAuthStatus::Authorized, ..Default::default() };
        }
        OAuthSummary::default()
    }
}

#[must_use]
pub fn server_summary(
    snap: &Snapshot,
    s: &ServerRecord,
    extras: &Extras,
    viewer: &UserPrincipal,
) -> ServerSummary {
    let tools: Vec<_> = snap
        .tools
        .values()
        .filter(|t| t.server == s.name && viewer.may_use_tool(snap, &t.canonical))
        .collect();
    // Prompts and resources come only with the server itself, never through a group.
    let whole = viewer.may_use_server(&s.name);
    let oauth = extras.oauth_for(&s.name);
    ServerSummary {
        name: s.name.clone(),
        transport: s.config.transport(),
        enabled: s.enabled,
        description: s.description.clone(),
        session_mode: s.session_mode,
        config_summary: config_summary(&s.config),
        counts: EntityCounts {
            tools: tools.len(),
            tools_enabled: tools.iter().filter(|t| t.enabled).count(),
            prompts: snap.prompts.values().filter(|p| whole && p.server == s.name).count(),
            resources: snap.resources.values().filter(|r| whole && r.server == s.name).count(),
        },
        // The id of a pending authorization is what completes it: administrators only.
        oauth: if viewer.is_admin() {
            oauth
        } else {
            OAuthSummary { status: oauth.status, ..Default::default() }
        },
        last_error: extras.health.get(&s.name).and_then(|h| h.last_error.clone()),
        created_at: s.created_at,
        updated_at: s.updated_at,
    }
}

/// The servers the viewer may see.
#[must_use]
pub fn servers(snap: &Snapshot, extras: &Extras, viewer: &UserPrincipal) -> Vec<ServerSummary> {
    snap.servers
        .values()
        .filter(|s| viewer.may_see_server(snap, &s.name))
        .map(|s| server_summary(snap, s, extras, viewer))
        .collect()
}

/// One server with the entities of it the viewer may use. The caller checks visibility
/// (`UserPrincipal::visible_server`).
#[must_use]
pub fn server_detail(
    snap: &Snapshot,
    s: &ServerRecord,
    extras: &Extras,
    viewer: &UserPrincipal,
) -> ServerDetail {
    let whole = viewer.may_use_server(&s.name);
    ServerDetail {
        summary: server_summary(snap, s, extras, viewer),
        tools: snap
            .tools
            .values()
            .filter(|t| t.server == s.name && viewer.may_use_tool(snap, &t.canonical))
            .map(|t| t.to_api())
            .collect(),
        prompts: snap
            .prompts
            .values()
            .filter(|p| whole && p.server == s.name)
            .map(|p| p.to_api())
            .collect(),
        resources: snap
            .resources
            .values()
            .filter(|r| whole && r.server == s.name)
            .map(|r| r.to_api())
            .collect(),
    }
}

#[must_use]
pub fn group_summary(g: &Arc<GroupView>) -> GroupSummary {
    GroupSummary {
        group: crate::groups::to_api(g),
        streamable_http_endpoint: crate::groups::endpoint(&g.name),
        effective_tools: g.effective_tools.iter().map(ToString::to_string).collect(),
        degraded: g.effective_tools.is_empty(),
    }
}

/// Counts of what the viewer may use. `stateful_active` describes the whole process and is
/// reported to administrators only.
#[must_use]
pub fn overview(
    snap: &Snapshot,
    info: &RuntimeInfo,
    stateful_active: u64,
    extras: &Extras,
    viewer: &UserPrincipal,
) -> Overview {
    let now = Utc::now();
    let servers: Vec<_> =
        snap.servers.values().filter(|s| viewer.may_see_server(snap, &s.name)).collect();
    let tools: Vec<_> =
        snap.tools.values().filter(|t| viewer.may_use_tool(snap, &t.canonical)).collect();
    Overview {
        mode: info.mode,
        version: info.version.clone(),
        uptime_seconds: (now - info.started_at).num_seconds().max(0),
        counts: Counts {
            servers: servers.len(),
            servers_enabled: servers.iter().filter(|s| s.enabled).count(),
            tools: tools.len(),
            tools_enabled: tools.iter().filter(|t| t.enabled).count(),
            prompts: snap.prompts.values().filter(|p| viewer.may_use_server(&p.server)).count(),
            resources: snap.resources.values().filter(|r| viewer.may_use_server(&r.server)).count(),
            tool_groups: snap.groups.values().filter(|g| viewer.may_use_group(&g.name)).count(),
            // Pending authorizations belong to registrations, an administrator's business.
            pending_oauth: if viewer.is_admin() {
                extras.pending.values().filter(|p| p.expires_at > now).count()
            } else {
                0
            },
        },
        sessions: viewer.is_admin().then_some(SessionStats { stateful_active }),
        metrics_enabled: info.metrics_enabled,
    }
}

#[must_use]
pub fn diagnostics(info: &RuntimeInfo, extras: &Extras) -> Diagnostics {
    let now = Utc::now();
    Diagnostics {
        database: DatabaseInfo {
            backend: info.database_backend.clone(),
            url_redacted: info.database_url.clone(),
            migrations_latest: info.migrations_latest,
        },
        bind: BindInfo { host: info.bind_host.clone(), port: info.bind_port },
        allowed_hosts: info.allowed_hosts.clone(),
        upstream_init_timeout_sec: info.upstream_init_timeout_sec,
        session_idle_timeout_sec: info.session_idle_timeout_sec,
        metrics_enabled: info.metrics_enabled,
        servers_unreachable: extras
            .health
            .iter()
            .filter_map(|(name, h)| {
                h.last_error.as_ref().map(|e| UnreachableServer {
                    name: name.clone(),
                    error: e.clone(),
                    at: h.last_error_at,
                })
            })
            .collect(),
        pending_oauth: extras
            .pending
            .values()
            .filter(|p| p.expires_at > now)
            .map(|p| PendingOAuth {
                session_id: p.session_id.clone(),
                server_name: p.server_name.clone(),
                expires_at: p.expires_at,
            })
            .collect(),
    }
}

/// Status of one authorization session for the web app's polling.
#[must_use]
pub fn oauth_session_status(
    session_id: &str,
    pending: Option<&PendingAuthorization>,
    registered: Option<&ServerName>,
) -> OAuthSessionStatus {
    match (pending, registered) {
        (Some(p), _) if p.expires_at > Utc::now() => OAuthSessionStatus {
            session_id: session_id.to_owned(),
            server_name: Some(p.server_name.clone()),
            status: OAuthSessionState::Pending,
            expires_at: Some(p.expires_at),
        },
        (Some(p), _) => OAuthSessionStatus {
            session_id: session_id.to_owned(),
            server_name: Some(p.server_name.clone()),
            status: OAuthSessionState::Expired,
            expires_at: Some(p.expires_at),
        },
        (None, Some(name)) => OAuthSessionStatus {
            session_id: session_id.to_owned(),
            server_name: Some(name.clone()),
            status: OAuthSessionState::Completed,
            expires_at: None,
        },
        (None, None) => OAuthSessionStatus {
            session_id: session_id.to_owned(),
            server_name: None,
            status: OAuthSessionState::Unknown,
            expires_at: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;

    #[test]
    fn config_summary_never_carries_values() {
        let cfg = UpstreamConfig::StreamableHttp {
            url: "https://u/mcp".into(),
            bearer_token: Some(SecretString::from("tok".to_owned())),
            headers: BTreeMap::from([("X-Key".to_owned(), SecretString::from("v".to_owned()))]),
        };
        let s = config_summary(&cfg);
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("X-Key") && !json.contains("tok") && !json.contains("\"v\""));
        let cfg = UpstreamConfig::Stdio {
            command: "x".repeat(200),
            args: vec!["--flag".into()],
            env: BTreeMap::from([("SECRET".to_owned(), SecretString::from("s".to_owned()))]),
        };
        let s = config_summary(&cfg);
        assert_eq!(s.command.as_ref().unwrap().chars().count(), 80);
        assert_eq!(s.env_keys, vec!["SECRET"]);
        assert!(!serde_json::to_string(&s).unwrap().contains("\"s\""));
    }
}
