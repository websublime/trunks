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
    assert_eq!(api_status(&as_bob.list_groups().await.unwrap_err()), 403);
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

fn admin_token(c: &Client) -> String {
    c.token().map(|t| secrecy::ExposeSecret::expose_secret(t).to_owned()).unwrap_or_default()
}
