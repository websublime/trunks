//! Adopt the legacy-format fixtures: migrate, normalise, read every table — on both backends.
//!
//! SQLite always runs (fixtures are copied to a temp dir). PostgreSQL runs when
//! `TRUNKS_TEST_PG_URL` points at a server where the test may create databases.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::too_many_lines)]

use std::path::PathBuf;

use trunks_core::ports::ConfigStore as _;
use trunks_db::Store;
use trunks_db::repos::registry::{NewPromptRow, NewResourceRow, NewServerRow, NewToolRow};
use trunks_db::repos::{catalog, config, identity, oauth, registry};
use trunks_db::{Db, with_pool};
use trunks_types::ServerMode;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

async fn sqlite_from_fixture(name: &str) -> (Db, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join(name);
    std::fs::copy(fixtures().join(name), &dst).unwrap();
    let db = Db::connect(&format!("sqlite://{}", dst.display()), 2).await.unwrap();
    (db, dir)
}

/// Creates a throwaway database from `legacy-enterprise.sql`. `None` when PG is not configured.
async fn postgres_from_fixture() -> Option<(Db, String, String)> {
    let admin_url = std::env::var("TRUNKS_TEST_PG_URL").ok()?;
    let name = format!("trunks_t_{}", std::process::id() ^ rand_u32());
    let admin = Db::connect(&admin_url, 1).await.unwrap();
    with_pool!(&admin, |p| sqlx::raw_sql(&format!("CREATE DATABASE {name}"))
        .execute(p)
        .await
        .map(|_| ())
        .unwrap());
    let base = admin_url.rsplit_once('/').map(|(b, _)| b.to_owned()).unwrap();
    let url = format!("{base}/{name}");
    let db = Db::connect(&url, 2).await.unwrap();
    let sql = std::fs::read_to_string(fixtures().join("legacy-enterprise.sql")).unwrap();
    with_pool!(&db, |p| sqlx::raw_sql(&sql).execute(p).await.map(|_| ()).unwrap());
    Some((db, admin_url, name))
}

fn rand_u32() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    (SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos()) ^ 0x5bd1_e995
}

async fn drop_pg(admin_url: &str, name: &str) {
    let admin = Db::connect(admin_url, 1).await.unwrap();
    with_pool!(&admin, |p| sqlx::raw_sql(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(p)
        .await
        .map(|_| ())
        .unwrap());
}

/// The checks every backend must pass on the enterprise fixture.
async fn assert_enterprise_fixture(db: &Db) {
    // migrations: baseline must be a no-op on legacy schema; all three versions applied
    let applied = db.migrate().await.unwrap();
    assert_eq!(applied.versions, vec![1, 2, 3], "all migrations applied on first run");
    assert_eq!(db.migrate().await.unwrap().versions, Vec::<i64>::new(), "second run is a no-op");

    // normalize_legacy
    let cfg = config::get(db).await.unwrap().unwrap();
    assert_eq!(cfg.mode, "enterprise", "'production' alias normalised");
    assert!(cfg.initialized);
    let store = Store(db.clone());
    assert_eq!(store.get().await.unwrap().unwrap().mode, ServerMode::Enterprise);
    assert!(!store.init(ServerMode::Development).await.unwrap(), "init is idempotent");

    let servers = catalog::servers(db).await.unwrap();
    assert_eq!(
        servers.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["atlassian", "context7", "filesystem", "legacy-sse"]
    );
    let fs = servers.iter().find(|s| s.name == "filesystem").unwrap();
    assert_eq!(fs.config.0["command"], "npx");
    assert_eq!(fs.config.0["env"]["LOG_LEVEL"], "debug");
    assert_eq!(fs.session_mode.as_deref(), Some("stateful"));
    assert!(fs.created_at.is_some(), "legacy timestamp text decodes to DateTime");
    let at = servers.iter().find(|s| s.name == "atlassian").unwrap();
    assert_eq!(at.session_mode.as_deref(), Some("stateless"), "NULL session_mode normalised");
    assert_eq!(at.config.0["headers"]["Authorization"], "Bearer custom-wins");
    let sse = servers.iter().find(|s| s.name == "legacy-sse").unwrap();
    assert!(!sse.enabled);
    assert_eq!(sse.transport, "sse");

    let tools = catalog::tools(db).await.unwrap();
    assert_eq!(tools.len(), 6);
    let read = tools.iter().find(|t| t.name == "read_file").unwrap();
    assert_eq!(read.annotations.as_ref().unwrap().0["readOnlyHint"], true);
    assert!(
        tools.iter().find(|t| t.name == "write_file").unwrap().annotations.is_none(),
        "NULL json → None"
    );
    assert!(tools.iter().any(|t| t.name == "jira__search"), "tool names may contain __");
    assert_eq!(catalog::tools_by_server(db, fs.id).await.unwrap().len(), 2);

    let prompts = catalog::prompts(db).await.unwrap();
    assert_eq!(prompts.len(), 2);
    assert!(prompts.iter().any(|p| p.name == "Model Details"));

    let resources = catalog::resources(db).await.unwrap();
    assert_eq!(resources.len(), 2);
    for r in &resources {
        assert!(
            r.uri.starts_with("trunks://res/filesystem/"),
            "URIs rewritten to trunks://: {}",
            r.uri
        );
    }
    assert_eq!(resources.iter().filter(|r| r.enabled).count(), 1);

    let groups = catalog::tool_groups(db).await.unwrap();
    assert_eq!(groups.len(), 2);
    let docs = groups.iter().find(|g| g.name == "docs-only").unwrap();
    assert!(docs.included_tools.is_none(), "NULL included_tools → None");
    assert_eq!(docs.included_servers.as_ref().unwrap().0, serde_json::json!(["context7"]));

    let users = identity::users(db).await.unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(
        identity::user_by_token(db, "admin-token-0123456789").await.unwrap().unwrap().role,
        "admin"
    );
    assert!(identity::user_by_token(db, "nope").await.unwrap().is_none());
    let clients = identity::clients(db).await.unwrap();
    assert_eq!(clients.len(), 2);
    let root = identity::client_by_token(db, "root-token-0123456789").await.unwrap().unwrap();
    assert_eq!(root.allow_list.0, serde_json::json!(["*"]));

    // ports: the catalog snapshot and identities map cleanly onto domain records
    {
        use trunks_core::ports::{IdentityStore as _, RegistryStore as _};
        let snap = store.load_snapshot().await.unwrap();
        assert_eq!(snap.servers.len(), 4);
        assert_eq!(snap.tools.len(), 6);
        assert_eq!(snap.resources.len(), 2);
        assert_eq!(snap.groups[1].included_servers, vec!["context7".to_string()]);
        let cat = trunks_core::catalog::Catalog::empty();
        let view = cat.reload(&store).await.unwrap();
        assert_eq!(
            view.enabled_tools().count(),
            4,
            "write_file disabled, old_tool on a disabled server"
        );
        assert_eq!(view.groups.len(), 2);
        assert_eq!(
            view.groups[&trunks_types::GroupName::parse("docs-only").unwrap()]
                .effective_tools
                .len(),
            1
        );
        let admin = store.user_by_token("admin-token-0123456789").await.unwrap().unwrap();
        assert_eq!(admin.role, trunks_types::Role::Admin);
        let root = store.client_by_token("root-token-0123456789").await.unwrap().unwrap();
        assert!(root.may_access(&trunks_types::ServerName::parse("anything").unwrap()));
        let cursor = store.client_by_token("cursor-token-0123456789").await.unwrap().unwrap();
        assert!(cursor.may_access(&trunks_types::ServerName::parse("filesystem").unwrap()));
        assert!(!cursor.may_access(&trunks_types::ServerName::parse("atlassian").unwrap()));
        let created = store
            .create_user("carol", trunks_types::Role::User, "carol-token-0123456789")
            .await
            .unwrap();
        assert_eq!(created.username, "carol");
        assert!(
            store
                .create_user("carol", trunks_types::Role::User, "other-token-0123456789")
                .await
                .is_err(),
            "duplicate username rejected"
        );
    }

    let tok = oauth::token_by_server(db, "atlassian").await.unwrap().unwrap();
    assert_eq!(tok.refresh_token.as_deref(), Some("at-refresh"));
    assert!(tok.expires_at.is_some());
    assert_eq!(oauth::pending_sessions(db).await.unwrap().len(), 1);
}

/// Registry writes on top of the enterprise fixture: register (with replace), flags, delete.
async fn assert_registry_writes(db: &Db) {
    let before = catalog::servers(db).await.unwrap().len();
    let server = NewServerRow {
        name: "newsrv".into(),
        transport: "stdio".into(),
        description: "d".into(),
        config: serde_json::json!({"command": "echo", "args": [], "env": {"K": "v"}}),
        session_mode: "stateless".into(),
    };
    let tools = vec![
        NewToolRow {
            name: "a".into(),
            description: String::new(),
            input_schema: Some(serde_json::json!({"type": "object"})),
            annotations: None,
        },
        NewToolRow {
            name: "b".into(),
            description: "B".into(),
            input_schema: None,
            annotations: None,
        },
    ];
    let prompts =
        vec![NewPromptRow { name: "p".into(), description: String::new(), arguments: None }];
    let resources = vec![NewResourceRow {
        uri: "trunks://res/newsrv/bWVtOi8vaGVsbG8".into(),
        original_uri: "mem://hello".into(),
        name: "hello".into(),
        description: String::new(),
        mime_type: Some("text/plain".into()),
        annotations: None,
        meta: None,
    }];
    let row =
        registry::create_server(db, &server, &tools, &prompts, &resources, false).await.unwrap();
    assert!(row.enabled);
    assert_eq!(row.config.0["env"]["K"], "v");
    assert_eq!(catalog::servers(db).await.unwrap().len(), before + 1);
    assert_eq!(catalog::tools_by_server(db, row.id).await.unwrap().len(), 2);

    // duplicate without replace → unique violation; with replace → new id, old entities gone
    assert!(
        registry::create_server(db, &server, &tools, &prompts, &resources, false).await.is_err()
    );
    let row2 = registry::create_server(db, &server, &tools[..1], &[], &[], true).await.unwrap();
    assert_ne!(row.id, row2.id);
    assert_eq!(catalog::servers(db).await.unwrap().len(), before + 1);
    assert_eq!(catalog::tools_by_server(db, row.id).await.unwrap().len(), 0);
    assert_eq!(catalog::tools_by_server(db, row2.id).await.unwrap().len(), 1);

    // flags
    assert_eq!(registry::set_server_enabled(db, "newsrv", false).await.unwrap(), Some(true));
    assert_eq!(registry::set_server_enabled(db, "newsrv", false).await.unwrap(), Some(false));
    assert_eq!(registry::set_server_enabled(db, "nope", false).await.unwrap(), None);
    assert_eq!(registry::set_tools_enabled(db, row2.id, None, false).await.unwrap(), vec!["a"]);
    assert!(registry::set_tools_enabled(db, row2.id, None, false).await.unwrap().is_empty());
    assert_eq!(registry::set_tools_enabled(db, row2.id, Some("a"), true).await.unwrap(), vec!["a"]);
    assert!(registry::set_tools_enabled(db, row2.id, Some("zzz"), true).await.unwrap().is_empty());

    // delete
    assert!(registry::delete_server(db, "newsrv").await.unwrap());
    assert!(!registry::delete_server(db, "newsrv").await.unwrap());
    assert_eq!(catalog::servers(db).await.unwrap().len(), before);
    assert_eq!(catalog::tools_by_server(db, row2.id).await.unwrap().len(), 0);
}

#[tokio::test]
async fn sqlite_registry_writes() {
    let (db, _dir) = sqlite_from_fixture("legacy-enterprise.sqlite").await;
    db.migrate().await.unwrap();
    assert_registry_writes(&db).await;
    db.close().await;
}

#[tokio::test]
async fn postgres_registry_writes() {
    let Some((db, admin, name)) = postgres_from_fixture().await else {
        eprintln!("TRUNKS_TEST_PG_URL not set — skipping PostgreSQL registry write test");
        return;
    };
    db.migrate().await.unwrap();
    assert_registry_writes(&db).await;
    db.close().await;
    drop_pg(&admin, &name).await;
}

#[tokio::test]
async fn sqlite_enterprise_fixture() {
    let (db, _dir) = sqlite_from_fixture("legacy-enterprise.sqlite").await;
    assert_enterprise_fixture(&db).await;
    db.close().await;
}

#[tokio::test]
async fn sqlite_dev_fixture_has_no_identities() {
    let (db, _dir) = sqlite_from_fixture("legacy-dev.sqlite").await;
    db.migrate().await.unwrap();
    assert_eq!(config::get(&db).await.unwrap().unwrap().mode, "development");
    assert!(identity::users(&db).await.unwrap().is_empty());
    assert!(identity::clients(&db).await.unwrap().is_empty());
    assert_eq!(catalog::servers(&db).await.unwrap().len(), 4);
}

#[tokio::test]
async fn sqlite_fresh_database_initialises() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::connect(&format!("sqlite://{}/fresh.db", dir.path().display()), 2).await.unwrap();
    assert_eq!(db.migrate().await.unwrap().versions, vec![1, 2, 3]);
    let store = Store(db.clone());
    assert!(store.get().await.unwrap().is_none());
    assert!(store.init(ServerMode::Development).await.unwrap());
    assert!(!store.init(ServerMode::Development).await.unwrap());
    assert_eq!(store.get().await.unwrap().unwrap().mode, ServerMode::Development);
    assert!(catalog::servers(&db).await.unwrap().is_empty());
}

#[tokio::test]
async fn postgres_enterprise_fixture() {
    let Some((db, admin, name)) = postgres_from_fixture().await else {
        eprintln!("TRUNKS_TEST_PG_URL not set — skipping PostgreSQL fixture test");
        return;
    };
    assert_enterprise_fixture(&db).await;
    db.close().await;
    drop_pg(&admin, &name).await;
}
