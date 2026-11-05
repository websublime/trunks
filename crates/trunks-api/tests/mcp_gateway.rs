//! End-to-end: real HTTP server, `/mcp` + group endpoint, REST invoke, with a stdio fake upstream.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names
)]

use std::sync::Arc;

use rmcp::ServiceExt as _;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde_json::{Value, json};
use trunks_api::AppState;
use trunks_api::mcp::{McpMounts, McpOptions};
use trunks_core::catalog::Catalog;
use trunks_core::dashboard::RuntimeInfo;
use trunks_core::gateway::GatewayState;
use trunks_core::groups::GroupService;
use trunks_core::identity::IdentityService;
use trunks_core::mode::ModeService;
use trunks_core::registry::RegistryService;
use trunks_core::sessions::{SessionManager, SessionOptions};
use trunks_core::upstream::ConnectOptions;
use trunks_db::{Db, Store};
use trunks_types::ServerMode;

/// Boots a server with the fake upstream registered twice (`alpha`, `beta`) plus one group.
async fn boot(mode: ServerMode) -> (String, Option<String>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::connect(&format!("sqlite://{}/db.sqlite", dir.path().display()), 4).await.unwrap();
    db.migrate().await.unwrap();
    let bin = trunks_testing::fake_upstream_bin().to_string_lossy().into_owned();
    // JSON-encoded (Windows paths contain backslashes) and SQL-quoted.
    let config = serde_json::json!({"command": bin}).to_string().replace('\'', "''");
    // seed directly (registration via the API arrives in Phase 2b)
    let seed = |name: &str| {
        format!(
            "INSERT INTO mcp_servers (created_at, updated_at, name, transport, enabled, description, config, session_mode) VALUES ('2026-01-01 00:00:00','2026-01-01 00:00:00','{name}','stdio',1,'',json('{config}'),'stateful')"
        )
    };
    trunks_db::with_pool!(&db, |p| {
        sqlx::raw_sql(&seed("alpha")).execute(p).await.unwrap();
        sqlx::raw_sql(&seed("beta")).execute(p).await.unwrap();
        for (sid, srv) in [(1, "alpha"), (2, "beta")] {
            for t in ["echo", "echo_headers", "fail"] {
                sqlx::raw_sql(&format!("INSERT INTO tools (created_at, updated_at, name, enabled, description, server_id) VALUES ('2026-01-01 00:00:00','2026-01-01 00:00:00','{t}',1,'',{sid})")).execute(p).await.unwrap();
            }
            sqlx::raw_sql(&format!("INSERT INTO prompts (created_at, updated_at, name, enabled, description, server_id) VALUES ('2026-01-01 00:00:00','2026-01-01 00:00:00','greet',1,'',{sid})")).execute(p).await.unwrap();
            sqlx::raw_sql(&format!("INSERT INTO resources (created_at, updated_at, uri, original_uri, name, enabled, server_id) VALUES ('2026-01-01 00:00:00','2026-01-01 00:00:00','trunks://res/{srv}/bWVtOi8vaGVsbG8','mem://hello','hello',1,{sid})")).execute(p).await.unwrap();
        }
        sqlx::raw_sql("INSERT INTO tool_groups (created_at, updated_at, name, description, included_tools) VALUES ('2026-01-01 00:00:00','2026-01-01 00:00:00','only-alpha-echo','',json('[\"alpha__echo\"]'))").execute(p).await.unwrap();
        sqlx::raw_sql("INSERT INTO mcp_clients (created_at, updated_at, name, description, access_token, allow_list) VALUES ('2026-01-01 00:00:00','2026-01-01 00:00:00','cli-alpha','','tok-alpha-0123456789',json('[\"alpha\"]'))").execute(p).await.unwrap();
    });
    let store = Arc::new(Store(db));
    let identity = IdentityService::new(store.clone());
    let mode_svc = ModeService::load(store.clone(), identity.clone()).await.unwrap();
    let init = mode_svc.init(mode).await.unwrap();
    let catalog = Catalog::empty();
    catalog.reload(store.as_ref()).await.unwrap();
    let gateway = Arc::new(GatewayState {
        catalog: catalog.clone(),
        sessions: SessionManager::new(SessionOptions::default()),
        metrics: None,
        version: "vtest".into(),
    });
    let state = AppState {
        catalog,
        gateway: gateway.clone(),
        mcp: McpMounts::new(gateway.clone(), McpOptions::default()),
        registry_service: RegistryService::new(
            store.clone(),
            store.clone(),
            gateway.catalog.clone(),
            gateway.sessions.clone(),
            ConnectOptions::default(),
        ),
        groups: GroupService::new(store.clone(), gateway.catalog.clone()),
        registry: store,
        identity,
        mode: mode_svc,
        metrics: None,
        runtime: RuntimeInfo {
            mode,
            version: "vtest".into(),
            started_at: chrono::Utc::now(),
            database_backend: "sqlite".into(),
            database_url: "sqlite://test".into(),
            migrations_latest: None,
            bind_host: "127.0.0.1".into(),
            bind_port: 0,
            allowed_hosts: vec![],
            upstream_init_timeout_sec: 30,
            session_idle_timeout_sec: 0,
            metrics_enabled: false,
        },
    };
    let app = trunks_api::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), init.admin_access_token, dir)
}

async fn mcp_client(
    url: &str,
    token: Option<&str>,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let mut h = reqwest::header::HeaderMap::new();
    if let Some(t) = token {
        h.insert(reqwest::header::AUTHORIZATION, format!("Bearer {t}").parse().unwrap());
    }
    h.insert("x-downstream", "leak?".parse().unwrap());
    let rc = reqwest::Client::builder().default_headers(h).build().unwrap();
    let t = StreamableHttpClientTransport::with_client(
        rc,
        StreamableHttpClientTransportConfig::with_uri(url.to_owned()),
    );
    ().serve(t).await.unwrap()
}

fn text(r: &rmcp::model::CallToolResult) -> String {
    r.content.first().and_then(|c| c.as_text()).map(|t| t.text.clone()).unwrap_or_default()
}

#[tokio::test]
async fn dev_mode_gateway_and_rest_invoke() {
    let (base, _, _dir) = boot(ServerMode::Development).await;
    let c = mcp_client(&format!("{base}/mcp"), None).await;
    let tools = c.list_all_tools().await.unwrap();
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(
        names,
        [
            "alpha__echo",
            "alpha__echo_headers",
            "alpha__fail",
            "beta__echo",
            "beta__echo_headers",
            "beta__fail"
        ]
    );
    let mut p = CallToolRequestParams::new("beta__echo");
    p.arguments = json!({"text": "olá"}).as_object().cloned();
    assert_eq!(text(&c.call_tool(p).await.unwrap()), "olá");
    // nothing downstream reaches the upstream
    let seen = text(&c.call_tool(CallToolRequestParams::new("alpha__echo_headers")).await.unwrap());
    let seen: Vec<String> = serde_json::from_str(&seen).unwrap();
    assert!(!seen.iter().any(|h| h == "x-downstream" || h == "authorization"), "{seen:?}");
    assert_eq!(c.list_all_prompts().await.unwrap().len(), 2);
    let res = c.list_all_resources().await.unwrap();
    assert_eq!(res.len(), 2);
    assert!(res[0].uri.starts_with("trunks://res/alpha/"));
    let read = c
        .read_resource(rmcp::model::ReadResourceRequestParams::new(res[0].uri.clone()))
        .await
        .unwrap();
    match &read.contents[0] {
        rmcp::model::ResourceContents::TextResourceContents { uri, text, .. } => {
            assert_eq!(uri, &res[0].uri);
            assert_eq!(text, "hello from upstream");
        }
        other => panic!("{other:?}"),
    }
    c.cancel().await.unwrap();

    // group endpoint: tools only, only the included one
    let g = mcp_client(&format!("{base}/v0/groups/only-alpha-echo/mcp"), None).await;
    let names: Vec<String> =
        g.list_all_tools().await.unwrap().iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, ["alpha__echo"]);
    assert!(g.list_all_prompts().await.unwrap().is_empty());
    assert!(
        g.call_tool(CallToolRequestParams::new("beta__echo")).await.is_err(),
        "out-of-group tool is not callable"
    );
    g.cancel().await.unwrap();
    let http = reqwest::Client::new();
    let r = http.post(format!("{base}/v0/groups/nope/mcp")).send().await.unwrap();
    assert_eq!(r.status(), 404);
    assert_eq!(r.json::<Value>().await.unwrap()["error"], "tool group: nope not found");

    // REST invocation
    let r = http
        .post(format!("{base}/api/v0/tools/invoke"))
        .json(&json!({"name": "alpha__echo", "text": "via rest"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["content"][0]["text"], "via rest");
    assert_ne!(v["isError"], true);
    let r = http
        .post(format!("{base}/api/v0/tools/invoke"))
        .json(&json!({"name": "alpha__fail"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.json::<Value>().await.unwrap()["isError"], true);
    let r = http
        .post(format!("{base}/api/v0/tools/invoke"))
        .json(&json!({"name": "alpha__nope"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
    let r = http
        .post(format!("{base}/api/v0/tools/invoke"))
        .json(&json!({"name": "nosep"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let r = http
        .post(format!("{base}/api/v0/prompts/render"))
        .json(&json!({"name": "beta__greet", "arguments": {"name": "Ana"}}))
        .send()
        .await
        .unwrap();
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["messages"][0]["content"]["text"], "Hello, Ana!");
    let r = http
        .post(format!("{base}/api/v0/resources/read"))
        .json(&json!({"uri": "trunks://res/beta/bWVtOi8vaGVsbG8"}))
        .send()
        .await
        .unwrap();
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["contents"][0]["text"], "hello from upstream");
    assert_eq!(v["contents"][0]["uri"], "trunks://res/beta/bWVtOi8vaGVsbG8");
    let r = http
        .post(format!("{base}/api/v0/resources/get"))
        .json(&json!({"uri": "trunks://res/beta/bWVtOi8vaGVsbG8"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.json::<Value>().await.unwrap()["name"], "hello");
}

#[tokio::test]
async fn enterprise_mode_gateway_enforces_allow_lists() {
    let (base, admin, _dir) = boot(ServerMode::Enterprise).await;
    let http = reqwest::Client::new();
    let r = http.post(format!("{base}/mcp")).send().await.unwrap();
    assert_eq!(r.status(), 401);
    assert_eq!(r.json::<Value>().await.unwrap()["error"], "missing MCP client access token");
    let r = http.post(format!("{base}/mcp")).bearer_auth(admin.unwrap()).send().await.unwrap();
    assert_eq!(r.status(), 401, "user/admin tokens do not open the MCP endpoint");

    let c = mcp_client(&format!("{base}/mcp"), Some("tok-alpha-0123456789")).await;
    let names: Vec<String> =
        c.list_all_tools().await.unwrap().iter().map(|t| t.name.to_string()).collect();
    assert_eq!(
        names,
        ["alpha__echo", "alpha__echo_headers", "alpha__fail"],
        "allow-list filters listing"
    );
    let mut p = CallToolRequestParams::new("alpha__echo");
    p.arguments = json!({"text": "ok"}).as_object().cloned();
    assert_eq!(text(&c.call_tool(p).await.unwrap()), "ok");
    let err =
        c.call_tool(CallToolRequestParams::new("beta__echo")).await.err().unwrap().to_string();
    assert!(err.contains("not authorized to access MCP server beta"), "{err}");
    let err = c
        .read_resource(rmcp::model::ReadResourceRequestParams::new(
            "trunks://res/beta/bWVtOi8vaGVsbG8".to_string(),
        ))
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("not authorized"), "{err}");
    c.cancel().await.unwrap();

    // group never widens access
    let g =
        mcp_client(&format!("{base}/v0/groups/only-alpha-echo/mcp"), Some("tok-alpha-0123456789"))
            .await;
    assert_eq!(g.list_all_tools().await.unwrap().len(), 1);
    g.cancel().await.unwrap();
}
