//! Secrets never reach the logs (`docs/02-especificacao.md` §10, §11): a full enterprise
//! session — registration with credentials, identities, REST and MCP calls, browser login,
//! OAuth-style callbacks — is run with every log level enabled, and the captured output must
//! not contain any credential that went through the server.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::too_many_lines)]

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use trunks_api::AppState;
use trunks_api::mcp::{McpMounts, McpOptions};
use trunks_core::catalog::Catalog;
use trunks_core::dashboard::RuntimeInfo;
use trunks_core::gateway::GatewayState;
use trunks_core::groups::GroupService;
use trunks_core::identity::IdentityService;
use trunks_core::login::LoginService;
use trunks_core::mode::ModeService;
use trunks_core::registry::RegistryService;
use trunks_core::sessions::{SessionManager, SessionOptions};
use trunks_core::upstream::ConnectOptions;
use trunks_db::{Db, Store};
use trunks_types::ServerMode;

/// Collects everything the process logs.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Boots an enterprise server on a loopback port; returns its base URL and the admin token.
async fn boot() -> (String, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::connect(&format!("sqlite://{}/db.sqlite", dir.path().display()), 4).await.unwrap();
    db.migrate().await.unwrap();
    let store = Arc::new(Store(db));
    let identity = IdentityService::new(store.clone(), store.clone());
    let mode = ModeService::load(store.clone(), identity.clone()).await.unwrap();
    let init = mode.init(ServerMode::Enterprise).await.unwrap();
    let catalog = Catalog::empty();
    catalog.reload(store.as_ref()).await.unwrap();
    let sessions = SessionManager::new(SessionOptions::default());
    let gateway = Arc::new(GatewayState {
        catalog: catalog.clone(),
        sessions: sessions.clone(),
        metrics: None,
        version: "vtest".into(),
    });
    let state = AppState {
        catalog: catalog.clone(),
        gateway: gateway.clone(),
        mcp: McpMounts::new(gateway, McpOptions { allowed_hosts: vec!["*".into()] }),
        registry_service: RegistryService::new(
            store.clone(),
            store.clone(),
            catalog.clone(),
            sessions,
            ConnectOptions::default(),
        ),
        groups: GroupService::new(store.clone(), catalog),
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
        metrics: None,
        runtime: RuntimeInfo {
            mode: ServerMode::Enterprise,
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, trunks_api::router(state)).await.unwrap() });
    (format!("http://{addr}"), init.admin_access_token.unwrap(), dir)
}

const USER_TOKEN: &str = "hygiene-user-token-7f3a91";
const CLIENT_TOKEN: &str = "hygiene-client-token-2c80e4";
const UPSTREAM_BEARER: &str = "hygiene-upstream-bearer-91c2d7";
const UPSTREAM_HEADER: &str = "hygiene-header-value-55d0aa";
const STDIO_ENV: &str = "hygiene-env-value-0b6e13";
const OAUTH_CODE: &str = "hygiene-oauth-code-3d41f0";
const OAUTH_STATE: &str = "hygiene-oauth-state-8a7712";

#[tokio::test]
async fn no_credential_reaches_the_logs_at_any_level() {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(capture.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let (base, admin_token, _dir) = boot().await;
    let http = reqwest::Client::new();
    let post = |path: &str, token: &str, body: Value| {
        http.post(format!("{base}{path}")).bearer_auth(token).json(&body).send()
    };

    // upstreams registered with every kind of credential, then used
    let upstream = trunks_testing::serve_http().await;
    let res = post(
        "/api/v0/servers",
        &admin_token,
        json!({
            "name": "web", "transport": "streamable_http", "description": "", "url": upstream,
            "bearer_token": UPSTREAM_BEARER, "headers": {"x-api-key": UPSTREAM_HEADER},
        }),
    )
    .await
    .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let res = post(
        "/api/v0/servers",
        &admin_token,
        json!({
            "name": "local", "transport": "stdio", "description": "",
            "command": trunks_testing::fake_upstream_bin(), "env": {"API_KEY": STDIO_ENV},
        }),
    )
    .await
    .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    for tool in ["web__echo", "local__echo"] {
        let res = post(
            "/api/v0/tools/invoke",
            &admin_token,
            json!({"name": tool, "input": {"text": "hello"}}),
        )
        .await
        .unwrap();
        assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    }

    // identities with their tokens; a REST call as the user and an MCP call as the client
    let res =
        post("/api/v0/users", &admin_token, json!({"username": "ana", "access_token": USER_TOKEN}))
            .await
            .unwrap();
    assert_eq!(res.status(), 201);
    let res = post(
        "/api/v0/clients",
        &admin_token,
        json!({"name": "ide", "description": "", "allow_list": ["*"], "access_token": CLIENT_TOKEN}),
    )
    .await
    .unwrap();
    assert_eq!(res.status(), 201);
    let res = http
        .get(format!("{base}/api/v0/users/whoami"))
        .bearer_auth(USER_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let res = http
        .post(format!("{base}/mcp"))
        .bearer_auth(CLIENT_TOKEN)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                       "clientInfo": {"name": "hygiene", "version": "0"}},
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
    // wrong credentials are not echoed either
    let res = http
        .get(format!("{base}/api/v0/servers"))
        .bearer_auth("hygiene-wrong-token-64aa0c")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);

    // browser login: the session cookie is a credential too
    let res = http
        .post(format!("{base}/auth/token"))
        .json(&json!({"access_token": USER_TOKEN}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let cookie = res.headers()["set-cookie"].to_str().unwrap().to_owned();
    let session = cookie.split(';').next().unwrap().split_once('=').unwrap().1.to_owned();
    let res = http
        .get(format!("{base}/api/dashboard/overview"))
        .header("cookie", format!("trunks_session={session}"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    // authorization codes and states arrive in query strings
    for path in ["/auth/callback", "/api/dashboard/oauth/callback"] {
        http.get(format!("{base}{path}?code={OAUTH_CODE}&state={OAUTH_STATE}"))
            .send()
            .await
            .unwrap();
    }

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let logs = String::from_utf8_lossy(&capture.0.lock().unwrap()).into_owned();

    // the capture works and the access log is there, without query strings
    assert!(logs.contains("path=/api/v0/tools/invoke"), "access log missing:\n{logs}");
    assert!(logs.contains("principal=\"admin\""), "principal kind missing from the access log");
    assert!(logs.contains("principal=\"mcp_client\""));
    assert!(logs.contains("path=/auth/callback"));

    let secrets: BTreeMap<&str, &str> = BTreeMap::from([
        ("admin token", admin_token.as_str()),
        ("user token", USER_TOKEN),
        ("MCP client token", CLIENT_TOKEN),
        ("rejected token", "hygiene-wrong-token-64aa0c"),
        ("upstream bearer", UPSTREAM_BEARER),
        ("upstream header value", UPSTREAM_HEADER),
        ("stdio env value", STDIO_ENV),
        ("session cookie", session.as_str()),
        ("authorization code", OAUTH_CODE),
        ("OAuth state", OAUTH_STATE),
    ]);
    let leaked: Vec<String> = secrets
        .iter()
        .filter(|(_, value)| logs.contains(**value))
        .map(|(what, value)| {
            let line = logs.lines().find(|l| l.contains(*value)).unwrap_or_default();
            format!("{what}: {}", line.chars().take(300).collect::<String>())
        })
        .collect();
    assert!(leaked.is_empty(), "credentials found in the logs:\n{}", leaked.join("\n"));
}
