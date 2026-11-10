//! The HTTP accept loop.
//!
//! `axum::serve` has no settings: without a timer hyper never gives up on a client that opens
//! a connection and stalls while sending its request head (slowloris), and a graceful
//! shutdown waits for every open stream for as long as it stays open. This loop sets both.

use std::time::Duration;

use axum::Router;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use tokio::net::{TcpListener, TcpStream};

use crate::HttpOptions;

/// How often an idle HTTP/2 connection is pinged, and how long the peer has to answer.
const HTTP2_KEEP_ALIVE: Duration = Duration::from_secs(30);

/// What an HTTP/2 connection starts with (RFC 9113 §3.4).
const HTTP2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// Waits, for at most `limit`, until the first bytes of a connection tell HTTP/1 from HTTP/2.
/// `false` when the client closed or did not send them in time.
///
/// hyper only starts its header timeout once it knows the protocol, and waits without limit
/// for the bytes that decide it — a client that connects and sends nothing (or a fragment
/// of the HTTP/2 preface) would hold its connection forever.
async fn protocol_decided(stream: &TcpStream, limit: Duration) -> bool {
    let decided = async {
        let mut first = [0u8; HTTP2_PREFACE.len()];
        loop {
            match stream.peek(&mut first).await {
                Ok(0) | Err(_) => return false,
                Ok(n) if n == first.len() || first[..n] != HTTP2_PREFACE[..n] => return true,
                // A proper prefix of the preface: `peek` would return the same bytes at
                // once, so poll. Real HTTP/2 clients send the preface in one piece.
                Ok(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
    };
    tokio::time::timeout(limit, decided).await.unwrap_or(false)
}

/// Serves `app` on `listener` until `shutdown` resolves. Then no new connection is accepted
/// and open ones get `opts.shutdown_grace` to finish their requests before they are dropped.
pub async fn serve(
    listener: TcpListener,
    app: Router,
    opts: HttpOptions,
    shutdown: impl Future<Output = ()>,
) {
    let mut builder = Builder::new(TokioExecutor::new());
    builder.http1().timer(TokioTimer::new()).header_read_timeout(opts.header_read_timeout);
    builder
        .http2()
        .timer(TokioTimer::new())
        .keep_alive_interval(Some(HTTP2_KEEP_ALIVE))
        .keep_alive_timeout(HTTP2_KEEP_ALIVE);

    let graceful = GracefulShutdown::new();
    let mut shutdown = std::pin::pin!(shutdown);
    loop {
        let stream = tokio::select! {
            () = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                // The client went away between connecting and being accepted.
                Err(e) if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionRefused
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::ConnectionReset
                ) => continue,
                // Typically "too many open files": back off instead of spinning.
                Err(e) => {
                    tracing::error!(error = %e, "accepting a connection failed");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            },
        };
        let service = TowerToHyperService::new(app.clone());
        let builder = builder.clone();
        let watcher = graceful.watcher();
        let head_timeout = opts.header_read_timeout;
        tokio::spawn(async move {
            if !protocol_decided(&stream, head_timeout).await {
                return;
            }
            let connection = builder.serve_connection_with_upgrades(TokioIo::new(stream), service);
            if let Err(e) = watcher.watch(connection).await {
                tracing::trace!(error = %e, "connection closed with an error");
            }
        });
    }

    drop(listener);
    let open = graceful.count();
    if tokio::time::timeout(opts.shutdown_grace, graceful.shutdown()).await.is_err() {
        tracing::warn!(
            open,
            grace_secs = opts.shutdown_grace.as_secs(),
            "connections still open after the shutdown grace period were dropped"
        );
    }
}
