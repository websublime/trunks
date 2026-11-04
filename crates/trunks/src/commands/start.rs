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
use trunks_core::catalog::Catalog;
use trunks_core::identity::IdentityService;
use trunks_core::metrics::Metrics;
use trunks_core::mode::ModeService;
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
    match mode.current() {
        Some(current) if current != args.mode => {
            snafu::whatever!(
                "server is already initialized in {current} mode, cannot start in {} mode",
                args.mode
            );
        }
        Some(_) => {}
        None if args.mode == ServerMode::Development => {
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

    let catalog = Catalog::empty();
    let snap = catalog.reload(store.as_ref()).await.whatever_context("loading the catalog")?;
    let sse: Vec<_> = snap
        .servers
        .values()
        .filter(|s| s.enabled && !s.config.transport().is_supported())
        .map(|s| s.name.to_string())
        .collect();
    if !sse.is_empty() {
        if std::env::var("TRUNKS_IGNORE_SSE_SERVERS").as_deref() == Ok("1") {
            tracing::warn!(servers = ?sse, "ignoring enabled servers with the unsupported sse transport");
        } else {
            snafu::whatever!(
                "servers {:?} use the unsupported 'sse' transport; re-register them as streamable_http, disable them, or set TRUNKS_IGNORE_SSE_SERVERS=1",
                sse
            );
        }
    }
    tracing::info!(
        servers = snap.servers.len(),
        tools = snap.tools.len(),
        prompts = snap.prompts.len(),
        resources = snap.resources.len(),
        groups = snap.groups.len(),
        "catalog loaded"
    );

    let metrics_enabled = args.metrics.unwrap_or(args.mode.is_enterprise());
    let state = AppState {
        catalog,
        registry: store.clone(),
        identity,
        mode,
        metrics: metrics_enabled.then(|| Metrics::new(&trunks_types::version::version())),
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
    tracing::info!("server stopped");
    Ok(())
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
