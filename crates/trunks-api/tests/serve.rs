//! The accept loop (`trunks_api::serve`): stalled clients are disconnected and shutdown is
//! graceful but bounded.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::routing::get;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, oneshot};
use tokio::task::JoinHandle;
use trunks_api::HttpOptions;

/// `entered` is notified when a `/slow` or `/stuck` handler starts running.
fn app(entered: Arc<Notify>) -> Router {
    let (slow, stuck) = (entered.clone(), entered);
    Router::new()
        .route("/fast", get(|| async { "fast" }))
        .route(
            "/slow",
            get(move || async move {
                slow.notify_one();
                tokio::time::sleep(Duration::from_millis(400)).await;
                "slow"
            }),
        )
        .route(
            "/stuck",
            get(move || async move {
                stuck.notify_one();
                tokio::time::sleep(Duration::from_mins(10)).await;
                "never"
            }),
        )
}

struct Server {
    addr: SocketAddr,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
    /// Resolves once a slow handler is running.
    entered: Arc<Notify>,
}

async fn start(opts: HttpOptions) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let entered = Arc::new(Notify::new());
    let task = tokio::spawn(trunks_api::serve(listener, app(entered.clone()), opts, async move {
        let _ = stopped.await;
    }));
    Server { addr, stop, task, entered }
}

/// Sends `request` and returns everything the server answers until it closes the connection.
async fn exchange(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    // A reset counts as closed: that is what dropping a socket with unread input looks like.
    let _closed_or_reset =
        tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut response))
            .await
            .expect("the server should have closed the connection");
    String::from_utf8_lossy(&response).into_owned()
}

fn get_request(path: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
}

#[tokio::test]
async fn a_client_that_never_finishes_its_request_head_is_disconnected() {
    let opts =
        HttpOptions { header_read_timeout: Duration::from_millis(300), ..HttpOptions::default() };
    let server = start(opts).await;
    let addr = server.addr;

    // a complete request is served
    let response = exchange(addr, &get_request("/fast")).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("fast"), "{response}");

    // a request head that never ends (no blank line) is cut off after the timeout
    let started = Instant::now();
    let response = exchange(addr, "GET /fast HTTP/1.1\r\nHost: localhost\r\n").await;
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(250), "cut off too early: {waited:?}");
    assert!(waited < Duration::from_secs(5), "cut off too late: {waited:?}");
    assert!(!response.contains("200"), "a stalled request must not be served: {response}");

    // so is a connection that never sends anything, or stalls inside the HTTP/2 preface —
    // before the server even knows which protocol it is speaking
    for opening in ["", "PRI * HT"] {
        let started = Instant::now();
        let response = exchange(addr, opening).await;
        let waited = started.elapsed();
        assert!(waited >= Duration::from_millis(250), "{opening:?} cut off too early");
        assert!(waited < Duration::from_secs(5), "{opening:?} cut off too late: {waited:?}");
        assert!(response.is_empty(), "{opening:?}: {response}");
    }
}

#[tokio::test]
async fn shutdown_lets_requests_in_flight_finish_and_stops_accepting() {
    let Server { addr, stop, task, entered } = start(HttpOptions::default()).await;

    let in_flight = tokio::spawn(async move { exchange(addr, &get_request("/slow")).await });
    entered.notified().await; // the request is being handled
    let started = Instant::now();
    stop.send(()).unwrap();

    let response = in_flight.await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200") && response.ends_with("slow"), "{response}");
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("serve returns once the last request is done")
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(TcpStream::connect(addr).await.is_err(), "the listener is closed");
}

#[tokio::test]
async fn shutdown_does_not_wait_forever_for_a_request_that_never_ends() {
    let opts = HttpOptions { shutdown_grace: Duration::from_millis(300), ..HttpOptions::default() };
    let Server { addr, stop, task, entered } = start(opts).await;

    let mut stuck = TcpStream::connect(addr).await.unwrap();
    stuck.write_all(get_request("/stuck").as_bytes()).await.unwrap();
    entered.notified().await;
    let started = Instant::now();
    stop.send(()).unwrap();

    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("serve returns after the grace period")
        .unwrap();
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(250), "the grace period was not honoured: {waited:?}");
}
