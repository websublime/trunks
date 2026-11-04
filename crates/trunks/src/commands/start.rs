//! `trunks start` — runs the HTTP server.
//!
//! Phase 0: binds and serves the public routes with graceful shutdown. Mode,
//! database and catalog wiring arrive in Phase 1 (`docs/04-plano.md`).

use std::net::SocketAddr;

use clap::Args as ClapArgs;
use snafu::{ResultExt, Whatever};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
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
}

fn parse_mode(s: &str) -> Result<ServerMode, String> {
    ServerMode::parse_lenient(s)
        .ok_or_else(|| format!("invalid mode '{s}': expected development or enterprise"))
}

pub async fn run(args: Args) -> Result<(), Whatever> {
    init_tracing();
    tracing::info!(mode = %args.mode, "starting");
    let host = if args.host.is_empty() { "0.0.0.0" } else { args.host.as_str() };
    let addr: SocketAddr =
        format!("{host}:{}", args.port).parse().whatever_context("invalid bind address")?;

    let app = trunks_api::router();
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
