//! `trunks start` — runs the HTTP server.
//!
//! Phase 0: binds and serves the public routes with graceful shutdown. Mode,
//! database and catalog wiring arrive in Phase 1 (`docs/04-plano.md`).

use std::net::SocketAddr;

use clap::Args as ClapArgs;
use snafu::{ResultExt, Whatever};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use trunks_api::AppState;
use trunks_api::mcp::{McpMounts, McpOptions};
use trunks_core::catalog::Catalog;
use trunks_core::gateway::GatewayState;
use trunks_core::groups::GroupService;
use trunks_core::identity::IdentityService;
use trunks_core::metrics::Metrics;
use trunks_core::mode::ModeService;
use trunks_core::registry::RegistryService;
use trunks_core::sessions::{SessionManager, SessionOptions};
use trunks_core::upstream::ConnectOptions;
use trunks_db::Db;
use trunks_db::Store;
use trunks_types::ServerMode;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Port to listen on
    #[arg(long, env = "TRUNKS_PORT", default_value_t = 8080)]
    port: u16,

    /// Host to bind to (empty = all interfaces)
    #[arg(long, env = "TRUNKS_BIND_HOST", default_value = "")]
    host: String,

    /// Operating mode: development | enterprise
    #[arg(long, env = "TRUNKS_MODE", default_value = "development", value_parser = parse_mode)]
    mode: ServerMode,

    /// Database URL: sqlite://<path> or postgres://user:pass@host:port/db
    #[arg(long, env = "TRUNKS_DATABASE_URL", default_value = "sqlite://trunks.db")]
    database_url: String,

    /// Expose Prometheus metrics at /metrics (default: on in enterprise mode, off in development)
    #[arg(long, env = "TRUNKS_METRICS_ENABLED")]
    metrics: Option<bool>,

    /// Timeout (seconds) for the MCP initialize handshake with upstream servers
    #[arg(long, env = "TRUNKS_UPSTREAM_INIT_TIMEOUT_SEC", default_value_t = 30)]
    upstream_init_timeout_sec: u64,

    /// Close stateful upstream sessions idle for this many seconds (0 = never)
    #[arg(long, env = "TRUNKS_SESSION_IDLE_TIMEOUT_SEC", default_value_t = 0)]
    session_idle_timeout_sec: u64,

    /// Host header values accepted on the MCP endpoints (comma-separated; "*" disables the check)
    #[arg(long, env = "TRUNKS_ALLOWED_HOSTS", value_delimiter = ',')]
    allowed_hosts: Vec<String>,
}

fn parse_mode(s: &str) -> Result<ServerMode, String> {
    ServerMode::parse_lenient(s)
        .ok_or_else(|| format!("invalid mode '{s}': expected development or enterprise"))
}

pub async fn run(args: Args) -> Result<(), Whatever> {
    init_tracing();
    tracing::info!(mode = %args.mode, db = %trunks_db::redact(&args.database_url), "starting");

    let db =
        Db::connect(&args.database_url, 8).await.whatever_context("database connection failed")?;
    let applied = db.migrate().await.whatever_context("database migration failed")?;
    if !applied.versions.is_empty() {
        tracing::info!(versions = ?applied.versions, "applied migrations");
    }
    let store = Arc::new(Store(db.clone()));
    let identity = IdentityService::new(store.clone());
    let mode = ModeService::load(store.clone(), identity.clone())
        .await
        .whatever_context("reading server configuration")?;
    check_mode(&mode, args.mode).await?;

    let catalog = Catalog::empty();
    let snap = catalog.reload(store.as_ref()).await.whatever_context("loading the catalog")?;
    guard_sse(&snap)?;
    tracing::info!(
        servers = snap.servers.len(),
        tools = snap.tools.len(),
        prompts = snap.prompts.len(),
        resources = snap.resources.len(),
        groups = snap.groups.len(),
        "catalog loaded"
    );

    let metrics_enabled = args.metrics.unwrap_or(args.mode.is_enterprise());
    let metrics = metrics_enabled.then(|| Metrics::new(&trunks_types::version::version()));
    // The session manager connects for gateway calls, so it needs the OAuth store too.
    let connect = ConnectOptions {
        init_timeout: std::time::Duration::from_secs(args.upstream_init_timeout_sec.max(1)),
        oauth: Some(store.clone()),
    };
    let sessions = SessionManager::new(SessionOptions {
        connect: connect.clone(),
        idle_timeout: (args.session_idle_timeout_sec > 0)
            .then(|| std::time::Duration::from_secs(args.session_idle_timeout_sec)),
        ..SessionOptions::default()
    });
    let gateway = Arc::new(GatewayState {
        catalog: catalog.clone(),
        sessions: sessions.clone(),
        metrics: metrics.clone(),
        version: trunks_types::version::version(),
    });
    let registry_service = RegistryService::new(
        store.clone(),
        store.clone(),
        catalog.clone(),
        sessions.clone(),
        connect,
    );
    match registry_service.purge_expired_oauth().await {
        Ok(n) if n > 0 => tracing::info!(count = n, "purged expired OAuth authorization sessions"),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "could not purge expired OAuth sessions"),
    }
    let state = AppState {
        catalog: catalog.clone(),
        gateway: gateway.clone(),
        mcp: McpMounts::new(
            gateway,
            McpOptions {
                allowed_hosts: args
                    .allowed_hosts
                    .iter()
                    .filter(|h| !h.is_empty())
                    .cloned()
                    .collect(),
            },
        ),
        registry_service,
        groups: GroupService::new(store.clone(), catalog.clone()),
        registry: store.clone(),
        identity,
        mode,
        metrics,
    };

    let host = if args.host.is_empty() { "0.0.0.0" } else { args.host.as_str() };
    let addr: SocketAddr =
        format!("{host}:{}", args.port).parse().whatever_context("invalid bind address")?;

    let app = trunks_api::router(state);
    let listener = TcpListener::bind(addr).await.whatever_context("failed to bind")?;

    println!("{}", super::super::BANNER);
    println!("Trunks HTTP server listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .whatever_context("server error")?;
    sessions.shutdown().await;
    tracing::info!("server stopped");
    Ok(())
}

/// Refuses to start in a mode other than the one the database was initialized in;
/// auto-initializes development mode on a fresh database.
async fn check_mode(mode: &ModeService, requested: ServerMode) -> Result<(), Whatever> {
    match mode.current() {
        Some(current) if current != requested => {
            snafu::whatever!(
                "server is already initialized in {current} mode, cannot start in {requested} mode"
            );
        }
        Some(_) => {}
        None if requested == ServerMode::Development => {
            mode.init(ServerMode::Development)
                .await
                .whatever_context("initializing development mode")?;
            tracing::info!("initialized in development mode");
        }
        None => {
            println!(
                "Server is not initialized. Run `trunks init-server` to initialize it in enterprise mode."
            );
        }
    }
    Ok(())
}

/// Enabled servers with the unsupported `sse` transport stop the start-up unless
/// `TRUNKS_IGNORE_SSE_SERVERS=1`.
fn guard_sse(snap: &trunks_core::catalog::Snapshot) -> Result<(), Whatever> {
    let sse: Vec<_> = snap
        .servers
        .values()
        .filter(|s| s.enabled && !s.config.transport().is_supported())
        .map(|s| s.name.to_string())
        .collect();
    if sse.is_empty() {
        return Ok(());
    }
    if std::env::var("TRUNKS_IGNORE_SSE_SERVERS").as_deref() == Ok("1") {
        tracing::warn!(servers = ?sse, "ignoring enabled servers with the unsupported sse transport");
        return Ok(());
    }
    snafu::whatever!(
        "servers {sse:?} use the unsupported 'sse' transport; re-register them as streamable_http, disable them, or set TRUNKS_IGNORE_SSE_SERVERS=1"
    )
}

fn init_tracing() {
    let filter = EnvFilter::try_from_env("TRUNKS_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
