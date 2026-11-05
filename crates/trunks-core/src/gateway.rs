//! The MCP gateway: one `ServerHandler` parameterized by scope, over the shared catalog.
//!
//! Authorization is fail-closed: a missing `Principal` yields empty listings and denied calls.
//! Names and URIs are rewritten canonical ↔ upstream on the way in and out. This is the second
//! module allowed to import `rmcp` directly.

use std::sync::Arc;
use std::time::Instant;

use rmcp::RoleServer;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ErrorData, GetPromptRequestParams,
    GetPromptResponse, GetPromptResult, Implementation, JsonObject, ListPromptsResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, Prompt, PromptArgument,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
    ResourceContents, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use trunks_types::{CanonicalName, GroupName, ResourceUri, ServerName};

use crate::catalog::{Catalog, Snapshot};
use crate::domain::{PromptEntry, ResourceEntry, ToolEntry};
use crate::identity::Principal;
use crate::metrics::Metrics;
use crate::sessions::SessionManager;
use crate::{Error, Result};

/// Which part of the catalog this handler exposes.
#[derive(Debug, Clone)]
pub enum Scope {
    Global,
    Group(GroupName),
}

/// Shared by every handler instance and by the REST invoker.
pub struct GatewayState {
    pub catalog: Catalog,
    pub sessions: Arc<SessionManager>,
    pub metrics: Option<Metrics>,
    pub version: String,
}

impl std::fmt::Debug for GatewayState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayState").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct GatewayHandler {
    scope: Scope,
    state: Arc<GatewayState>,
}

impl GatewayHandler {
    #[must_use]
    pub fn new(scope: Scope, state: Arc<GatewayState>) -> Self {
        Self { scope, state }
    }

    fn principal(ctx: &RequestContext<RoleServer>) -> Option<Principal> {
        ctx.extensions.get::<http::request::Parts>()?.extensions.get::<Principal>().cloned()
    }

    fn in_scope(&self, snap: &Snapshot, tool: &CanonicalName) -> bool {
        match &self.scope {
            Scope::Global => true,
            Scope::Group(g) => snap.groups.get(g).is_some_and(|v| v.effective_tools.contains(tool)),
        }
    }

    fn mcp_err(e: &Error) -> ErrorData {
        match e {
            Error::NotFound { .. } | Error::InvalidInput { .. } => {
                ErrorData::invalid_params(e.to_string(), None)
            }
            _ => ErrorData::internal_error(e.to_string(), None),
        }
    }
}

// ---------------------------------------------------------------- conversions
fn to_mcp_tool(t: &ToolEntry) -> Tool {
    let schema: JsonObject =
        t.input_schema.as_ref().and_then(|v| v.as_object().cloned()).unwrap_or_default();
    let mut tool = Tool::new(t.canonical.to_string(), t.description.clone(), Arc::new(schema));
    if let Some(a) = &t.annotations {
        tool.annotations = serde_json::from_value(a.clone()).ok();
    }
    tool
}

fn to_mcp_prompt(p: &PromptEntry) -> Prompt {
    let arguments: Option<Vec<PromptArgument>> =
        p.arguments.as_ref().and_then(|v| serde_json::from_value(v.clone()).ok());
    let mut prompt = Prompt::new(p.canonical.to_string(), Some(p.description.clone()), arguments);
    if p.description.is_empty() {
        prompt.description = None;
    }
    prompt
}

fn to_mcp_resource(r: &ResourceEntry) -> Resource {
    let mut res = Resource::new(r.uri.to_string(), r.name.clone());
    if !r.description.is_empty() {
        res.description = Some(r.description.clone());
    }
    res.mime_type.clone_from(&r.mime_type);
    res.annotations = r.annotations.as_ref().and_then(|v| serde_json::from_value(v.clone()).ok());
    res.meta = r.meta.as_ref().and_then(|v| serde_json::from_value(v.clone()).ok());
    res
}

/// Rewrites upstream URIs in a read result back to the gateway's public URI.
fn rewrite_contents(
    mut result: ReadResourceResult,
    public: &ResourceUri,
    original: &str,
) -> ReadResourceResult {
    for c in &mut result.contents {
        match c {
            ResourceContents::TextResourceContents { uri, .. }
            | ResourceContents::BlobResourceContents { uri, .. }
                if uri == original || uri.is_empty() =>
            {
                *uri = public.to_string();
            }
            _ => {}
        }
    }
    result
}

// ---------------------------------------------------------------- the invoker (shared with REST)
/// Executes calls against upstreams through the session manager. No authorization here:
/// the MCP handler checks the `Principal`, the REST layer its own middleware.
#[derive(Debug, Clone)]
pub struct Invoker(pub Arc<GatewayState>);

impl Invoker {
    pub async fn call_tool(
        &self,
        canonical: &CanonicalName,
        arguments: Option<JsonObject>,
    ) -> Result<CallToolResult> {
        let snap = self.0.catalog.load();
        let tool = snap
            .tools
            .get(canonical)
            .filter(|t| t.enabled)
            .ok_or_else(|| Error::not_found(format!("tool '{canonical}'")))?;
        let server = snap.server(&tool.server)?;
        let started = Instant::now();
        let lease = self.0.sessions.acquire(server).await?;
        let outcome = lease.conn.call_tool(canonical.local(), arguments).await;
        self.0.sessions.release(lease, outcome.as_ref().err()).await;
        let ok = outcome.as_ref().is_ok_and(|r| r.is_error != Some(true));
        if let Some(m) = &self.0.metrics {
            m.record_call(
                server.name.as_str(),
                canonical.local(),
                ok,
                started.elapsed().as_secs_f64(),
            );
        }
        outcome
            .map_err(|e| Error::Upstream { server: server.name.to_string(), reason: e.to_string() })
    }

    pub async fn get_prompt(
        &self,
        canonical: &CanonicalName,
        arguments: Option<JsonObject>,
    ) -> Result<GetPromptResult> {
        let snap = self.0.catalog.load();
        let prompt = snap
            .prompts
            .get(canonical)
            .filter(|p| p.enabled)
            .ok_or_else(|| Error::not_found(format!("prompt '{canonical}'")))?;
        let server = snap.server(&prompt.server)?;
        let started = Instant::now();
        let lease = self.0.sessions.acquire(server).await?;
        let outcome = lease.conn.get_prompt(canonical.local(), arguments).await;
        self.0.sessions.release(lease, outcome.as_ref().err()).await;
        if let Some(m) = &self.0.metrics {
            m.record_call(
                server.name.as_str(),
                canonical.local(),
                outcome.is_ok(),
                started.elapsed().as_secs_f64(),
            );
        }
        outcome
            .map_err(|e| Error::Upstream { server: server.name.to_string(), reason: e.to_string() })
    }

    pub async fn read_resource(&self, uri: &ResourceUri) -> Result<ReadResourceResult> {
        let snap = self.0.catalog.load();
        let res = snap
            .resources
            .get(uri)
            .filter(|r| r.enabled)
            .ok_or_else(|| Error::not_found(format!("resource '{uri}'")))?;
        let server = snap.server(&res.server)?;
        let lease = self.0.sessions.acquire(server).await?;
        let outcome = lease.conn.read_resource(&res.original_uri).await;
        self.0.sessions.release(lease, outcome.as_ref().err()).await;
        outcome
            .map(|r| rewrite_contents(r, uri, &res.original_uri))
            .map_err(|e| Error::Upstream { server: server.name.to_string(), reason: e.to_string() })
    }

    /// The server a canonical/resource entry belongs to — for authorization before any call.
    #[must_use]
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.0.catalog.load()
    }
}

// ---------------------------------------------------------------- ServerHandler
impl ServerHandler for GatewayHandler {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_prompts()
            .enable_resources()
            .build();
        let name = match &self.scope {
            Scope::Global => "Trunks MCP Gateway".to_string(),
            Scope::Group(g) => format!("Trunks gateway for tool group: {g}"),
        };
        info.server_info = Implementation::new(name, self.state.version.clone());
        info
    }

    async fn list_tools(
        &self,
        _r: Option<PaginatedRequestParams>,
        ctx: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        let Some(p) = Self::principal(&ctx) else { return Ok(ListToolsResult::default()) };
        let snap = self.state.catalog.load();
        let tools = snap
            .enabled_tools()
            .filter(|t| self.in_scope(&snap, &t.canonical) && p.may_use_tool(&snap, &t.canonical))
            .map(|t| to_mcp_tool(t))
            .collect();
        Ok(ListToolsResult { tools, ..Default::default() })
    }

    async fn call_tool(
        &self,
        req: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        let Some(p) = Self::principal(&ctx) else {
            return Err(ErrorData::internal_error("not authorized", None));
        };
        let canonical = CanonicalName::parse(&req.name)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let snap = self.state.catalog.load();
        if !self.in_scope(&snap, &canonical)
            || !snap.tools.get(&canonical).is_some_and(|t| t.enabled)
        {
            return Err(ErrorData::invalid_params(format!("tool '{canonical}' not found"), None));
        }
        if !p.may_use_tool(&snap, &canonical) {
            return Err(ErrorData::internal_error(
                format!("not authorized to access MCP server {}", canonical.server()),
                None,
            ));
        }
        drop(snap);
        let result = Invoker(self.state.clone())
            .call_tool(&canonical, req.arguments)
            .await
            .map_err(|e| Self::mcp_err(&e))?;
        Ok(CallToolResponse::Complete(result))
    }

    async fn list_prompts(
        &self,
        _r: Option<PaginatedRequestParams>,
        ctx: RequestContext<RoleServer>,
    ) -> std::result::Result<ListPromptsResult, ErrorData> {
        let Some(_p) = Self::principal(&ctx) else { return Ok(ListPromptsResult::default()) };
        if matches!(self.scope, Scope::Group(_)) {
            return Ok(ListPromptsResult::default());
        }
        let snap = self.state.catalog.load();
        let prompts = snap.enabled_prompts().map(|p| to_mcp_prompt(p)).collect();
        Ok(ListPromptsResult { prompts, ..Default::default() })
    }

    async fn get_prompt(
        &self,
        req: GetPromptRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> std::result::Result<GetPromptResponse, ErrorData> {
        let Some(p) = Self::principal(&ctx) else {
            return Err(ErrorData::internal_error("not authorized", None));
        };
        let canonical = CanonicalName::parse(&req.name)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        if matches!(self.scope, Scope::Group(_)) {
            return Err(ErrorData::invalid_params(format!("prompt '{canonical}' not found"), None));
        }
        if !p.may_access(canonical.server()) {
            return Err(ErrorData::internal_error(
                format!("not authorized to access MCP server {}", canonical.server()),
                None,
            ));
        }
        Invoker(self.state.clone())
            .get_prompt(&canonical, req.arguments)
            .await
            .map(GetPromptResponse::Complete)
            .map_err(|e| Self::mcp_err(&e))
    }

    async fn list_resources(
        &self,
        _r: Option<PaginatedRequestParams>,
        ctx: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourcesResult, ErrorData> {
        let Some(_p) = Self::principal(&ctx) else { return Ok(ListResourcesResult::default()) };
        if matches!(self.scope, Scope::Group(_)) {
            return Ok(ListResourcesResult::default());
        }
        let snap = self.state.catalog.load();
        let resources = snap.enabled_resources().map(|r| to_mcp_resource(r)).collect();
        Ok(ListResourcesResult { resources, ..Default::default() })
    }

    async fn read_resource(
        &self,
        req: ReadResourceRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResponse, ErrorData> {
        let Some(p) = Self::principal(&ctx) else {
            return Err(ErrorData::internal_error("not authorized", None));
        };
        let uri = ResourceUri::parse(&req.uri)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        if matches!(self.scope, Scope::Group(_)) {
            return Err(ErrorData::invalid_params(format!("resource '{uri}' not found"), None));
        }
        if !p.may_access(uri.server()) {
            return Err(ErrorData::internal_error(
                format!("not authorized to access MCP server {}", uri.server()),
                None,
            ));
        }
        Invoker(self.state.clone())
            .read_resource(&uri)
            .await
            .map(ReadResourceResponse::Complete)
            .map_err(|e| Self::mcp_err(&e))
    }
}

/// Convenience used by tests and the REST layer.
#[must_use]
pub fn server_of(name: &CanonicalName) -> &ServerName {
    name.server()
}
