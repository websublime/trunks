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
use trunks_types::api::RegisterServerInput;
use trunks_types::{ServerMode, SessionMode, Transport};

/// Boots an empty server; returns a client (admin token in enterprise mode) and the base URL.
async fn boot(mode: ServerMode) -> (Client, Client, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::connect(&format!("sqlite://{}/db.sqlite", dir.path().display()), 4).await.unwrap();
    db.migrate().await.unwrap();
    let store = Arc::new(Store(db));
    let identity = IdentityService::new(store.clone(), store.clone());
    let mode_svc = ModeService::load(store.clone(), identity.clone()).await.unwrap();
    let init = mode_svc.init(mode).await.unwrap();
    let catalog = Catalog::empty();
    catalog.reload(store.as_ref()).await.unwrap();
    let sessions = SessionManager::new(SessionOptions {
        connect: ConnectOptions { oauth: Some(store.clone()), ..ConnectOptions::default() },
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

// ---- Phase 3: users and MCP clients

use trunks_types::api::{
    CreateMcpClientRequest, CreateUserRequest, UpdateMcpClientRequest, UpdateUserRequest,
};

async fn mcp_list_tools(base: &str, token: &str) -> Result<Vec<String>, String> {
    use rmcp::ServiceExt as _;
    let mut h = reqwest::header::HeaderMap::new();
    h.insert(reqwest::header::AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
    let rc = reqwest::Client::builder().default_headers(h).build().unwrap();
    let t = rmcp::transport::StreamableHttpClientTransport::with_client(
        rc,
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
            format!("{base}/mcp"),
        ),
    );
    let c = ().serve(t).await.map_err(|e| e.to_string())?;
    let tools = c.list_all_tools().await.map_err(|e| e.to_string())?;
    Ok(tools.iter().map(|t| t.name.to_string()).collect())
}

#[tokio::test]
async fn identities_are_enterprise_only() {
    let (admin, _anon, _dir) = boot(ServerMode::Development).await;
    let e = admin.list_users().await.unwrap_err();
    assert_eq!(api_status(&e), 403);
    assert!(e.to_string().contains("enterprise"));
    assert_eq!(api_status(&admin.list_clients().await.unwrap_err()), 403);
    let req = CreateUserRequest { username: "bob".into(), access_token: None };
    assert_eq!(api_status(&admin.create_user(&req).await.unwrap_err()), 403);
}

#[tokio::test]
async fn users_crud_and_token_semantics() {
    let (admin, anon, _dir) = boot(ServerMode::Enterprise).await;
    let users = admin.list_users().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "admin");

    // validation
    let bad = CreateUserRequest { username: "  ".into(), access_token: None };
    let e = admin.create_user(&bad).await.unwrap_err();
    assert_eq!(api_status(&e), 400);
    assert!(e.to_string().contains("username is required"));
    let bad = CreateUserRequest { username: "bob".into(), access_token: Some("short".into()) };
    assert_eq!(api_status(&admin.create_user(&bad).await.unwrap_err()), 400);
    let bad = CreateUserRequest { username: "has space".into(), access_token: None };
    assert_eq!(api_status(&admin.create_user(&bad).await.unwrap_err()), 400);

    // create with a generated token; the token works and is never listed
    let bob = admin
        .create_user(&CreateUserRequest { username: "bob".into(), access_token: None })
        .await
        .unwrap();
    assert_eq!(bob.access_token.len(), 43);
    assert_eq!(bob.role, trunks_types::Role::User);
    let as_bob = anon.with_token(SecretString::from(bob.access_token.clone()));
    assert_eq!(as_bob.whoami().await.unwrap().username, "bob");
    assert!(
        !serde_json::to_value(admin.list_users().await.unwrap())
            .unwrap()
            .to_string()
            .contains("access_token")
    );

    // duplicate
    let e = admin
        .create_user(&CreateUserRequest { username: "bob".into(), access_token: None })
        .await
        .unwrap_err();
    assert_eq!(api_status(&e), 400);
    assert!(e.to_string().contains("already exists"));

    // a regular user is not an admin
    assert_eq!(api_status(&as_bob.list_users().await.unwrap_err()), 403);
    assert_eq!(
        api_status(&as_bob.register_server(&stdio_input("x"), false).await.unwrap_err()),
        403
    );
    assert!(as_bob.list_servers().await.is_ok());

    // custom token + rotation: old token stops working
    let carol = admin
        .create_user(&CreateUserRequest {
            username: "carol".into(),
            access_token: Some("carol-token-1".into()),
        })
        .await
        .unwrap();
    assert_eq!(carol.access_token, "carol-token-1");
    let rotated =
        admin.update_user("carol", &UpdateUserRequest { access_token: None }).await.unwrap();
    assert_ne!(rotated.access_token, "carol-token-1");
    let old = anon.with_token(SecretString::from("carol-token-1".to_owned()));
    assert_eq!(api_status(&old.whoami().await.unwrap_err()), 401);
    let new = anon.with_token(SecretString::from(rotated.access_token));
    assert_eq!(new.whoami().await.unwrap().username, "carol");
    assert_eq!(
        api_status(&admin.update_user("nobody", &UpdateUserRequest::default()).await.unwrap_err()),
        404
    );

    // delete: idempotent, admin protected
    admin.delete_user("carol").await.unwrap();
    admin.delete_user("carol").await.unwrap();
    assert_eq!(api_status(&new.whoami().await.unwrap_err()), 401);
    let e = admin.delete_user("admin").await.unwrap_err();
    assert_eq!(api_status(&e), 400);
    assert!(e.to_string().contains("admin"));
    assert_eq!(admin.list_users().await.unwrap().len(), 2);
}

#[tokio::test]
async fn mcp_clients_crud_and_allow_lists_apply_live() {
    let (admin, anon, _dir) = boot(ServerMode::Enterprise).await;
    let base = admin.base_url().to_string();
    let base = base.trim_end_matches('/').to_owned();
    admin.register_server(&stdio_input("alpha"), false).await.unwrap();
    admin.register_server(&stdio_input("beta"), false).await.unwrap();

    // validation
    let bad = CreateMcpClientRequest { name: String::new(), ..Default::default() };
    assert_eq!(api_status(&admin.create_client(&bad).await.unwrap_err()), 400);
    let bad = CreateMcpClientRequest {
        name: "c".into(),
        allow_list: vec!["bad__x".into()],
        ..Default::default()
    };
    let e = admin.create_client(&bad).await.unwrap_err();
    assert_eq!(api_status(&e), 400);
    assert!(e.to_string().contains("allow list"));

    // empty allow list → authenticated but sees nothing (fail closed)
    let c1 = admin
        .create_client(&CreateMcpClientRequest {
            name: "agent".into(),
            description: "d".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let t1 = c1.access_token.clone().unwrap();
    assert_eq!(mcp_list_tools(&base, &t1).await.unwrap(), Vec::<String>::new());

    // allow list change applies to the next request, no restart
    let upd = admin
        .update_client(
            "agent",
            &UpdateMcpClientRequest { allow_list: Some(vec!["beta".into()]), ..Default::default() },
        )
        .await
        .unwrap();
    assert!(upd.access_token.is_none(), "token must not be returned unless it changed");
    let tools = mcp_list_tools(&base, &t1).await.unwrap();
    assert_eq!(tools, ["beta__echo", "beta__echo_headers", "beta__fail"]);

    // wildcard
    let c2 = admin
        .create_client(&CreateMcpClientRequest {
            name: "all".into(),
            allow_list: vec!["*".into()],
            access_token: Some("client-token-xyz".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(c2.access_token.as_deref(), Some("client-token-xyz"));
    assert_eq!(mcp_list_tools(&base, "client-token-xyz").await.unwrap().len(), 6);

    // listings never carry tokens; user tokens are rejected on /mcp
    let listed = admin.list_clients().await.unwrap();
    assert_eq!(listed.len(), 2);
    assert!(!serde_json::to_value(&listed).unwrap().to_string().contains("token"));
    let bob = admin
        .create_user(&CreateUserRequest { username: "bob".into(), access_token: None })
        .await
        .unwrap();
    assert!(mcp_list_tools(&base, &bob.access_token).await.is_err());

    // rotation invalidates the old token; duplicate names rejected; delete idempotent
    let rot = admin
        .update_client(
            "agent",
            &UpdateMcpClientRequest { rotate_token: true, ..Default::default() },
        )
        .await
        .unwrap();
    let t1b = rot.access_token.unwrap();
    assert_ne!(t1b, t1);
    assert!(mcp_list_tools(&base, &t1).await.is_err());
    assert_eq!(mcp_list_tools(&base, &t1b).await.unwrap().len(), 3);
    let dup = CreateMcpClientRequest { name: "agent".into(), ..Default::default() };
    assert!(admin.create_client(&dup).await.unwrap_err().to_string().contains("already exists"));
    assert_eq!(
        api_status(
            &admin.update_client("ghost", &UpdateMcpClientRequest::default()).await.unwrap_err()
        ),
        404
    );
    admin.delete_client("agent").await.unwrap();
    admin.delete_client("agent").await.unwrap();
    assert!(mcp_list_tools(&base, &t1b).await.is_err());
    assert_eq!(admin.list_clients().await.unwrap().len(), 1);

    // the anonymous client cannot touch identities
    assert_eq!(api_status(&anon.list_clients().await.unwrap_err()), 401);
}

// ---- Phase 4: tool groups

use trunks_types::api::ToolGroup;

fn group(name: &str, tools: &[&str], servers: &[&str], excluded: &[&str]) -> ToolGroup {
    ToolGroup {
        name: name.into(),
        description: "d".into(),
        included_tools: tools.iter().map(|s| (*s).to_owned()).collect(),
        included_servers: servers.iter().map(|s| (*s).to_owned()).collect(),
        excluded_tools: excluded.iter().map(|s| (*s).to_owned()).collect(),
    }
}

async fn mcp_group_tools(
    base: &str,
    group: &str,
    token: Option<&str>,
) -> Result<Vec<String>, String> {
    use rmcp::ServiceExt as _;
    let mut h = reqwest::header::HeaderMap::new();
    if let Some(t) = token {
        h.insert(reqwest::header::AUTHORIZATION, format!("Bearer {t}").parse().unwrap());
    }
    let rc = reqwest::Client::builder().default_headers(h).build().unwrap();
    let t = rmcp::transport::StreamableHttpClientTransport::with_client(
        rc,
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
            format!("{base}/v0/groups/{group}/mcp"),
        ),
    );
    let c = ().serve(t).await.map_err(|e| e.to_string())?;
    let tools = c.list_all_tools().await.map_err(|e| e.to_string())?;
    Ok(tools.iter().map(|t| t.name.to_string()).collect())
}

#[tokio::test]
async fn tool_groups_crud_validation_and_degradation() {
    let (admin, _anon, _dir) = boot(ServerMode::Development).await;
    let base = admin.base_url().to_string().trim_end_matches('/').to_owned();
    admin.register_server(&stdio_input("alpha"), false).await.unwrap();
    admin.register_server(&stdio_input("beta"), false).await.unwrap();

    // validation
    let e = admin.create_group(&group("bad name", &["alpha__echo"], &[], &[])).await.unwrap_err();
    assert_eq!(api_status(&e), 400);
    let e = admin.create_group(&group("g", &["alpha__nope"], &[], &[])).await.unwrap_err();
    assert!(e.to_string().contains("does not exist or is disabled"), "{e}");
    let e = admin.create_group(&group("g", &[], &["gamma"], &[])).await.unwrap_err();
    assert!(e.to_string().contains("server 'gamma' does not exist"), "{e}");
    let e =
        admin.create_group(&group("g", &["alpha__echo"], &[], &["alpha__echo"])).await.unwrap_err();
    assert!(e.to_string().contains("no enabled tools"), "{e}");
    admin.set_entities_enabled("tools", "alpha__fail", false).await.unwrap();
    let e = admin.create_group(&group("g", &["alpha__fail"], &[], &[])).await.unwrap_err();
    assert!(e.to_string().contains("disabled"), "{e}");

    // create: lists de-duplicated and sorted; endpoint returned; effective = union − exclusions
    let created = admin
        .create_group(&group("dev", &["alpha__echo", "alpha__echo"], &["beta"], &["beta__fail"]))
        .await
        .unwrap();
    assert_eq!(created.streamable_http_endpoint, "/v0/groups/dev/mcp");
    assert_eq!(created.group.included_tools, vec!["alpha__echo"]);
    let eff: Vec<String> =
        admin.group_tools("dev").await.unwrap().iter().map(|t| t.name.to_string()).collect();
    assert_eq!(eff, ["alpha__echo", "beta__echo", "beta__echo_headers"]);
    assert_eq!(mcp_group_tools(&base, "dev", None).await.unwrap(), eff);
    assert_eq!(admin.list_groups().await.unwrap().len(), 1);
    assert_eq!(admin.get_group("dev").await.unwrap().group.included_servers, vec!["beta"]);
    assert!(
        admin
            .create_group(&group("dev", &["alpha__echo"], &[], &[]))
            .await
            .unwrap_err()
            .to_string()
            .contains("already exists")
    );
    assert_eq!(api_status(&admin.get_group("nope").await.unwrap_err()), 404);

    // update is a full replacement; response carries old and new
    let upd = admin
        .update_group("dev", &group("ignored", &["alpha__echo_headers"], &[], &[]))
        .await
        .unwrap();
    assert_eq!(upd.name, "dev");
    assert_eq!(upd.old.included_servers, vec!["beta"]);
    assert!(upd.new.included_servers.is_empty());
    assert_eq!(mcp_group_tools(&base, "dev", None).await.unwrap(), ["alpha__echo_headers"]);
    assert_eq!(
        api_status(
            &admin.update_group("nope", &group("x", &["alpha__echo"], &[], &[])).await.unwrap_err()
        ),
        404
    );

    // disabling the only tool degrades the group to empty (no failure); re-enabling restores it
    admin.set_entities_enabled("tools", "alpha__echo_headers", false).await.unwrap();
    assert!(mcp_group_tools(&base, "dev", None).await.unwrap().is_empty());
    admin.set_entities_enabled("tools", "alpha__echo_headers", true).await.unwrap();
    assert_eq!(mcp_group_tools(&base, "dev", None).await.unwrap().len(), 1);

    // deregistering the server degrades too; the stored definition is untouched
    admin.deregister_server("alpha").await.unwrap();
    assert!(mcp_group_tools(&base, "dev", None).await.unwrap().is_empty());
    assert_eq!(
        admin.get_group("dev").await.unwrap().group.included_tools,
        vec!["alpha__echo_headers"]
    );

    // delete: idempotent; the group endpoint disappears (cached service dropped)
    admin.delete_group("dev").await.unwrap();
    admin.delete_group("dev").await.unwrap();
    assert!(admin.list_groups().await.unwrap().is_empty());
    assert!(mcp_group_tools(&base, "dev", None).await.is_err());
}

#[tokio::test]
async fn tool_groups_never_widen_a_client_allow_list() {
    let (admin, anon, _dir) = boot(ServerMode::Enterprise).await;
    let base = admin.base_url().to_string().trim_end_matches('/').to_owned();
    admin.register_server(&stdio_input("alpha"), false).await.unwrap();
    admin.register_server(&stdio_input("beta"), false).await.unwrap();
    admin.create_group(&group("both", &[], &["alpha", "beta"], &[])).await.unwrap();
    let c = admin
        .create_client(&CreateMcpClientRequest {
            name: "only-beta".into(),
            allow_list: vec!["beta".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    let token = c.access_token.unwrap();
    let tools = mcp_group_tools(&base, "both", Some(&token)).await.unwrap();
    assert_eq!(tools, ["beta__echo", "beta__echo_headers", "beta__fail"]);
    assert!(
        mcp_group_tools(&base, "both", None).await.is_err(),
        "group endpoints need a client token"
    );
    // a user cannot manage groups
    let bob = admin
        .create_user(&CreateUserRequest { username: "bob".into(), access_token: None })
        .await
        .unwrap();
    let as_bob = anon.with_token(SecretString::from(bob.access_token));
    assert_eq!(
        api_status(&as_bob.create_group(&group("x", &[], &["alpha"], &[])).await.unwrap_err()),
        403
    );
    // D14: users may list groups, but only the ones granted to them
    assert!(as_bob.list_groups().await.unwrap().is_empty());
    assert_eq!(api_status(&as_bob.get_group("both").await.unwrap_err()), 404);
}

// ---- Phase 6a: dashboard reads

#[tokio::test]
async fn dashboard_reads_are_sanitized_and_gated_in_enterprise() {
    let (admin, anon, _dir) = boot(ServerMode::Enterprise).await;
    let base = admin.base_url().to_string().trim_end_matches('/').to_owned();
    let http = reqwest::Client::new();

    // without a token: 401 on every read; the callback stays public
    for path in [
        "overview",
        "servers",
        "tools",
        "prompts",
        "resources",
        "tool-groups",
        "diagnostics",
        "oauth/session/x",
    ] {
        let res = http.get(format!("{base}/api/dashboard/{path}")).send().await.unwrap();
        assert_eq!(res.status(), 401, "{path}");
    }
    let res =
        http.get(format!("{base}/api/dashboard/oauth/callback?error=x")).send().await.unwrap();
    assert_eq!(res.status(), 200);

    // with the admin token: register a server with secrets, then read everything
    let mut input = stdio_input("alpha");
    input.env.insert("API_KEY".into(), "super-secret-value".into());
    admin.register_server(&input, false).await.unwrap();
    admin.create_group(&group("dev", &["alpha__echo"], &[], &[])).await.unwrap();
    let bearer = {
        // the SDK client holds the token; reuse it through a raw request
        let cfgs_req = admin.server_configs().await.unwrap();
        assert_eq!(cfgs_req[0].env.get("API_KEY").map(String::as_str), Some("super-secret-value"));
        admin_token(&admin)
    };
    let get = |path: &str| {
        let http = http.clone();
        let url = format!("{base}/api/dashboard/{path}");
        let bearer = bearer.clone();
        async move { http.get(url).bearer_auth(bearer).send().await.unwrap() }
    };
    let overview: serde_json::Value = get("overview").await.json().await.unwrap();
    assert_eq!(overview["mode"], "enterprise");
    assert_eq!(overview["counts"]["servers"], 1);
    assert_eq!(overview["counts"]["tools"], 3);
    assert_eq!(overview["counts"]["tool_groups"], 1);
    let servers = get("servers").await.text().await.unwrap();
    assert!(servers.contains("API_KEY"), "env keys are listed: {servers}");
    assert!(!servers.contains("super-secret-value"), "values never: {servers}");
    assert!(servers.contains("fake-upstream"));
    let detail: serde_json::Value = get("servers/alpha").await.json().await.unwrap();
    assert_eq!(detail["counts"]["tools_enabled"], 3);
    assert_eq!(detail["oauth"]["status"], "none");
    assert_eq!(detail["tools"].as_array().unwrap().len(), 3);
    assert_eq!(get("servers/zzz").await.status(), 404);
    let tools: serde_json::Value =
        get("tools?enabled=true&server=alpha").await.json().await.unwrap();
    assert_eq!(tools.as_array().unwrap().len(), 3);
    let groups: serde_json::Value = get("tool-groups").await.json().await.unwrap();
    assert_eq!(groups[0]["effective_tools"], serde_json::json!(["alpha__echo"]));
    assert_eq!(groups[0]["degraded"], false);
    assert_eq!(groups[0]["streamable_http_endpoint"], "/v0/groups/dev/mcp");
    let diag: serde_json::Value = get("diagnostics").await.json().await.unwrap();
    assert_eq!(diag["database"]["backend"], "sqlite");
    assert!(diag["servers_unreachable"].as_array().unwrap().is_empty());

    // a disabled server degrades the group; an unreachable upstream shows up in diagnostics
    admin.set_entities_enabled("tools", "alpha__echo", false).await.unwrap();
    let groups: serde_json::Value = get("tool-groups/dev").await.json().await.unwrap();
    assert_eq!(groups["degraded"], true);
    let mut broken = stdio_input("broken");
    broken.command = Some("/definitely/missing".into());
    // registration fails before persisting, so seed the failure through the gateway instead:
    let _ = admin.register_server(&broken, false).await; // not persisted
    let _ = anon;
    let _ = admin.invoke_tool("alpha__fail", serde_json::Map::new()).await; // tool error, not transport
    let diag: serde_json::Value = get("diagnostics").await.json().await.unwrap();
    assert!(
        diag["servers_unreachable"].as_array().unwrap().is_empty(),
        "a tool error is not unreachability"
    );
}

/// The dashboard views follow the same rule as `/api/v0`: what a caller may not use is not
/// listed, not counted, and answered exactly like something that does not exist.
#[tokio::test]
async fn dashboard_views_hold_only_what_the_caller_may_use() {
    let (admin, anon, _dir) = boot(ServerMode::Enterprise).await;
    let base = admin.base_url().to_string().trim_end_matches('/').to_owned();
    let http = reqwest::Client::new();
    let mut alpha = stdio_input("alpha");
    alpha.env.insert("API_KEY".into(), "super-secret-value".into());
    admin.register_server(&alpha, false).await.unwrap();
    admin.register_server(&stdio_input("beta"), false).await.unwrap();
    admin.create_group(&group("dev", &["beta__echo"], &[], &[])).await.unwrap();
    admin.create_group(&group("ops", &["alpha__echo"], &[], &[])).await.unwrap();

    let get = |who: &Client, path: &str| {
        let req = http.get(format!("{base}/api/dashboard/{path}")).bearer_auth(admin_token(who));
        async move {
            let res = req.send().await.unwrap();
            let status = res.status().as_u16();
            (status, res.json::<serde_json::Value>().await.unwrap_or_default())
        }
    };
    let listed = |v: &serde_json::Value, key: &str| -> Vec<String> {
        v.as_array().unwrap().iter().map(|x| x[key].as_str().unwrap().to_owned()).collect()
    };

    // ---- no grants: nothing, and nothing to tell apart from "does not exist"
    let nobody = user_client(&admin, &anon, "nobody").await;
    for path in ["servers", "tools", "prompts", "resources", "tool-groups", "tools?server=alpha"] {
        let (status, body) = get(&nobody, path).await;
        assert_eq!((status, &body), (200, &serde_json::json!([])), "{path}");
    }
    let (status, overview) = get(&nobody, "overview").await;
    assert_eq!(status, 200);
    assert_eq!(
        overview["counts"],
        serde_json::json!({
            "servers": 0, "servers_enabled": 0, "tools": 0, "tools_enabled": 0,
            "prompts": 0, "resources": 0, "tool_groups": 0, "pending_oauth": 0
        })
    );
    assert!(overview.get("sessions").is_none(), "process-wide facts are for admins: {overview}");
    let (status, hidden) = get(&nobody, "servers/alpha").await;
    let (_, missing) = get(&nobody, "servers/zzz").await;
    assert_eq!(status, 404);
    assert_eq!(
        hidden["error"].as_str().unwrap().replace("alpha", "zzz"),
        missing["error"].as_str().unwrap(),
        "a hidden server answers like a missing one"
    );
    let (status, hidden) = get(&nobody, "tool-groups/dev").await;
    let (_, missing) = get(&nobody, "tool-groups/zzz").await;
    assert_eq!(status, 404);
    assert_eq!(
        hidden["error"].as_str().unwrap().replace("dev", "zzz"),
        missing["error"].as_str().unwrap()
    );
    // the installation's own views are for administrators
    for path in ["diagnostics", "oauth/session/x"] {
        assert_eq!(get(&nobody, path).await.0, 403, "{path}");
    }

    // ---- a group grant: that group, its tools, and the server only as far as those tools go
    let gina = user_client(&admin, &anon, "gina").await;
    admin.grant(&grant("user", "gina", "tool_group", "dev")).await.unwrap();
    let (_, servers) = get(&gina, "servers").await;
    assert_eq!(listed(&servers, "name"), ["beta"]);
    assert_eq!(
        servers[0]["counts"],
        serde_json::json!({"tools": 1, "tools_enabled": 1, "prompts": 0, "resources": 0})
    );
    let (_, detail) = get(&gina, "servers/beta").await;
    assert_eq!(listed(&detail["tools"], "name"), ["beta__echo"]);
    assert!(detail["prompts"].as_array().unwrap().is_empty());
    assert!(detail["resources"].as_array().unwrap().is_empty());
    assert_eq!(get(&gina, "servers/alpha").await.0, 404);
    assert_eq!(listed(&get(&gina, "tools").await.1, "name"), ["beta__echo"]);
    assert_eq!(get(&gina, "prompts").await.1, serde_json::json!([]));
    assert_eq!(get(&gina, "resources").await.1, serde_json::json!([]));
    assert_eq!(listed(&get(&gina, "tool-groups").await.1, "name"), ["dev"]);
    assert_eq!(get(&gina, "tool-groups/dev").await.0, 200);
    assert_eq!(get(&gina, "tool-groups/ops").await.0, 404);
    let (_, overview) = get(&gina, "overview").await;
    assert_eq!(overview["counts"]["servers"], 1);
    assert_eq!(overview["counts"]["tools"], 1);
    assert_eq!(overview["counts"]["prompts"], 0);
    assert_eq!(overview["counts"]["tool_groups"], 1);

    // ---- a server grant: the whole server, and no group
    let sam = user_client(&admin, &anon, "sam").await;
    admin.grant(&grant("user", "sam", "server", "alpha")).await.unwrap();
    let (_, servers) = get(&sam, "servers").await;
    assert_eq!(listed(&servers, "name"), ["alpha"]);
    assert_eq!(
        servers[0]["counts"],
        serde_json::json!({"tools": 3, "tools_enabled": 3, "prompts": 1, "resources": 1})
    );
    let text = servers.to_string();
    assert!(text.contains("API_KEY") && !text.contains("super-secret-value"), "{text}");
    assert_eq!(get(&sam, "tools").await.1.as_array().unwrap().len(), 3);
    assert_eq!(get(&sam, "tools?server=beta").await.1, serde_json::json!([]));
    assert_eq!(get(&sam, "prompts").await.1.as_array().unwrap().len(), 1);
    assert_eq!(get(&sam, "resources").await.1.as_array().unwrap().len(), 1);
    assert_eq!(get(&sam, "tool-groups").await.1, serde_json::json!([]));
    assert_eq!(get(&sam, "diagnostics").await.0, 403);

    // ---- the administrator still sees everything, process-wide facts included
    let (_, overview) = get(&admin, "overview").await;
    assert_eq!(overview["counts"]["servers"], 2);
    assert_eq!(overview["counts"]["tools"], 6);
    assert_eq!(overview["counts"]["tool_groups"], 2);
    assert!(overview["sessions"]["stateful_active"].is_u64(), "{overview}");
    assert_eq!(listed(&get(&admin, "servers").await.1, "name"), ["alpha", "beta"]);
    assert_eq!(get(&admin, "diagnostics").await.0, 200);
    assert_eq!(get(&admin, "oauth/session/x").await.0, 200);

    // ---- the same rule on /api/v0: filtering by a hidden server is filtering by a missing one
    for kind in ["tools", "prompts", "resources"] {
        let status = |who: &Client, server: &str| {
            let req = http
                .get(format!("{base}/api/v0/{kind}?server={server}"))
                .bearer_auth(admin_token(who));
            async move { req.send().await.unwrap().status().as_u16() }
        };
        assert_eq!(status(&nobody, "alpha").await, 404, "{kind}: hidden");
        assert_eq!(status(&nobody, "zzz").await, 404, "{kind}: missing");
        assert_eq!(status(&sam, "alpha").await, 200, "{kind}: granted");
        assert_eq!(status(&sam, "beta").await, 404, "{kind}: hidden");
        assert_eq!(status(&gina, "beta").await, 200, "{kind}: seen through a group");
    }
}

fn admin_token(c: &Client) -> String {
    c.token().map(|t| secrecy::ExposeSecret::expose_secret(t).to_owned()).unwrap_or_default()
}

// ---- Phase 6b: teams, grants, owned clients (D14)

use trunks_types::api::{AddTeamMemberRequest, GrantSpec, Owner, Team};

fn grant(sk: &str, s: &str, tk: &str, t: &str) -> GrantSpec {
    GrantSpec {
        subject_kind: sk.into(),
        subject: s.into(),
        target_kind: tk.into(),
        target: t.into(),
    }
}

async fn user_client(admin: &Client, anon: &Client, name: &str) -> Client {
    let u = admin
        .create_user(&CreateUserRequest { username: name.into(), access_token: None })
        .await
        .unwrap();
    anon.with_token(SecretString::from(u.access_token))
}

#[allow(clippy::needless_pass_by_value)]
fn names<T, F: Fn(&T) -> String>(items: Vec<T>, f: F) -> Vec<String> {
    items.iter().map(f).collect()
}

#[tokio::test]
async fn grants_drive_what_users_see_and_invoke() {
    let (admin, anon, _dir) = boot(ServerMode::Enterprise).await;
    admin.register_server(&stdio_input("alpha"), false).await.unwrap();
    admin.register_server(&stdio_input("beta"), false).await.unwrap();
    admin.create_group(&group("dev", &["beta__echo"], &[], &[])).await.unwrap();
    let bob = user_client(&admin, &anon, "bob").await;

    // secure by default: nothing
    assert!(bob.list_servers().await.unwrap().is_empty());
    assert!(bob.list_tools(None).await.unwrap().is_empty());
    assert!(bob.list_groups().await.unwrap().is_empty());
    assert_eq!(
        api_status(&bob.invoke_tool("alpha__echo", serde_json::Map::new()).await.unwrap_err()),
        404
    );
    assert_eq!(api_status(&bob.get_tool("alpha__echo").await.unwrap_err()), 404);
    let me = bob.whoami().await.unwrap();
    assert!(!me.access.all && me.access.servers.is_empty());

    // users cannot grant; admins can; validation of kinds and names
    assert_eq!(
        api_status(&bob.grant(&grant("user", "bob", "server", "alpha")).await.unwrap_err()),
        403
    );
    assert_eq!(
        api_status(&admin.grant(&grant("robot", "bob", "server", "alpha")).await.unwrap_err()),
        400
    );
    assert_eq!(
        api_status(&admin.grant(&grant("user", "bob", "server", "bad__name")).await.unwrap_err()),
        400
    );
    assert_eq!(
        api_status(&admin.grant(&grant("user", "nobody", "server", "alpha")).await.unwrap_err()),
        404
    );

    // direct server grant: sees alpha only, can invoke, prompts/resources follow the server
    admin.grant(&grant("user", "bob", "server", "alpha")).await.unwrap();
    assert_eq!(names(bob.list_servers().await.unwrap(), |s| s.name.to_string()), ["alpha"]);
    assert_eq!(bob.list_tools(None).await.unwrap().len(), 3);
    assert_eq!(bob.list_prompts(None).await.unwrap().len(), 1);
    assert_eq!(bob.list_resources(None).await.unwrap().len(), 1);
    let out = bob
        .invoke_tool("alpha__echo", serde_json::json!({"text": "hi"}).as_object().cloned().unwrap())
        .await
        .unwrap();
    assert_eq!(out.content[0]["text"], "hi");
    assert_eq!(
        api_status(&bob.invoke_tool("beta__echo", serde_json::Map::new()).await.unwrap_err()),
        404
    );
    let me = bob.whoami().await.unwrap();
    assert_eq!(me.access.servers, ["alpha"]);
    // idempotent grant → 200, listing by target
    admin.grant(&grant("user", "bob", "server", "alpha")).await.unwrap();
    let on_alpha = admin.list_grants(Some(("server", "alpha"))).await.unwrap();
    assert_eq!(on_alpha.len(), 1);
    assert_eq!(on_alpha[0].subject, "bob");

    // a group grant exposes exactly the group's effective tools (beta__echo), not all of beta,
    // and beta's prompts/resources stay hidden; the server shows as visible through the group
    admin.grant(&grant("user", "bob", "tool_group", "dev")).await.unwrap();
    let tools = names(bob.list_tools(None).await.unwrap(), |t| t.name.to_string());
    assert_eq!(tools, ["alpha__echo", "alpha__echo_headers", "alpha__fail", "beta__echo"]);
    assert_eq!(names(bob.list_servers().await.unwrap(), |s| s.name.to_string()), ["alpha", "beta"]);
    assert_eq!(bob.list_prompts(None).await.unwrap().len(), 1, "beta's prompt is not granted");
    assert_eq!(names(bob.list_groups().await.unwrap(), |g| g.name.clone()), ["dev"]);
    assert_eq!(bob.group_tools("dev").await.unwrap().len(), 1);
    assert!(
        bob.invoke_tool(
            "beta__echo",
            serde_json::json!({"text": "x"}).as_object().cloned().unwrap()
        )
        .await
        .is_ok()
    );
    assert_eq!(
        api_status(&bob.invoke_tool("beta__fail", serde_json::Map::new()).await.unwrap_err()),
        404
    );

    // revoke the group → beta disappears again
    admin.revoke(&grant("user", "bob", "tool_group", "dev")).await.unwrap();
    assert_eq!(bob.list_tools(None).await.unwrap().len(), 3);

    // wildcard
    admin.grant(&grant("user", "bob", "server", "*")).await.unwrap();
    assert!(bob.whoami().await.unwrap().access.all);
    assert_eq!(bob.list_tools(None).await.unwrap().len(), 6);
}

#[tokio::test]
async fn teams_memberships_and_owned_clients() {
    let (admin, anon, _dir) = boot(ServerMode::Enterprise).await;
    let base = admin.base_url().to_string().trim_end_matches('/').to_owned();
    admin.register_server(&stdio_input("alpha"), false).await.unwrap();
    admin.register_server(&stdio_input("beta"), false).await.unwrap();
    let bob = user_client(&admin, &anon, "bob").await;
    let carol = user_client(&admin, &anon, "carol").await;

    // teams: admin creates; users read; validation
    assert_eq!(
        api_status(
            &bob.create_team(&Team { name: "data".into(), description: String::new() })
                .await
                .unwrap_err()
        ),
        403
    );
    let t = admin
        .create_team(&Team { name: "data".into(), description: "data team".into() })
        .await
        .unwrap();
    assert!(t.members.is_empty());
    assert!(
        admin
            .create_team(&Team { name: "data".into(), ..Default::default() })
            .await
            .unwrap_err()
            .to_string()
            .contains("already exists")
    );
    assert_eq!(
        api_status(
            &admin
                .create_team(&Team { name: "bad name".into(), ..Default::default() })
                .await
                .unwrap_err()
        ),
        400
    );
    assert_eq!(bob.list_teams().await.unwrap().len(), 1);

    // membership: bob is maintainer, carol member; bob (maintainer) can add, carol cannot
    admin
        .add_team_member(
            "data",
            &AddTeamMemberRequest { username: "bob".into(), role: Some("maintainer".into()) },
        )
        .await
        .unwrap();
    assert_eq!(
        api_status(
            &carol
                .add_team_member(
                    "data",
                    &AddTeamMemberRequest { username: "carol".into(), role: None }
                )
                .await
                .unwrap_err()
        ),
        403
    );
    let d = bob
        .add_team_member("data", &AddTeamMemberRequest { username: "carol".into(), role: None })
        .await
        .unwrap();
    assert_eq!(d.members.len(), 2);
    assert_eq!(
        api_status(
            &bob.add_team_member(
                "data",
                &AddTeamMemberRequest { username: "carol".into(), role: Some("boss".into()) }
            )
            .await
            .unwrap_err()
        ),
        400
    );
    assert_eq!(
        api_status(
            &bob.add_team_member(
                "data",
                &AddTeamMemberRequest { username: "ghost".into(), role: None }
            )
            .await
            .unwrap_err()
        ),
        404
    );
    let me = carol.whoami().await.unwrap();
    assert_eq!(me.teams.len(), 1);
    assert_eq!(me.teams[0].role, "member");

    // team grant reaches every member
    admin.grant(&grant("team", "data", "server", "beta")).await.unwrap();
    assert_eq!(names(carol.list_servers().await.unwrap(), |s| s.name.to_string()), ["beta"]);
    assert_eq!(names(bob.list_servers().await.unwrap(), |s| s.name.to_string()), ["beta"]);

    // owned clients: carol's client inherits her access (beta only), even with an empty allow list;
    // allow_list narrows but never widens; carol cannot own a client as the team (not maintainer)
    let c = carol
        .create_client(&CreateMcpClientRequest { name: "carol-ide".into(), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(
        c.owner.as_ref().map(|o| (o.kind.as_str(), o.name.as_str())),
        Some(("user", "carol"))
    );
    let tok = c.access_token.clone().unwrap();
    assert_eq!(
        mcp_list_tools(&base, &tok).await.unwrap(),
        ["beta__echo", "beta__echo_headers", "beta__fail"]
    );
    let wide = carol
        .create_client(&CreateMcpClientRequest {
            name: "carol-wide".into(),
            allow_list: vec!["*".into(), "alpha".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        mcp_list_tools(&base, &wide.access_token.unwrap()).await.unwrap().len(),
        3,
        "allow_list cannot widen beyond the owner"
    );
    assert_eq!(
        api_status(
            &carol
                .create_client(&CreateMcpClientRequest {
                    name: "team-bot".into(),
                    owner: Some(Owner { kind: "team".into(), name: "data".into() }),
                    ..Default::default()
                })
                .await
                .unwrap_err()
        ),
        403
    );
    // bob (maintainer) can create a team-owned client
    let tb = bob
        .create_client(&CreateMcpClientRequest {
            name: "team-bot".into(),
            owner: Some(Owner { kind: "team".into(), name: "data".into() }),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(tb.owner.as_ref().map(|o| o.kind.as_str()), Some("team"));
    assert_eq!(mcp_list_tools(&base, &tb.access_token.clone().unwrap()).await.unwrap().len(), 3);

    // listing: carol sees her own + the team's; bob sees the same set; admin sees all
    let mine = names(carol.list_clients().await.unwrap(), |c| c.name.clone());
    assert_eq!(mine, ["carol-ide", "carol-wide", "team-bot"]);
    assert_eq!(admin.list_clients().await.unwrap().len(), 3);
    // carol cannot touch a client she does not own; a maintainer can manage the team's
    let dave = user_client(&admin, &anon, "dave").await;
    assert_eq!(api_status(&dave.delete_client("carol-ide").await.unwrap_err()), 403);
    assert_eq!(
        api_status(
            &carol
                .update_client(
                    "team-bot",
                    &UpdateMcpClientRequest { rotate_token: true, ..Default::default() }
                )
                .await
                .unwrap_err()
        ),
        403
    );
    bob.update_client(
        "team-bot",
        &UpdateMcpClientRequest { rotate_token: true, ..Default::default() },
    )
    .await
    .unwrap();
    carol.delete_client("carol-wide").await.unwrap();

    // a client's access follows live changes: revoking the team grant empties carol's client
    admin.revoke(&grant("team", "data", "server", "beta")).await.unwrap();
    assert!(mcp_list_tools(&base, &tok).await.unwrap().is_empty());
    // removing carol from the team: she keeps nothing
    bob.remove_team_member("data", "carol").await.unwrap();
    assert!(carol.whoami().await.unwrap().teams.is_empty());

    // Deleting the team deletes its clients. Left behind without an owner, a client would be
    // limited by its allow list alone: this one names a server the team could not use.
    let reach = bob
        .create_client(&CreateMcpClientRequest {
            name: "team-reach".into(),
            allow_list: vec!["alpha".into()],
            owner: Some(Owner { kind: "team".into(), name: "data".into() }),
            ..Default::default()
        })
        .await
        .unwrap();
    let reach_token = reach.access_token.unwrap();
    assert!(
        mcp_list_tools(&base, &reach_token).await.unwrap().is_empty(),
        "the team has no grant on alpha, so neither has its client"
    );
    admin.delete_team("data").await.unwrap();
    admin.delete_team("data").await.unwrap();
    assert_eq!(
        names(admin.list_clients().await.unwrap(), |c| c.name.clone()),
        ["carol-ide"],
        "the team's clients went with it; carol's own client stays"
    );
    assert!(mcp_list_tools(&base, &reach_token).await.is_err(), "its token no longer works");

    // ownerless admin-created clients keep the legacy allow-list behaviour
    let legacy = admin
        .create_client(&CreateMcpClientRequest {
            name: "legacy".into(),
            allow_list: vec!["alpha".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(legacy.owner.is_none());
    assert_eq!(mcp_list_tools(&base, &legacy.access_token.unwrap()).await.unwrap().len(), 3);
}
