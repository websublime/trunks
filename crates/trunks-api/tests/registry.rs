//! Registry read-write through the real HTTP server and the `trunks-client` SDK, with the
//! stdio fake upstream: register (discovery, duplicates, force), enable/disable semantics,
//! config export, deregister, and the admin gate in enterprise mode.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::too_many_lines)]

use std::collections::BTreeMap;
use std::sync::Arc;

use secrecy::SecretString;
use trunks_api::AppState;
use trunks_api::mcp::{McpMounts, McpOptions};
use trunks_client::Client;
use trunks_core::catalog::Catalog;
use trunks_core::gateway::GatewayState;
use trunks_core::identity::IdentityService;
use trunks_core::mode::ModeService;
use trunks_core::registry::RegistryService;
use trunks_core::sessions::{SessionManager, SessionOptions};
use trunks_core::upstream::ConnectOptions;
use trunks_db::{Db, Store};
use trunks_types::api::RegisterServerInput;
use trunks_types::{ServerMode, SessionMode, Transport};

/// Boots an empty server; returns a client (admin token in enterprise mode) and the base URL.
async fn boot(mode: ServerMode) -> (Client, Client, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::connect(&format!("sqlite://{}/db.sqlite", dir.path().display()), 4).await.unwrap();
    db.migrate().await.unwrap();
    let store = Arc::new(Store(db));
    let identity = IdentityService::new(store.clone());
    let mode_svc = ModeService::load(store.clone(), identity.clone()).await.unwrap();
    let init = mode_svc.init(mode).await.unwrap();
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
        mcp: McpMounts::new(gateway, McpOptions::default()),
        registry_service: RegistryService::new(
            store.clone(),
            catalog,
            sessions,
            ConnectOptions::default(),
        ),
        registry: store,
        identity,
        mode: mode_svc,
        metrics: None,
    };
    let app = trunks_api::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let anon = Client::new(&format!("http://{addr}"), None, reqwest::Client::new()).unwrap();
    let admin = match init.admin_access_token {
        Some(t) => anon.with_token(SecretString::from(t)),
        None => anon.clone(),
    };
    (admin, anon, dir)
}

fn stdio_input(name: &str) -> RegisterServerInput {
    RegisterServerInput {
        name: name.into(),
        transport: Transport::Stdio,
        description: "fake".into(),
        url: None,
        bearer_token: None,
        headers: BTreeMap::new(),
        command: Some(trunks_testing::fake_upstream_bin().to_string_lossy().into_owned()),
        args: vec![],
        env: BTreeMap::from([("SECRET".to_owned(), "s3cret".to_owned())]),
        session_mode: SessionMode::Stateless,
        oauth_redirect_uri: None,
        oauth_client_id: None,
        oauth_client_secret: None,
        oauth_scopes: vec![],
    }
}

fn api_status(e: &trunks_client::Error) -> u16 {
    match e {
        trunks_client::Error::Api { status, .. } | trunks_client::Error::Status { status, .. } => {
            *status
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[tokio::test]
async fn register_discovers_entities_and_validates_input() {
    let (admin, _anon, _dir) = boot(ServerMode::Development).await;

    // validation happens before any connection
    let mut bad = stdio_input("bad__name");
    bad.command = Some("/definitely/missing".into());
    assert_eq!(api_status(&admin.register_server(&bad, false).await.unwrap_err()), 400);
    let mut no_cmd = stdio_input("nocmd");
    no_cmd.command = None;
    let e = admin.register_server(&no_cmd, false).await.unwrap_err();
    assert_eq!(api_status(&e), 400);
    assert!(e.to_string().contains("command is required"));
    let mut sse = stdio_input("sse");
    sse.transport = Transport::Sse;
    sse.url = Some("http://127.0.0.1:1/sse".into());
    assert!(admin.register_server(&sse, false).await.unwrap_err().to_string().contains("sse"));
    let mut oauth = stdio_input("oauth");
    oauth.oauth_client_id = Some("x".into());
    assert_eq!(api_status(&admin.register_server(&oauth, false).await.unwrap_err()), 400);

    // a connection failure persists nothing
    let mut missing = stdio_input("missing");
    missing.command = Some("/definitely/missing/binary".into());
    let e = admin.register_server(&missing, false).await.unwrap_err();
    assert_eq!(api_status(&e), 500);
    assert!(e.to_string().contains("failed to spawn"), "{e}");
    assert!(admin.list_servers().await.unwrap().is_empty());

    // success: 201 with the sanitized server; entities discovered
    let r = admin.register_server(&stdio_input("alpha"), false).await.unwrap();
    let s = r.server.unwrap();
    assert_eq!(s.name.as_str(), "alpha");
    assert_eq!(s.env_keys, vec!["SECRET"]);
    let tools = admin.list_tools(Some("alpha")).await.unwrap();
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, vec!["alpha__echo", "alpha__echo_headers", "alpha__fail"]);
    assert!(tools.iter().all(|t| t.enabled && t.input_schema.is_some()));
    assert_eq!(admin.list_prompts(Some("alpha")).await.unwrap().len(), 1);
    let res = admin.list_resources(Some("alpha")).await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].uri.original(), "mem://hello");

    // the registered server is immediately usable through the gateway
    let out = admin
        .invoke_tool("alpha__echo", serde_json::json!({"text": "hi"}).as_object().cloned().unwrap())
        .await
        .unwrap();
    assert_eq!(out.content[0]["text"], "hi");

    // duplicate → 400; force → replaced
    let e = admin.register_server(&stdio_input("alpha"), false).await.unwrap_err();
    assert_eq!(api_status(&e), 400);
    assert!(e.to_string().contains("already exists"));
    let mut again = stdio_input("alpha");
    again.description = "replaced".into();
    let r = admin.register_server(&again, true).await.unwrap();
    assert_eq!(r.server.unwrap().description, "replaced");
    assert_eq!(admin.list_servers().await.unwrap().len(), 1);
    assert_eq!(admin.list_tools(Some("alpha")).await.unwrap().len(), 3);

    // export carries the secrets (admin only)
    let configs = admin.server_configs().await.unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].env.get("SECRET").map(String::as_str), Some("s3cret"));
    assert_eq!(configs[0].description, "replaced");
}

#[tokio::test]
async fn enable_disable_semantics_and_deregister() {
    let (admin, _anon, _dir) = boot(ServerMode::Development).await;
    admin.register_server(&stdio_input("alpha"), false).await.unwrap();
    admin.register_server(&stdio_input("beta"), false).await.unwrap();

    // single entity: always echoed back, even when unchanged
    let r = admin.set_entities_enabled("tools", "alpha__echo", false).await.unwrap();
    assert_eq!(r.tools_affected, vec!["alpha__echo"]);
    let r = admin.set_entities_enabled("tools", "alpha__echo", false).await.unwrap();
    assert_eq!(r.tools_affected, vec!["alpha__echo"]);
    assert!(!admin.get_tool("alpha__echo").await.unwrap().enabled);
    // a disabled tool is not callable
    let e = admin.invoke_tool("alpha__echo", serde_json::Map::new()).await.unwrap_err();
    assert_eq!(api_status(&e), 404);

    // server-wide: only the ones that changed
    let r = admin.set_entities_enabled("tools", "alpha", false).await.unwrap();
    assert_eq!(r.tools_affected, vec!["alpha__echo_headers", "alpha__fail"]);
    let r = admin.set_entities_enabled("tools", "alpha", true).await.unwrap();
    assert_eq!(r.tools_affected, vec!["alpha__echo", "alpha__echo_headers", "alpha__fail"]);

    // prompts and resources follow the same rules; resources take a URI or a server
    let r = admin.set_entities_enabled("prompts", "beta__greet", false).await.unwrap();
    assert_eq!(r.prompts_affected, vec!["beta__greet"]);
    let uri = admin.list_resources(Some("beta")).await.unwrap()[0].uri.to_string();
    let r = admin.set_entities_enabled("resources", &uri, false).await.unwrap();
    assert_eq!(r.resources_affected, vec![uri.clone()]);
    let r = admin.set_entities_enabled("resources", "beta", true).await.unwrap();
    assert_eq!(r.resources_affected, vec![uri]);
    let e =
        admin.set_entities_enabled("resources", "not-a-uri-nor-server", true).await.unwrap_err();
    assert_eq!(api_status(&e), 400);

    // unknown entities
    assert_eq!(
        api_status(&admin.set_entities_enabled("tools", "alpha__nope", true).await.unwrap_err()),
        404
    );
    assert_eq!(api_status(&admin.set_entities_enabled("tools", "", true).await.unwrap_err()), 400);

    // server flag cascades to the effective state; the response lists what changed
    admin.set_entities_enabled("tools", "alpha__fail", false).await.unwrap();
    let r = admin.set_server_enabled("alpha", false).await.unwrap();
    assert_eq!(r.name.as_deref(), Some("alpha"));
    assert_eq!(r.tools_affected, vec!["alpha__echo", "alpha__echo_headers"]);
    assert_eq!(r.prompts_affected, vec!["alpha__greet"]);
    assert_eq!(r.resources_affected.len(), 1);
    let r = admin.set_server_enabled("alpha", false).await.unwrap();
    assert!(r.tools_affected.is_empty());
    assert!(admin.list_tools(Some("alpha")).await.unwrap().iter().all(|t| !t.enabled));
    let r = admin.set_server_enabled("alpha", true).await.unwrap();
    // `alpha__fail` stays individually disabled
    assert_eq!(r.tools_affected, vec!["alpha__echo", "alpha__echo_headers"]);
    assert_eq!(api_status(&admin.set_server_enabled("zeta", true).await.unwrap_err()), 404);

    // deregister
    admin.deregister_server("beta").await.unwrap();
    assert_eq!(api_status(&admin.deregister_server("beta").await.unwrap_err()), 404);
    assert_eq!(admin.list_servers().await.unwrap().len(), 1);
    assert!(admin.list_tools(Some("beta")).await.is_err());
}

#[tokio::test]
async fn enterprise_admin_routes_require_admin() {
    let (admin, anon, _dir) = boot(ServerMode::Enterprise).await;
    assert_eq!(
        api_status(&anon.register_server(&stdio_input("alpha"), false).await.unwrap_err()),
        401
    );
    assert_eq!(api_status(&anon.server_configs().await.unwrap_err()), 401);
    let bad = anon.with_token(SecretString::from("not-a-token".to_owned()));
    assert_eq!(
        api_status(&bad.register_server(&stdio_input("alpha"), false).await.unwrap_err()),
        401
    );
    admin.register_server(&stdio_input("alpha"), false).await.unwrap();
    assert_eq!(admin.server_configs().await.unwrap().len(), 1);
    admin.deregister_server("alpha").await.unwrap();
}
