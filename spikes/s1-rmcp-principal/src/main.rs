//! Spike S1 — can a `GatewayHandler` behind `rmcp::StreamableHttpService` see a per-request
//! `Principal` produced by axum middleware, in both legacy-session and stateless modes?
//!
//! Run: `cargo run` (starts two servers, runs the probes, exits non-zero on any failed check).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
use rmcp::RoleServer;
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::{Value, json};

// ----------------------------------------------------------------------------- principal
#[derive(Clone, Debug)]
enum Principal {
    DevOpen,
    Client { name: String, allow: Vec<String> },
}

impl Principal {
    fn may(&self, server: &str) -> bool {
        match self {
            Principal::DevOpen => true,
            Principal::Client { allow, .. } => allow.iter().any(|a| a == "*" || a == server),
        }
    }
    fn label(&self) -> String {
        match self {
            Principal::DevOpen => "dev".into(),
            Principal::Client { name, .. } => name.clone(),
        }
    }
}

/// Fake token table: token → client.
fn lookup(token: &str) -> Option<Principal> {
    match token {
        "tok-alpha" => Some(Principal::Client { name: "alpha".into(), allow: vec!["fs".into()] }),
        "tok-root" => Some(Principal::Client { name: "root".into(), allow: vec!["*".into()] }),
        _ => None,
    }
}

async fn mcp_client_auth(enterprise: bool, mut req: Request, next: Next) -> Response {
    let principal = if !enterprise {
        Principal::DevOpen
    } else {
        let Some(token) = bearer(req.headers()) else {
            return (StatusCode::UNAUTHORIZED, "missing MCP client access token").into_response();
        };
        match lookup(&token) {
            Some(p) => p,
            None => return (StatusCode::UNAUTHORIZED, "invalid MCP client token").into_response(),
        }
    };
    req.extensions_mut().insert(principal);
    next.run(req).await
}

fn bearer(h: &HeaderMap) -> Option<String> {
    h.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ").map(str::to_owned)
}

// ----------------------------------------------------------------------------- handler
#[derive(Clone)]
struct Gateway {
    /// (canonical name, server)
    catalog: Arc<Vec<(String, String)>>,
}

impl Gateway {
    fn principal(ctx: &RequestContext<RoleServer>) -> Option<Principal> {
        ctx.extensions.get::<http::request::Parts>()?.extensions.get::<Principal>().cloned()
    }
}

impl ServerHandler for Gateway {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = Implementation::new("spike-s1-gateway", "0.0.0");
        info
    }

    async fn list_tools(
        &self,
        _r: Option<PaginatedRequestParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        // Fail closed: no principal → no tools.
        let Some(p) = Self::principal(&ctx) else { return Ok(ListToolsResult::default()) };
        let tools = self
            .catalog
            .iter()
            .filter(|(_, server)| p.may(server))
            .map(|(name, _)| Tool::new(name.clone(), "spike tool", Arc::new(serde_json::Map::new())))
            .collect();
        Ok(ListToolsResult { tools, ..Default::default() })
    }

    async fn call_tool(
        &self,
        req: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let Some(p) = Self::principal(&ctx) else {
            return Err(ErrorData::internal_error("not authorized", None));
        };
        let server = req.name.split_once("__").map(|(s, _)| s).unwrap_or("");
        if !p.may(server) {
            return Err(ErrorData::internal_error(format!("not authorized to access MCP server {server}"), None));
        }
        Ok(CallToolResponse::Complete(CallToolResult::success(vec![ContentBlock::text(format!(
            "called {} as {}",
            req.name,
            p.label()
        ))])))
    }
}

fn router(enterprise: bool, legacy_session_mode: bool) -> Router {
    let catalog = Arc::new(vec![("fs__read".to_string(), "fs".to_string()), ("gh__search".to_string(), "gh".to_string())]);
    let mut config = StreamableHttpServerConfig::default();
    config.legacy_session_mode = legacy_session_mode;
    let svc = StreamableHttpService::new(
        move || Ok(Gateway { catalog: catalog.clone() }),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    Router::new()
        .nest_service("/mcp", svc)
        .layer(middleware::from_fn(move |req, next| mcp_client_auth(enterprise, req, next)))
}

// ----------------------------------------------------------------------------- probes (raw HTTP)
struct Probe {
    http: reqwest::Client,
    url: String,
}

#[derive(Debug)]
struct Reply {
    status: u16,
    session: Option<String>,
    body: Value,
}

impl Probe {
    async fn rpc(&self, token: Option<&str>, session: Option<&str>, body: Value) -> anyhow::Result<Reply> {
        let mut r = self
            .http
            .post(&self.url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-06-18");
        if let Some(t) = token {
            r = r.bearer_auth(t);
        }
        if let Some(s) = session {
            r = r.header("mcp-session-id", s);
        }
        let res = r.json(&body).send().await?;
        let status = res.status().as_u16();
        let session = res.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()).map(str::to_owned);
        let ct = res.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
        let text = res.text().await?;
        let body = if ct.starts_with("text/event-stream") {
            // take the last `data:` line that parses as JSON
            text.lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
                .last()
                .unwrap_or(Value::Null)
        } else if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        Ok(Reply { status, session, body })
    }

    async fn initialize(&self, token: Option<&str>) -> anyhow::Result<Reply> {
        let r = self
            .rpc(token, None, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}))
            .await?;
        if r.status == 200 {
            // initialized notification (needed in legacy session mode)
            let _ = self.rpc(token, r.session.as_deref(), json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await?;
        }
        Ok(r)
    }

    async fn tools(&self, token: Option<&str>, session: Option<&str>) -> anyhow::Result<(u16, Vec<String>)> {
        let r = self.rpc(token, session, json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await?;
        let names = r.body["result"]["tools"].as_array().map(|a| a.iter().filter_map(|t| t["name"].as_str().map(str::to_owned)).collect()).unwrap_or_default();
        Ok((r.status, names))
    }

    async fn call(&self, token: Option<&str>, session: Option<&str>, name: &str) -> anyhow::Result<String> {
        let r = self.rpc(token, session, json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":name,"arguments":{}}})).await?;
        Ok(r.body["result"]["content"][0]["text"].as_str().map(str::to_owned).unwrap_or_else(|| r.body["error"]["message"].as_str().unwrap_or("?").to_owned()))
    }
}

async fn serve(enterprise: bool, legacy: bool, port: u16) -> anyhow::Result<()> {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let l = tokio::net::TcpListener::bind(addr).await?;
    tokio::spawn(async move { axum::serve(l, router(enterprise, legacy)).await.unwrap() });
    Ok(())
}

macro_rules! check {
    ($name:expr, $cond:expr) => {{
        let ok = $cond;
        println!("  [{}] {}", if ok { "ok" } else { "FAIL" }, $name);
        ok
    }};
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into())).init();
    // enterprise servers in both session modes, plus a dev-mode one
    serve(true, true, 18081).await?;
    serve(true, false, 18082).await?;
    serve(false, true, 18083).await?;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let http = reqwest::Client::new();
    let mut all = true;

    for (label, port) in [("legacy_session_mode=true", 18081u16), ("legacy_session_mode=false (stateless)", 18082)] {
        println!("== enterprise, {label}");
        let p = Probe { http: http.clone(), url: format!("http://127.0.0.1:{port}/mcp") };
        // no token → 401 before reaching the handler
        let r = p.initialize(None).await?;
        all &= check!("initialize without token → 401", r.status == 401);
        // alpha: only fs
        let r = p.initialize(Some("tok-alpha")).await?;
        all &= check!("initialize with token → 200", r.status == 200);
        let sess = r.session.clone();
        println!("     session id: {:?}", sess);
        let (_, t) = p.tools(Some("tok-alpha"), sess.as_deref()).await?;
        all &= check!("alpha sees only fs__read", t == vec!["fs__read".to_string()]);
        let c = p.call(Some("tok-alpha"), sess.as_deref(), "fs__read").await?;
        all &= check!("alpha calls fs__read as alpha", c == "called fs__read as alpha");
        let c = p.call(Some("tok-alpha"), sess.as_deref(), "gh__search").await?;
        all &= check!("alpha denied on gh__search", c.contains("not authorized"));
        // THE TRAP: same session, different token → principal must follow the request, not the session
        let (_, t) = p.tools(Some("tok-root"), sess.as_deref()).await?;
        all &= check!("root token on alpha's session sees both tools (per-request principal)", t.len() == 2);
        let c = p.call(Some("tok-root"), sess.as_deref(), "gh__search").await?;
        all &= check!("root calls gh__search as root on the same session", c == "called gh__search as root");
        // invalid token on existing session → 401
        let (st, _) = p.tools(Some("tok-nope"), sess.as_deref()).await?;
        all &= check!("invalid token on existing session → 401", st == 401);
    }

    println!("== development mode (no auth)");
    let p = Probe { http: http.clone(), url: "http://127.0.0.1:18083/mcp".into() };
    let r = p.initialize(None).await?;
    let (_, t) = p.tools(None, r.session.as_deref()).await?;
    all &= check!("dev mode lists everything without a token", t.len() == 2);

    println!("== rmcp client transport (real SDK client) against legacy_session_mode=true");
    {
        use rmcp::ServiceExt as _;
        use rmcp::transport::StreamableHttpClientTransport;
        use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(reqwest::header::AUTHORIZATION, "Bearer tok-root".parse()?);
        let rc = reqwest::Client::builder().default_headers(h).build()?;
        let cfg = StreamableHttpClientTransportConfig::with_uri("http://127.0.0.1:18081/mcp");
        let t = StreamableHttpClientTransport::with_client(rc, cfg);
        let client = ().serve(t).await?;
        let tools = client.list_all_tools().await?;
        all &= check!("rmcp client: initialize + list_all_tools (root sees 2)", tools.len() == 2);
        let r = client.call_tool(CallToolRequestParams::new("gh__search")).await?;
        let txt = r.content.first().and_then(|c| c.as_text()).map(|t| t.text.clone()).unwrap_or_default();
        all &= check!("rmcp client: call_tool routed with principal", txt == "called gh__search as root");
        client.cancel().await?;
    }

    if let Ok(bin) = std::env::var("MCP_REMOTE_BIN") {
        println!("== mcp-remote (Node bridge used by Claude Desktop) → legacy_session_mode=true, token root");
        use rmcp::ServiceExt as _;
        use rmcp::transport::TokioChildProcess;
        let mut cmd = tokio::process::Command::new(bin);
        cmd.args(["http://127.0.0.1:18081/mcp", "--allow-http", "--transport", "http-only", "--header", "Authorization: Bearer tok-root"]);
        cmd.stderr(std::process::Stdio::null());
        let t = TokioChildProcess::new(cmd)?;
        let client = tokio::time::timeout(std::time::Duration::from_secs(60), ().serve(t)).await??;
        let tools = client.list_all_tools().await?;
        all &= check!("mcp-remote: list tools (root sees 2)", tools.len() == 2);
        let r = client.call_tool(CallToolRequestParams::new("fs__read")).await?;
        let txt = r.content.first().and_then(|c| c.as_text()).map(|t| t.text.clone()).unwrap_or_default();
        all &= check!("mcp-remote: call_tool routed with principal", txt == "called fs__read as root");
        client.cancel().await?;
    }

    println!();
    println!("{}", if all { "S1: ALL CHECKS PASSED" } else { "S1: FAILURES" });
    std::process::exit(if all { 0 } else { 1 });
}
