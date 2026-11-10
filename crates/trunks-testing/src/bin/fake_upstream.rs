//! `fake-upstream` — the test MCP server over stdio.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rmcp::ServiceExt as _;
use trunks_testing::FakeUpstream;

#[tokio::main]
async fn main() {
    // Descendant mode: hold a TCP connection open and never exit by itself. Whoever accepted
    // the connection sees it close when — and only when — this process is killed.
    if let Ok(addr) = std::env::var("FAKE_UPSTREAM_LINGER_CONNECT") {
        let _socket = std::net::TcpStream::connect(addr).expect("connecting to the test");
        loop {
            std::thread::sleep(std::time::Duration::from_mins(1));
        }
    }
    // Like servers started through a launcher (`npx`, a shell script), leave a process of
    // our own behind: it ignores stdin and outlives this one unless the whole tree is killed.
    if let Ok(addr) = std::env::var("FAKE_UPSTREAM_SPAWN_DESCENDANT") {
        // Never waited for, on purpose: an abandoned process is what the tests need.
        #[allow(clippy::zombie_processes)]
        let _abandoned = std::process::Command::new(std::env::current_exe().expect("own path"))
            .env_remove("FAKE_UPSTREAM_SPAWN_DESCENDANT")
            .env("FAKE_UPSTREAM_LINGER_CONNECT", addr)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawning the descendant");
    }
    // Like many real servers, log to stderr at start-up — including, when asked to, the value
    // of one of its environment variables (to test that the gateway never repeats it).
    if let Ok(name) = std::env::var("FAKE_UPSTREAM_STDERR_ENV") {
        let value = std::env::var(&name).unwrap_or_default();
        eprintln!("fake-upstream: starting with {name}={value}");
        eprintln!("fake-upstream: \u{1b}[31mready\u{1b}[0m");
    }
    let service = FakeUpstream.serve(rmcp::transport::stdio()).await.expect("serve stdio");
    let _ = service.waiting().await;
}
