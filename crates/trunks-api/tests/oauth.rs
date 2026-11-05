//! Upstream OAuth end-to-end: a fake authorization server (RFC 8414 metadata, RFC 7591 DCR,
//! token endpoint) and a protected MCP upstream (401 + `WWW-Authenticate`, RFC 9728 metadata),
//! both on wiremock, against the real HTTP server and the SDK.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::too_many_lines)]

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use secrecy::SecretString;
use serde_json::json;
use trunks_api::AppState;
use trunks_api::mcp::{McpMounts, McpOptions};
use trunks_client::Client;
use trunks_core::catalog::Catalog;
use trunks_core::dashboard::RuntimeInfo;
use trunks_core::gateway::GatewayState;
use trunks_core::groups::GroupService;
use trunks_core::identity::IdentityService;
use trunks_core::mode::ModeService;
use trunks_core::ports::OAuthStore as _;
use trunks_core::registry::RegistryService;
use trunks_core::sessions::{SessionManager, SessionOptions};
use trunks_core::upstream::ConnectOptions;
use trunks_db::{Db, Store};
use trunks_testing::oauth::Fake;
use trunks_types::api::RegisterServerInput;
use trunks_types::{ServerMode, ServerName, SessionMode, Transport};

async fn boot() -> (Client, Arc<Store>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::connect(&format!("sqlite://{}/db.sqlite", dir.path().display()), 4).await.unwrap();
    db.migrate().await.unwrap();
    let store = Arc::new(Store(db));
    let identity = IdentityService::new(store.clone(), store.clone());
    let mode_svc = ModeService::load(store.clone(), identity.clone()).await.unwrap();
    mode_svc.init(ServerMode::Development).await.unwrap();
    let catalog = Catalog::empty();
    catalog.reload(store.as_ref()).await.unwrap();
    let connect = ConnectOptions { oauth: Some(store.clone()), ..ConnectOptions::default() };
    let sessions = SessionManager::new(SessionOptions {
        connect: connect.clone(),
        ..SessionOptions::default()
    });
    let gateway = Arc::new(GatewayState {
        catalog: catalog.clone(),
        sessions: sessions.clone(),
        metrics: None,
        version: "vtest".into(),
    });
    let state = AppState {
        catalog: catalog.clone(),
        gateway: gateway.clone(),
        mcp: McpMounts::new(gateway, McpOptions::default()),
        registry_service: RegistryService::new(
            store.clone(),
            store.clone(),
            catalog.clone(),
            sessions,
            connect,
        ),
        groups: GroupService::new(store.clone(), catalog),
        registry: store.clone(),
        identity,
        mode: mode_svc,
        metrics: None,
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
    let app = trunks_api::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (Client::new(&format!("http://{addr}"), None, reqwest::Client::new()).unwrap(), store, dir)
}

fn http_input(name: &str, url: &str, redirect: Option<&str>) -> RegisterServerInput {
    RegisterServerInput {
        name: name.into(),
        transport: Transport::StreamableHttp,
        description: "protected".into(),
        url: Some(url.into()),
        bearer_token: None,
        headers: BTreeMap::new(),
        command: None,
        args: vec![],
        env: BTreeMap::new(),
        session_mode: SessionMode::Stateless,
        oauth_redirect_uri: redirect.map(str::to_owned),
        oauth_client_id: None,
        oauth_client_secret: None,
        oauth_scopes: vec!["mcp:use".into()],
    }
}

fn state_of(authorization_url: &str) -> String {
    url::Url::parse(authorization_url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.into_owned())
        .unwrap()
}

fn status(e: &trunks_client::Error) -> u16 {
    match e {
        trunks_client::Error::Api { status, .. } | trunks_client::Error::Status { status, .. } => {
            *status
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[tokio::test]
async fn oauth_two_phase_registration_with_dcr_then_calls_use_the_bearer() {
    let (admin, store, _dir) = boot().await;
    let fake = Fake::start(true, 3600).await;
    let url = fake.mcp_url();

    // no redirect → 400 with the machine-readable code the CLI keys on
    let e = admin.register_server(&http_input("prot", &url, None), false).await.unwrap_err();
    assert_eq!(status(&e), 400);
    assert_eq!(e.code(), Some("upstream_oauth_required"));
    assert!(admin.list_servers().await.unwrap().is_empty());

    // phase 1: 202 with the authorization URL; DCR happened once; pending session persisted
    let r = admin
        .register_server(
            &http_input("prot", &url, Some("http://127.0.0.1:1/oauth/callback")),
            false,
        )
        .await
        .unwrap();
    assert!(r.server.is_none());
    let auth = r.authorization_required.unwrap();
    assert_eq!(auth.session_id.len(), 64);
    let u = url::Url::parse(&auth.authorization_url).unwrap();
    let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
    assert_eq!(q.get("client_id").map(String::as_str), Some("dcr-client-123"));
    assert_eq!(q.get("code_challenge_method").map(String::as_str), Some("S256"));
    assert_eq!(fake.register_calls().await, 1);
    let pending = store.pending(&auth.session_id).await.unwrap().unwrap();
    assert_eq!(pending.server_name.as_str(), "prot");
    assert!(pending.expires_at > chrono::Utc::now());
    assert!(admin.list_servers().await.unwrap().is_empty(), "nothing registered yet");
    let state = state_of(&auth.authorization_url);

    // phase 2 errors: unknown session, wrong state (no token request), bad code (session kept)
    assert_eq!(status(&admin.complete_oauth("0000", "good-code", &state).await.unwrap_err()), 404);
    let e = admin.complete_oauth(&auth.session_id, "good-code", "wrong").await.unwrap_err();
    assert_eq!(status(&e), 400);
    assert!(e.to_string().contains("state"));
    assert!(fake.token_calls.snapshot().is_empty());
    let e = admin.complete_oauth(&auth.session_id, "bad-code", &state).await.unwrap_err();
    assert_eq!(status(&e), 400);
    assert!(
        store.pending(&auth.session_id).await.unwrap().is_some(),
        "a failed exchange keeps the session"
    );

    // phase 2: exchange + registration with discovery through the bearer
    let r = admin.complete_oauth(&auth.session_id, "good-code", &state).await.unwrap();
    let server = r.server.unwrap();
    assert_eq!(server.name.as_str(), "prot");
    assert!(store.pending(&auth.session_id).await.unwrap().is_none(), "session consumed");
    let creds = store.credentials(&ServerName::parse("prot").unwrap()).await.unwrap().unwrap();
    assert!(creds.has_tokens());
    assert_eq!(creds.client_id.as_deref(), Some("dcr-client-123"));
    assert!(creds.expires_at.is_some());
    let tools: Vec<String> =
        admin.list_tools(Some("prot")).await.unwrap().iter().map(|t| t.name.to_string()).collect();
    assert_eq!(tools, ["prot__search"]);

    // the export never shows OAuth tokens; the sanitized server view has no secrets
    let cfgs = admin.server_configs().await.unwrap();
    assert!(!serde_json::to_string(&cfgs).unwrap().contains("at-1"));

    // gateway calls use the stored bearer (session manager path), transparently
    let out = admin
        .invoke_tool("prot__search", json!({"q": "x"}).as_object().cloned().unwrap())
        .await
        .unwrap();
    assert_eq!(out.content[0]["text"], "secret result for x");
    assert!(fake.bearers.snapshot().iter().any(|b| b == "Bearer at-1"));

    // deregister removes credentials and pending sessions
    admin.deregister_server("prot").await.unwrap();
    assert!(store.credentials(&ServerName::parse("prot").unwrap()).await.unwrap().is_none());
}

#[tokio::test]
async fn oauth_pre_registered_client_skips_dcr_and_refreshes_expired_tokens() {
    let (admin, store, _dir) = boot().await;
    let fake = Fake::start(false, 1).await; // no DCR endpoint; 1 s tokens force a refresh
    let url = fake.mcp_url();

    // without a client id the AS cannot register us → actionable 400
    let e = admin
        .register_server(&http_input("prot", &url, Some("http://127.0.0.1:1/cb")), false)
        .await
        .unwrap_err();
    assert_eq!(status(&e), 400);
    assert!(e.to_string().contains("oauth_client_id"), "{e}");

    let mut input = http_input("prot", &url, Some("http://127.0.0.1:1/cb"));
    input.oauth_client_id = Some("pre-registered".into());
    input.oauth_client_secret = Some("s3cr3t".into());
    let r = admin.register_server(&input, false).await.unwrap();
    let auth = r.authorization_required.unwrap();
    assert!(auth.authorization_url.contains("client_id=pre-registered"));
    assert_eq!(fake.register_calls().await, 0);
    let state = state_of(&auth.authorization_url);
    let server =
        admin.complete_oauth(&auth.session_id, "good-code", &state).await.unwrap().server.unwrap();
    assert_eq!(server.name.as_str(), "prot");
    let creds = store.credentials(&ServerName::parse("prot").unwrap()).await.unwrap().unwrap();
    assert_eq!(creds.client_id.as_deref(), Some("pre-registered"));
    assert!(creds.client_secret.is_some(), "the secret survives the token save");

    // the 1 s token is inside the refresh buffer: the next call refreshes and uses at-2
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let out = admin
        .invoke_tool("prot__search", json!({"q": "y"}).as_object().cloned().unwrap())
        .await
        .unwrap();
    assert_eq!(out.content[0]["text"], "secret result for y");
    assert!(fake.token_calls.snapshot().iter().any(|g| g == "refresh_token"));
    assert!(fake.bearers.snapshot().iter().any(|b| b == "Bearer at-2"));
    let creds = store.credentials(&ServerName::parse("prot").unwrap()).await.unwrap().unwrap();
    assert_eq!(
        creds.refresh_token.map(|t| secrecy::ExposeSecret::expose_secret(&t).to_owned()).as_deref(),
        Some("rt-2"),
        "refreshed credentials persisted"
    );
    let _ = SecretString::from(String::new());
}

// ---- Phase 6a: dashboard reads + browser callback

#[tokio::test]
async fn dashboard_oauth_callback_completes_and_polling_sees_it() {
    let (admin, store, _dir) = boot().await;
    let fake = Fake::start(true, 3600).await;
    let base = admin.base_url().to_string().trim_end_matches('/').to_owned();
    let http = reqwest::Client::new();
    let callback = format!("{base}/api/dashboard/oauth/callback");

    let r = admin
        .register_server(&http_input("prot", &fake.mcp_url(), Some(&callback)), false)
        .await
        .unwrap();
    let auth = r.authorization_required.unwrap();
    let state = state_of(&auth.authorization_url);

    // polling: pending
    let st: serde_json::Value = http
        .get(format!("{base}/api/dashboard/oauth/session/{}", auth.session_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(st["status"], "pending");
    assert_eq!(st["server_name"], "prot");
    // dashboard servers view shows nothing yet (not registered), diagnostics lists the pending session
    let diag: serde_json::Value = http
        .get(format!("{base}/api/dashboard/diagnostics"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(diag["pending_oauth"][0]["server_name"], "prot");

    // the browser is redirected to the callback: the backend completes everything
    let res = http.get(format!("{callback}?code=good-code&state={state}")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(html.contains("Authorization received") && html.contains("prot"), "{html}");
    let st: serde_json::Value = http
        .get(format!("{base}/api/dashboard/oauth/session/{}", auth.session_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(st["status"], "completed");
    assert!(store.pending(&auth.session_id).await.unwrap().is_none());

    // sanitized views: the server shows as authorized, no token anywhere
    let servers: serde_json::Value = http
        .get(format!("{base}/api/dashboard/servers"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(servers[0]["name"], "prot");
    assert_eq!(servers[0]["oauth"]["status"], "authorized");
    assert_eq!(servers[0]["counts"]["tools"], 1);
    let text = servers.to_string();
    assert!(
        !text.contains("at-1") && !text.contains("rt-1") && !text.contains("dcr-client"),
        "{text}"
    );
    let detail: serde_json::Value = http
        .get(format!("{base}/api/dashboard/servers/prot"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["tools"][0]["name"], "prot__search");

    // callback error paths render a page, never a 500
    let res = http
        .get(format!("{callback}?error=access_denied&error_description=nope"))
        .send()
        .await
        .unwrap();
    assert!(res.text().await.unwrap().contains("access_denied"));
    let res = http.get(format!("{callback}?code=x&state=unknown")).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert!(res.text().await.unwrap().contains("Authorization failed"));
    let st: serde_json::Value = http
        .get(format!("{base}/api/dashboard/oauth/session/nope"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(st["status"], "unknown");
}
