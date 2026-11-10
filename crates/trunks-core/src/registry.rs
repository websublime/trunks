//! Registry writes: register/deregister servers and flip enabled flags.
//!
//! Invariants (`docs/02-especificacao.md` §4.4): a server is persisted only after a successful
//! `initialize` and `tools/list` against it; prompts and resources are discovered best-effort;
//! every write is one transaction; the catalog is rebuilt after each mutation and stateful
//! sessions of the affected server are invalidated.

use std::sync::Arc;

use secrecy::SecretString;
use serde_json::Value;
use trunks_types::api::{AuthorizationRequired, EnableDisableResult, RegisterServerInput};
use trunks_types::error::messages;
use trunks_types::{CanonicalName, ResourceUri, ServerName, Transport};

use crate::catalog::{Catalog, Snapshot};
use crate::domain::{ServerRecord, UpstreamConfig};
use crate::ports::{
    Discovered, NewPrompt, NewResource, NewServer, NewTool, OAuthStore, RegistryStore,
};
use crate::sessions::SessionManager;
use crate::upstream::{ConnectOptions, UpstreamConn};
use crate::{Error, Result};

#[derive(Clone)]
pub struct RegistryService {
    store: Arc<dyn RegistryStore>,
    oauth: Arc<dyn OAuthStore>,
    catalog: Catalog,
    sessions: Arc<SessionManager>,
    connect: ConnectOptions,
    /// Sessions completed by this process (`session_id → server`), so the web app's polling
    /// can see "completed" after the pending row is gone. Bounded by the TTL of sessions.
    completed_oauth: Arc<dashmap::DashMap<String, (ServerName, std::time::Instant)>>,
}

/// Result of `register`: either done, or the caller must complete an OAuth authorization.
#[derive(Debug, Clone)]
pub enum RegisterOutcome {
    Registered(Arc<ServerRecord>),
    AuthorizationRequired(AuthorizationRequired),
}

impl std::fmt::Debug for RegistryService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryService").finish_non_exhaustive()
    }
}

/// Which entity kind an enable/disable call targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Tool,
    Prompt,
    Resource,
}

impl RegistryService {
    /// `connect.oauth` is set from `oauth` so upstream connections use stored tokens.
    #[must_use]
    pub fn new(
        store: Arc<dyn RegistryStore>,
        oauth: Arc<dyn OAuthStore>,
        catalog: Catalog,
        sessions: Arc<SessionManager>,
        mut connect: ConnectOptions,
    ) -> Self {
        connect.oauth = Some(oauth.clone());
        Self {
            store,
            oauth,
            catalog,
            sessions,
            connect,
            completed_oauth: Arc::new(dashmap::DashMap::new()),
        }
    }

    /// Validates the input, connects to the upstream, discovers its entities and persists
    /// everything. Nothing is stored when the connection or `tools/list` fails. When the
    /// upstream demands OAuth, the pending authorization is persisted instead (§4.8 phase 1).
    pub async fn register(
        &self,
        input: RegisterServerInput,
        force: bool,
    ) -> Result<RegisterOutcome> {
        let name = ServerName::parse(input.name.clone())?;
        let config = config_from_input(&input)?;
        if input.has_oauth_fields() && !matches!(config, UpstreamConfig::StreamableHttp { .. }) {
            return Err(Error::invalid_input(
                "oauth_* fields only apply to the streamable_http transport",
            ));
        }
        if !force && self.catalog.load().servers.contains_key(&name) {
            return Err(Error::invalid_input(format!(
                "server '{name}' already exists; use force to replace it"
            )));
        }

        let conn = match UpstreamConn::connect(&name, &config, &self.connect).await {
            Ok(c) => c,
            Err(Error::UpstreamAuthRequired { www_authenticate, .. }) => {
                let UpstreamConfig::StreamableHttp { url, .. } = &config else {
                    return Err(Error::Internal {
                        reason: "auth challenge on a non-HTTP upstream".into(),
                    });
                };
                let required = crate::oauth::bootstrap(
                    &self.oauth,
                    &name,
                    url,
                    &input,
                    force,
                    Some(&www_authenticate),
                )
                .await?;
                tracing::info!(server = %name, session = %required.session_id, "upstream requires OAuth; authorization pending");
                return Ok(RegisterOutcome::AuthorizationRequired(required));
            }
            Err(e) => return Err(e),
        };
        let discovered = discover(&name, &conn).await;
        conn.close().await;
        let discovered = discovered?;
        tracing::info!(
            server = %name,
            tools = discovered.tools.len(),
            prompts = discovered.prompts.len(),
            resources = discovered.resources.len(),
            "upstream discovered"
        );

        let server = NewServer {
            name: name.clone(),
            description: input.description,
            config,
            session_mode: input.session_mode,
        };
        let record = self.store.create_server(&server, &discovered, force).await?;
        if force {
            self.sessions.invalidate(&name).await;
        }
        self.catalog.reload(self.store.as_ref()).await?;
        Ok(RegisterOutcome::Registered(Arc::new(record)))
    }

    /// §4.8 phase 2: exchanges the authorization code of a pending session for tokens, then
    /// registers the server with them. The pending session is consumed either way once the
    /// exchange succeeded.
    pub async fn complete_oauth(
        &self,
        session_id: &str,
        code: &str,
        state: &str,
    ) -> Result<Arc<ServerRecord>> {
        let code = code.trim();
        let state = state.trim();
        if code.is_empty() || state.is_empty() {
            return Err(Error::invalid_input("code and state are required"));
        }
        let pending = self
            .oauth
            .pending(session_id)
            .await?
            .ok_or_else(|| Error::not_found(format!("authorization session '{session_id}'")))?;
        let input: RegisterServerInput = serde_json::from_value(pending.server_input.clone())
            .map_err(|e| Error::Internal {
                reason: format!("stored registration input is invalid: {e}"),
            })?;
        let url = input.url.clone().ok_or_else(|| Error::Internal {
            reason: "stored registration input has no url".into(),
        })?;
        crate::oauth::exchange(&self.oauth, &pending, &url, code, state).await?;
        self.oauth.delete_pending(session_id).await?;
        match self.register(input, pending.force).await? {
            RegisterOutcome::Registered(server) => {
                self.remember_completed(session_id, &server.name);
                Ok(server)
            }
            RegisterOutcome::AuthorizationRequired(_) => Err(Error::Upstream {
                server: pending.server_name.to_string(),
                reason: "the upstream still rejects the new access token".into(),
            }),
        }
    }

    pub async fn deregister(&self, name: &ServerName) -> Result<()> {
        if !self.store.delete_server(name).await? {
            return Err(Error::not_found(format!("server '{name}'")));
        }
        self.sessions.invalidate(name).await;
        self.oauth.delete_oauth(name).await?;
        self.catalog.reload(self.store.as_ref()).await?;
        Ok(())
    }

    /// Removes expired pending authorizations (called at startup).
    pub async fn purge_expired_oauth(&self) -> Result<u64> {
        self.oauth.purge_expired_pending().await
    }

    /// The browser-callback variant of `complete_oauth`: the IdP only returns `code` and
    /// `state`, so the session is looked up by `state`. Returns the registered server.
    pub async fn complete_oauth_by_state(
        &self,
        code: &str,
        state: &str,
    ) -> Result<Arc<ServerRecord>> {
        let pending = self
            .oauth
            .pending_by_state(state)
            .await?
            .ok_or_else(|| Error::not_found("authorization session for this state"))?;
        self.complete_oauth(&pending.session_id, code, state).await
    }

    /// Dashboard inputs the catalog does not hold (pending sessions, which servers hold OAuth
    /// tokens, gateway health notes).
    pub async fn dashboard_extras(&self) -> Result<crate::dashboard::Extras> {
        let mut extras = crate::dashboard::Extras::default();
        for p in self.oauth.pending_all().await? {
            extras.pending.insert(p.server_name.clone(), p);
        }
        for name in self.catalog.load().servers.keys() {
            if self.oauth.credentials(name).await?.is_some_and(|c| c.has_tokens()) {
                extras.authorized.push(name.clone());
            }
        }
        for (name, h) in self.sessions.health() {
            extras.health.insert(name, h);
        }
        Ok(extras)
    }

    fn remember_completed(&self, session_id: &str, server: &ServerName) {
        let now = std::time::Instant::now();
        let ttl = std::time::Duration::from_hours(1);
        self.completed_oauth.retain(|_, (_, at)| now.duration_since(*at) < ttl);
        self.completed_oauth.insert(session_id.to_owned(), (server.clone(), now));
    }

    /// Status of an authorization session for the web app's polling.
    pub async fn oauth_session_status(
        &self,
        session_id: &str,
    ) -> Result<crate::dashboard::OAuthSessionStatus> {
        let pending = self.oauth.pending(session_id).await?;
        let registered = self.completed_oauth.get(session_id).map(|e| e.0.clone());
        Ok(crate::dashboard::oauth_session_status(
            session_id,
            pending.as_ref(),
            registered.as_ref(),
        ))
    }

    /// Flips the server flag. The affected lists hold the entities whose **effective** state
    /// changed (those individually enabled); empty when the server was already in that state.
    pub async fn set_server_enabled(
        &self,
        name: &ServerName,
        enabled: bool,
    ) -> Result<EnableDisableResult> {
        let before = self.catalog.load();
        before.server(name)?;
        let Some(changed) = self.store.set_server_enabled(name, enabled).await? else {
            return Err(Error::not_found(format!("server '{name}'")));
        };
        let mut result = EnableDisableResult { name: Some(name.to_string()), ..Default::default() };
        if !changed {
            return Ok(result);
        }
        if !enabled {
            self.sessions.invalidate(name).await;
        }
        let after = self.catalog.reload(self.store.as_ref()).await?;
        result.tools_affected = before
            .tools
            .values()
            .filter(|t| &t.server == name)
            .filter(|t| after.tools.get(&t.canonical).is_some_and(|n| n.enabled != t.enabled))
            .map(|t| t.canonical.to_string())
            .collect();
        result.prompts_affected = before
            .prompts
            .values()
            .filter(|p| &p.server == name)
            .filter(|p| after.prompts.get(&p.canonical).is_some_and(|n| n.enabled != p.enabled))
            .map(|p| p.canonical.to_string())
            .collect();
        result.resources_affected = before
            .resources
            .values()
            .filter(|r| &r.server == name)
            .filter(|r| after.resources.get(&r.uri).is_some_and(|n| n.enabled != r.enabled))
            .map(|r| r.uri.to_string())
            .collect();
        Ok(result)
    }

    /// `entity` is a server name (every entity of that kind on it; returns the ones that
    /// changed) or one canonical name / `trunks://` URI (always returned, even if unchanged).
    pub async fn set_entities_enabled(
        &self,
        kind: EntityKind,
        entity: &str,
        enabled: bool,
    ) -> Result<Vec<String>> {
        let snap = self.catalog.load();
        if let Ok(server_name) = ServerName::parse(entity)
            && let Some(server) = snap.servers.get(&server_name)
        {
            let changed = self.flip(kind, server.id, None, enabled).await?;
            self.catalog.reload(self.store.as_ref()).await?;
            return Ok(match kind {
                EntityKind::Tool | EntityKind::Prompt => changed
                    .into_iter()
                    .map(|n| CanonicalName::new(server_name.clone(), n).to_string())
                    .collect(),
                EntityKind::Resource => changed,
            });
        }
        let (server_id, key) = resolve_single(&snap, kind, entity)?;
        self.flip(kind, server_id, Some(&key), enabled).await?;
        self.catalog.reload(self.store.as_ref()).await?;
        Ok(vec![entity.to_owned()])
    }

    async fn flip(
        &self,
        kind: EntityKind,
        server_id: i64,
        key: Option<&str>,
        enabled: bool,
    ) -> Result<Vec<String>> {
        match kind {
            EntityKind::Tool => self.store.set_tools_enabled(server_id, key, enabled).await,
            EntityKind::Prompt => self.store.set_prompts_enabled(server_id, key, enabled).await,
            EntityKind::Resource => self.store.set_resources_enabled(server_id, key, enabled).await,
        }
    }
}

/// Resolves a single entity to `(server_id, store key)`: the bare name for tools and prompts,
/// the public URI for resources.
fn resolve_single(snap: &Snapshot, kind: EntityKind, entity: &str) -> Result<(i64, String)> {
    match kind {
        EntityKind::Tool => {
            let canonical = parse_canonical(entity, "tool")?;
            let tool = snap
                .tools
                .get(&canonical)
                .ok_or_else(|| Error::not_found(format!("server or tool '{entity}'")))?;
            Ok((snap.server(&tool.server)?.id, canonical.local().to_owned()))
        }
        EntityKind::Prompt => {
            let canonical = parse_canonical(entity, "prompt")?;
            let prompt = snap
                .prompts
                .get(&canonical)
                .ok_or_else(|| Error::not_found(format!("server or prompt '{entity}'")))?;
            Ok((snap.server(&prompt.server)?.id, canonical.local().to_owned()))
        }
        EntityKind::Resource => {
            let uri = ResourceUri::parse(entity).map_err(|_| {
                Error::invalid_input(format!(
                    "'{entity}' is neither a registered server name nor a trunks:// resource URI"
                ))
            })?;
            let res = snap
                .resources
                .get(&uri)
                .ok_or_else(|| Error::not_found(format!("resource '{entity}'")))?;
            Ok((snap.server(&res.server)?.id, uri.to_string()))
        }
    }
}

fn parse_canonical(entity: &str, kind: &str) -> Result<CanonicalName> {
    CanonicalName::parse(entity)
        .map_err(|_| Error::not_found(format!("server or {kind} '{entity}'")))
}

fn config_from_input(input: &RegisterServerInput) -> Result<UpstreamConfig> {
    let secret = |(k, v): (&String, &String)| (k.clone(), SecretString::from(v.clone()));
    match input.transport {
        Transport::Stdio => {
            let command = input
                .command
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .ok_or_else(|| Error::invalid_input(messages::COMMAND_REQUIRED))?;
            Ok(UpstreamConfig::Stdio {
                command: command.to_owned(),
                args: input.args.clone(),
                env: input.env.iter().map(secret).collect(),
            })
        }
        Transport::StreamableHttp => {
            let url = input
                .url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .ok_or_else(|| Error::invalid_input(messages::URL_REQUIRED))?;
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(Error::invalid_input(format!(
                    "invalid url '{url}': expected an http:// or https:// URL"
                )));
            }
            Ok(UpstreamConfig::StreamableHttp {
                url: url.to_owned(),
                bearer_token: input
                    .bearer_token
                    .as_deref()
                    .filter(|t| !t.is_empty())
                    .map(|t| SecretString::from(t.to_owned())),
                headers: input.headers.iter().map(secret).collect(),
            })
        }
        Transport::Sse => Err(Error::invalid_input(messages::SSE_NOT_SUPPORTED)),
    }
}

/// `tools/list` is mandatory; prompts and resources are listed only when advertised, and a
/// failure there is logged, not fatal.
async fn discover(server: &ServerName, conn: &UpstreamConn) -> Result<Discovered> {
    let tools =
        conn.list_tools().await?.into_iter().filter_map(|t| tool_from(&to_value(&t))).collect();
    let caps = conn.capabilities();
    let mut out = Discovered { tools, ..Default::default() };
    if caps.as_ref().is_some_and(|c| c.prompts.is_some()) {
        match conn.list_prompts().await {
            Ok(list) => {
                out.prompts = list.iter().filter_map(|p| prompt_from(&to_value(p))).collect();
            }
            Err(e) => {
                tracing::warn!(server = %server, error = %e, "prompts/list failed; none registered");
            }
        }
    }
    if caps.as_ref().is_some_and(|c| c.resources.is_some()) {
        match conn.list_resources().await {
            Ok(list) => {
                out.resources =
                    list.iter().filter_map(|r| resource_from(server, &to_value(r))).collect();
            }
            Err(e) => {
                tracing::warn!(server = %server, error = %e, "resources/list failed; none registered");
            }
        }
    }
    Ok(out)
}

/// Entities are read through their JSON form (the MCP wire shape), so this module does not
/// depend on the exact field layout of the rmcp structs.
fn to_value<T: serde::Serialize>(t: &T) -> Value {
    serde_json::to_value(t).unwrap_or(Value::Null)
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn opt_field(v: &Value, key: &str) -> Option<Value> {
    v.get(key).filter(|x| !x.is_null()).cloned()
}

fn tool_from(v: &Value) -> Option<NewTool> {
    Some(NewTool {
        name: str_field(v, "name")?,
        description: str_field(v, "description").unwrap_or_default(),
        input_schema: opt_field(v, "inputSchema"),
        annotations: opt_field(v, "annotations"),
    })
}

fn prompt_from(v: &Value) -> Option<NewPrompt> {
    Some(NewPrompt {
        name: str_field(v, "name")?,
        description: str_field(v, "description").unwrap_or_default(),
        arguments: opt_field(v, "arguments"),
    })
}

fn resource_from(server: &ServerName, v: &Value) -> Option<NewResource> {
    // An empty URI cannot be exposed: `trunks://res/<server>/` does not parse back.
    let original = str_field(v, "uri").filter(|uri| !uri.is_empty())?;
    Some(NewResource {
        uri: ResourceUri::new(server.clone(), original.clone()),
        original_uri: original,
        name: str_field(v, "name").unwrap_or_default(),
        description: str_field(v, "description").unwrap_or_default(),
        mime_type: str_field(v, "mimeType"),
        annotations: opt_field(v, "annotations"),
        meta: opt_field(v, "_meta"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resources_without_a_usable_uri_are_skipped() {
        let server = ServerName::parse("srv").unwrap();
        let cases = [
            (serde_json::json!({"uri": "mem://hello", "name": "hello"}), true),
            (serde_json::json!({"uri": "", "name": "empty"}), false),
            (serde_json::json!({"name": "missing"}), false),
            (serde_json::json!({"uri": 7}), false),
        ];
        for (value, kept) in cases {
            let resource = resource_from(&server, &value);
            assert_eq!(resource.is_some(), kept, "{value}");
            if let Some(r) = resource {
                assert_eq!(ResourceUri::parse(&r.uri.to_string()).unwrap(), r.uri);
            }
        }
    }
}
