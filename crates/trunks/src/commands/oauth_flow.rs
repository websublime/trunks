//! The CLI side of upstream OAuth: a loopback listener for the authorization callback, the
//! browser hand-off, and the `complete` call.

use std::time::Duration;

use axum::extract::Query;
use axum::response::Html;
use axum::routing::get;
use snafu::{ResultExt, Whatever};
use tokio::sync::{mpsc, oneshot};
use trunks_client::Client;
use trunks_types::api::{AuthorizationRequired, McpServer};

/// `http://127.0.0.1:<port>/oauth/callback` on an ephemeral port, plus the server task.
pub struct Listener {
    pub redirect_uri: String,
    rx: mpsc::Receiver<Callback>,
    shutdown: Option<oneshot::Sender<()>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Callback {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

const PAGE_OK: &str = "<!doctype html><title>Trunks</title><body style=\"font-family:system-ui;padding:2rem\"><h2>Authorization received</h2><p>You can close this window and return to the terminal.</p>";
const PAGE_ERR: &str = "<!doctype html><title>Trunks</title><body style=\"font-family:system-ui;padding:2rem\"><h2>Authorization failed</h2><p>Return to the terminal for details.</p>";

impl Listener {
    pub async fn start() -> Result<Self, Whatever> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .whatever_context("binding the OAuth callback listener")?;
        let port = listener.local_addr().whatever_context("reading listener address")?.port();
        let (tx, rx) = mpsc::channel::<Callback>(4);
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let app = axum::Router::new().route(
            "/oauth/callback",
            get(move |Query(cb): Query<Callback>| {
                let tx = tx.clone();
                async move {
                    let ok = cb.error.is_none() && cb.code.is_some();
                    let _ = tx.send(cb).await;
                    Html(if ok { PAGE_OK } else { PAGE_ERR })
                }
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stop_rx.await;
                })
                .await;
        });
        Ok(Self {
            redirect_uri: format!("http://127.0.0.1:{port}/oauth/callback"),
            rx,
            shutdown: Some(stop_tx),
        })
    }

    /// Waits for the browser to hit the callback, at most until `deadline`.
    pub async fn wait(&mut self, deadline: Duration) -> Result<Callback, Whatever> {
        match tokio::time::timeout(deadline, self.rx.recv()).await {
            Ok(Some(cb)) => Ok(cb),
            Ok(None) => snafu::whatever!("the callback listener stopped unexpectedly"),
            Err(_) => snafu::whatever!(
                "timed out waiting for the authorization callback; run register again"
            ),
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if let Some(s) = self.shutdown.take() {
            let _ = s.send(());
        }
    }
}

/// Best-effort: opens `url` in the default browser. The URL is always printed as well.
pub fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let cmd = ("open", vec![url.to_owned()]);
    #[cfg(target_os = "windows")]
    let cmd = ("rundll32", vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()]);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let cmd = ("xdg-open", vec![url.to_owned()]);
    let _ = std::process::Command::new(cmd.0)
        .args(cmd.1)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Drives the browser round-trip for a pending authorization and completes it.
pub async fn finish(
    client: &Client,
    listener: &mut Listener,
    auth: &AuthorizationRequired,
) -> Result<McpServer, Whatever> {
    println!("The upstream requires OAuth authorization.");
    println!(
        "Opening your browser; if nothing happens, open this URL yourself:\n\n  {}\n",
        auth.authorization_url
    );
    open_browser(&auth.authorization_url);
    let remaining =
        (auth.expires_at - chrono::Utc::now()).to_std().unwrap_or(Duration::from_secs(60));
    println!(
        "Waiting for the authorization callback (session {} expires at {})…",
        auth.session_id,
        auth.expires_at.format("%H:%M:%S UTC")
    );
    let cb = listener.wait(remaining).await?;
    if let Some(err) = cb.error {
        snafu::whatever!(
            "authorization denied: {err}{}",
            cb.error_description.map(|d| format!(" — {d}")).unwrap_or_default()
        );
    }
    let (Some(code), Some(state)) = (cb.code, cb.state) else {
        snafu::whatever!("the authorization callback carried no code/state");
    };
    let r = client
        .complete_oauth(&auth.session_id, &code, &state)
        .await
        .whatever_context("completing the OAuth authorization")?;
    match r.server {
        Some(s) => Ok(s),
        None => snafu::whatever!("the server did not return the registered server"),
    }
}
