//! `/api/v0` read surface and the public routes, against the legacy fixtures (SQLite).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::too_many_lines)]

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::Value;
use tower::ServiceExt as _;
use trunks_api::AppState;
use trunks_api::mcp::{McpMounts, McpOptions};
use trunks_core::catalog::Catalog;
use trunks_core::dashboard::RuntimeInfo;
use trunks_core::gateway::GatewayState;
use trunks_core::groups::GroupService;
use trunks_core::identity::IdentityService;
use trunks_core::login::LoginService;
use trunks_core::metrics::Metrics;
use trunks_core::mode::ModeService;
use trunks_core::registry::RegistryService;
use trunks_core::sessions::{SessionManager, SessionOptions};
use trunks_core::upstream::ConnectOptions;
use trunks_db::{Db, Store};
use trunks_types::ServerMode;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

struct App {
    router: Router,
    _dir: tempfile::TempDir,
}

async fn app_from(fixture: Option<&str>, metrics: bool) -> App {
    app_with(fixture, metrics, trunks_api::HttpOptions::default()).await
}

async fn app_with(fixture: Option<&str>, metrics: bool, http: trunks_api::HttpOptions) -> App {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    if let Some(f) = fixture {
        std::fs::copy(fixtures().join(f), &path).unwrap();
    }
    let db = Db::connect(&format!("sqlite://{}", path.display()), 2).await.unwrap();
    db.migrate().await.unwrap();
    let store = Arc::new(Store(db));
    let identity = IdentityService::new(store.clone(), store.clone());
    let mode = ModeService::load(store.clone(), identity.clone()).await.unwrap();
    let catalog = Catalog::empty();
    catalog.reload(store.as_ref()).await.unwrap();
    let metrics = metrics.then(|| Metrics::new("vtest"));
    let gateway = Arc::new(GatewayState {
        catalog: catalog.clone(),
        sessions: SessionManager::new(SessionOptions::default()),
        metrics: metrics.clone(),
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
        registry: store.clone(),
        login: LoginService::new(
            None,
            identity.clone(),
            store.clone(),
            store.clone(),
            std::time::Duration::from_hours(1),
        ),
        public_url: None,
        identity,
        mode,
        metrics,
        runtime: RuntimeInfo {
            mode: ServerMode::Development,
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
    App { router: trunks_api::router_with(state, http), _dir: dir }
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()))
    };
    (status, json)
}

#[tokio::test]
async fn public_routes() {
    let app = app_from(None, true).await;
    let (s, b) = call(&app.router, "GET", "/health", None, None).await;
    assert_eq!((s, b), (StatusCode::OK, serde_json::json!({"status": "ok"})));
    let (s, b) = call(&app.router, "GET", "/metadata", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(b["version"].as_str().unwrap().starts_with('v'));
    let (s, b) = call(&app.router, "GET", "/metrics", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(b.as_str().unwrap().contains("trunks_build_info"));
}

#[tokio::test]
async fn uninitialised_api_is_403_until_init() {
    let app = app_from(None, false).await;
    let (s, b) = call(&app.router, "GET", "/api/v0/servers", None, None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(b["error"], "server is not initialized");
    let (s, _) = call(&app.router, "GET", "/metrics", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "metrics disabled → no route");

    let (s, b) =
        call(&app.router, "POST", "/init", None, Some(serde_json::json!({"mode": "development"})))
            .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["status"], "Server initialized successfully in development mode");
    assert!(b.get("admin_access_token").is_none());
    let (s, b) =
        call(&app.router, "POST", "/init", None, Some(serde_json::json!({"mode": "development"})))
            .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(b["error"].as_str().unwrap().contains("already initialized"));
    let (s, b) = call(&app.router, "GET", "/api/v0/servers", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b, serde_json::json!([]));
    let (s, _) = call(&app.router, "GET", "/api/v0/users/whoami", None, None).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "whoami is enterprise-only");
}

#[tokio::test]
async fn oversized_request_bodies_are_refused() {
    let app = app_from(None, false).await;
    let (s, _) =
        call(&app.router, "POST", "/init", None, Some(serde_json::json!({"mode": "development"})))
            .await;
    assert_eq!(s, StatusCode::OK);

    // REST: just under the limit is parsed (and refused for what it says), over it is a 413
    let group = |description: String| serde_json::json!({"name": "big", "description": description, "included_tools": ["a__b"]});
    let (s, b) = call(
        &app.router,
        "POST",
        "/api/v0/tool-groups",
        None,
        Some(group("x".repeat(trunks_api::BODY_LIMIT - 1024))),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{b}");
    let (s, b) = call(
        &app.router,
        "POST",
        "/api/v0/tool-groups",
        None,
        Some(group("x".repeat(trunks_api::BODY_LIMIT + 1))),
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(b, serde_json::json!({"error": "request body too large"}));

    // MCP: the same limit, enforced by the transport
    let mcp = |len: usize| {
        Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(header::HOST, "localhost")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .body(Body::from("x".repeat(len)))
            .unwrap()
    };
    let res = app.router.clone().oneshot(mcp(trunks_api::BODY_LIMIT - 1024)).await.unwrap();
    assert_ne!(res.status(), StatusCode::PAYLOAD_TOO_LARGE, "under the limit is read");
    let res = app.router.clone().oneshot(mcp(trunks_api::BODY_LIMIT + 1)).await.unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn a_stalled_mcp_request_body_is_abandoned() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let http = trunks_api::HttpOptions {
        body_idle_timeout: std::time::Duration::from_millis(300),
        ..trunks_api::HttpOptions::default()
    };
    let app = app_with(None, false, http).await;
    let (s, _) =
        call(&app.router, "POST", "/init", None, Some(serde_json::json!({"mode": "development"})))
            .await;
    assert_eq!(s, StatusCode::OK);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(trunks_api::serve(listener, app.router.clone(), http, std::future::pending()));

    // announce 200 bytes, send 10, then go quiet
    let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
    client
        .write_all(
            b"POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
              Accept: application/json, text/event-stream\r\nContent-Length: 200\r\n\
              Connection: close\r\n\r\n{\"jsonrpc\"",
        )
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let mut response = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(10), client.read_to_end(&mut response))
        .await
        .expect("the server must not wait for the rest of the body forever")
        .unwrap();
    assert!(started.elapsed() >= std::time::Duration::from_millis(250), "gave up too early");
    let head = String::from_utf8_lossy(&response);
    assert!(
        head.starts_with("HTTP/1.1 4") || head.starts_with("HTTP/1.1 5"),
        "expected an error response, got: {head:?}"
    );
}

#[tokio::test]
async fn init_enterprise_returns_admin_token_once() {
    let app = app_from(None, false).await;
    let (s, b) =
        call(&app.router, "POST", "/init", None, Some(serde_json::json!({"mode": "enterprise"})))
            .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["status"], "Server initialized successfully");
    let token = b["admin_access_token"].as_str().unwrap().to_owned();
    assert_eq!(token.len(), 43);
    let (s, b) = call(&app.router, "GET", "/api/v0/users/whoami", Some(&token), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["username"], "admin");
    assert_eq!(b["role"], "admin");
    assert_eq!(b["access"]["all"], true, "admins see everything");
    let (s, b) =
        call(&app.router, "POST", "/init", None, Some(serde_json::json!({"mode": "production"})))
            .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "legacy alias is not accepted: {b}");
}

#[tokio::test]
async fn dev_fixture_lists_everything_without_auth() {
    let app = app_from(Some("legacy-dev.sqlite"), false).await;
    let (s, b) = call(&app.router, "GET", "/api/v0/servers", None, None).await;
    assert_eq!(s, StatusCode::OK);
    let servers = b.as_array().unwrap();
    assert_eq!(servers.len(), 4);
    let fs = servers.iter().find(|x| x["name"] == "filesystem").unwrap();
    assert_eq!(fs["transport"], "stdio");
    assert_eq!(fs["command"], "npx");
    assert_eq!(fs["env_keys"], serde_json::json!(["LOG_LEVEL"]));
    assert!(fs.get("env").is_none(), "env values never leave the server");
    let ctx = servers.iter().find(|x| x["name"] == "context7").unwrap();
    assert!(
        ctx.get("bearer_token").is_none() && ctx.get("config").is_none(),
        "no credentials in listing"
    );
    let at = servers.iter().find(|x| x["name"] == "atlassian").unwrap();
    assert_eq!(at["header_keys"], serde_json::json!(["Authorization", "X-Tenant"]));

    let (s, b) = call(&app.router, "GET", "/api/v0/tools", None, None).await;
    assert_eq!(s, StatusCode::OK);
    let tools = b.as_array().unwrap();
    assert_eq!(tools.len(), 6);
    assert!(tools.iter().any(|t| t["name"] == "atlassian__jira__search"));
    let read = tools.iter().find(|t| t["name"] == "filesystem__read_file").unwrap();
    assert_eq!(read["enabled"], true);
    assert_eq!(read["input_schema"]["required"], serde_json::json!(["path"]));
    assert_eq!(
        tools.iter().find(|t| t["name"] == "legacy-sse__old_tool").unwrap()["enabled"],
        false
    );

    let (s, b) = call(&app.router, "GET", "/api/v0/tools?server=filesystem", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b.as_array().unwrap().len(), 2);
    let (s, b) = call(&app.router, "GET", "/api/v0/tools?server=nope", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(b["error"].as_str().unwrap().contains("not found"));
    let (s, _) = call(&app.router, "GET", "/api/v0/tools?server=bad__name", None, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, b) =
        call(&app.router, "GET", "/api/v0/tool?name=context7__get-library-docs", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["server"], "context7");
    let (s, b) = call(&app.router, "GET", "/api/v0/tool", None, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(b["error"].as_str().unwrap().contains("missing 'name' query parameter"));
    let (s, b) = call(&app.router, "GET", "/api/v0/tool?name=nosep", None, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(b["error"].as_str().unwrap().contains("does not contain a __ separator"));
    let (s, _) = call(&app.router, "GET", "/api/v0/tool?name=filesystem__nope", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, b) = call(&app.router, "GET", "/api/v0/prompts", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(b.as_array().unwrap().iter().any(|p| p["name"] == "context7__Model Details"));
    let (s, b) =
        call(&app.router, "GET", "/api/v0/prompt?name=filesystem__summarize", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["arguments"][0]["name"], "path");

    let (s, b) = call(&app.router, "GET", "/api/v0/resources", None, None).await;
    assert_eq!(s, StatusCode::OK);
    let res = b.as_array().unwrap();
    assert_eq!(res.len(), 2);
    assert!(res[0]["uri"].as_str().unwrap().starts_with("trunks://res/filesystem/"));
    assert!(res[0].get("original_uri").is_none(), "upstream URI stays internal");
}

#[tokio::test]
async fn enterprise_fixture_enforces_user_tokens() {
    let app = app_from(Some("legacy-enterprise.sqlite"), false).await;
    let (s, b) = call(&app.router, "GET", "/api/v0/tools", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(b["error"], "missing access token");
    let (s, b) = call(&app.router, "GET", "/api/v0/tools", Some("wrong"), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(b["error"].as_str().unwrap().starts_with("invalid access token"));
    // D14: a migrated user has no grants yet, so sees nothing until an admin grants access
    let (s, b) =
        call(&app.router, "GET", "/api/v0/tools", Some("bob-token-0123456789"), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b.as_array().unwrap().len(), 0, "no grants → no tools");
    let (s, b) =
        call(&app.router, "GET", "/api/v0/users/whoami", Some("bob-token-0123456789"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["username"], "bob");
    assert_eq!(b["role"], "user");
    assert_eq!(b["access"], serde_json::json!({"all": false, "servers": [], "tool_groups": []}));
    assert_eq!(b["teams"], serde_json::json!([]));
    let (s, _) =
        call(&app.router, "GET", "/api/v0/servers", Some("cursor-token-0123456789"), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "MCP client tokens do not open the REST API");
    let (s, b) =
        call(&app.router, "POST", "/init", None, Some(serde_json::json!({"mode": "enterprise"})))
            .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(b["error"], "Server is already initialized");
}
