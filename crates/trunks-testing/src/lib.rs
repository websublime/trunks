#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_panics_doc)]

//! A small, deterministic upstream MCP server for tests.
//!
//! Tools: `echo` (returns its `text` argument), `echo_headers` (returns the HTTP headers the
//! upstream saw — proves nothing downstream leaks through the gateway), `fail` (returns an
//! MCP tool error). Prompt: `greet(name)`. Resource: `mem://hello`.
//!
//! With `FAKE_UPSTREAM_EXTRA_TOOLS=<n>` in its environment it also lists `n` generated tools
//! (`bulk_000`, `bulk_001`, …) with a realistic description and input schema, to build large
//! catalogs for the benchmarks. They are listed only; calling one is an error.

pub mod oauth;
pub mod oidc;

use std::net::SocketAddr;
use std::sync::Arc;

use rmcp::RoleServer;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData,
    GetPromptRequestParams, GetPromptResponse, GetPromptResult, Implementation, JsonObject,
    ListPromptsResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams, Prompt,
    PromptArgument, PromptMessage, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceContents, Role, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

#[derive(Debug, Clone, Default)]
pub struct FakeUpstream;

fn schema(props: &serde_json::Value) -> Arc<JsonObject> {
    Arc::new(
        serde_json::json!({"type": "object", "properties": props}).as_object().cloned().unwrap(),
    )
}

impl ServerHandler for FakeUpstream {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_prompts()
            .enable_resources()
            .build();
        info.server_info = Implementation::new("fake-upstream", "0.0.0");
        info
    }

    async fn list_tools(
        &self,
        _r: Option<PaginatedRequestParams>,
        _c: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut echo = Tool::new(
            "echo",
            "Echoes `text`, after `delay_ms` milliseconds when given",
            schema(
                &serde_json::json!({"text": {"type": "string"}, "delay_ms": {"type": "integer"}}),
            ),
        );
        echo.annotations = serde_json::from_value(serde_json::json!({"readOnlyHint": true})).ok();
        let mut tools = vec![
            echo,
            Tool::new(
                "echo_headers",
                "Returns the HTTP headers this upstream received",
                schema(&serde_json::json!({})),
            ),
            Tool::new("fail", "Always returns a tool error", schema(&serde_json::json!({}))),
        ];
        let extra = std::env::var("FAKE_UPSTREAM_EXTRA_TOOLS")
            .ok()
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0);
        tools.extend((0..extra).map(|i| {
            Tool::new(
                format!("bulk_{i:03}"),
                format!(
                    "Generated tool {i}: searches the records of collection {i} and returns \
                     the matching entries, most recent first"
                ),
                schema(&serde_json::json!({
                    "query": {"type": "string", "description": "Full-text query"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100},
                    "cursor": {"type": "string", "description": "Opaque pagination cursor"},
                    "include_archived": {"type": "boolean"},
                })),
            )
        }));
        Ok(ListToolsResult { tools, ..Default::default() })
    }

    async fn call_tool(
        &self,
        req: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let result = match &*req.name {
            "echo" => {
                let delay = req
                    .arguments
                    .as_ref()
                    .and_then(|a| a.get("delay_ms"))
                    .and_then(serde_json::Value::as_u64);
                if let Some(ms) = delay {
                    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                }
                let text = req
                    .arguments
                    .as_ref()
                    .and_then(|a| a.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                CallToolResult::success(vec![ContentBlock::text(text)])
            }
            "echo_headers" => {
                let headers: Vec<String> = ctx
                    .extensions
                    .get::<http::request::Parts>()
                    .map(|p| p.headers.keys().map(|k| k.as_str().to_lowercase()).collect())
                    .unwrap_or_default();
                CallToolResult::success(vec![ContentBlock::text(
                    serde_json::json!(headers).to_string(),
                )])
            }
            "fail" => CallToolResult::error(vec![ContentBlock::text("boom")]),
            other => return Err(ErrorData::invalid_params(format!("unknown tool {other}"), None)),
        };
        Ok(CallToolResponse::Complete(result))
    }

    async fn list_prompts(
        &self,
        _r: Option<PaginatedRequestParams>,
        _c: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let mut arg = PromptArgument::new("name");
        arg.description = Some("Who to greet".into());
        arg.required = Some(true);
        let prompt = Prompt::new("greet", Some("Greets someone"), Some(vec![arg]));
        Ok(ListPromptsResult { prompts: vec![prompt], ..Default::default() })
    }

    async fn get_prompt(
        &self,
        req: GetPromptRequestParams,
        _c: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        if req.name != "greet" {
            return Err(ErrorData::invalid_params(format!("unknown prompt {}", req.name), None));
        }
        let name = req
            .arguments
            .as_ref()
            .and_then(|a| a.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("world")
            .to_owned();
        let mut r = GetPromptResult::new(vec![PromptMessage::new_text(
            Role::User,
            format!("Hello, {name}!"),
        )]);
        r.description = Some("A greeting".into());
        Ok(GetPromptResponse::Complete(r))
    }

    async fn list_resources(
        &self,
        _r: Option<PaginatedRequestParams>,
        _c: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let mut res = Resource::new("mem://hello", "hello");
        res.mime_type = Some("text/plain".into());
        Ok(ListResourcesResult { resources: vec![res], ..Default::default() })
    }

    async fn read_resource(
        &self,
        req: ReadResourceRequestParams,
        _c: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if req.uri != "mem://hello" {
            return Err(ErrorData::invalid_params(format!("unknown resource {}", req.uri), None));
        }
        let r = ReadResourceResult::new(vec![ResourceContents::text(
            "hello from upstream",
            "mem://hello",
        )]);
        Ok(ReadResourceResponse::Complete(r))
    }
}

/// Serves the fake upstream over Streamable HTTP on a random loopback port. Returns the MCP URL.
pub async fn serve_http() -> String {
    let mut config = StreamableHttpServerConfig::default();
    config.legacy_session_mode = true;
    let svc = StreamableHttpService::new(
        || Ok(FakeUpstream),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let app = axum::Router::new().nest_service("/mcp", svc);
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await.unwrap();
    let addr = listener.local_addr().unwrap();
    // No Nagle delay on the upstream's side either: it would add ~40 ms to every answer and
    // drown what the tests and the benchmarks measure.
    let listener = axum::serve::ListenerExt::tap_io(listener, |tcp| {
        let _ = tcp.set_nodelay(true);
    });
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/mcp")
}

/// Path of the `fake-upstream` binary (`target/<profile>/fake-upstream`).
///
/// `cargo test` only builds the binaries of the package under test, so when another crate's
/// tests need it the binary is built on demand (once per test process).
pub fn fake_upstream_bin() -> std::path::PathBuf {
    static BIN: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    BIN.get_or_init(|| {
        let exe = std::env::current_exe().unwrap();
        // target/<profile>/deps/<test-bin>  →  target/<profile>/fake-upstream
        let dir = exe.parent().unwrap();
        let dir = if dir.ends_with("deps") { dir.parent().unwrap() } else { dir };
        let path = dir.join(if cfg!(windows) { "fake-upstream.exe" } else { "fake-upstream" });
        if !path.exists() {
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let mut cmd = std::process::Command::new(cargo);
            cmd.args(["build", "--package", "trunks-testing", "--bin", "fake-upstream"]);
            if dir.ends_with("release") {
                cmd.arg("--release");
            }
            let status = cmd.status().expect("failed to run cargo build for fake-upstream");
            assert!(status.success(), "building fake-upstream failed");
            assert!(path.exists(), "fake-upstream not found at {}", path.display());
        }
        path
    })
    .clone()
}
