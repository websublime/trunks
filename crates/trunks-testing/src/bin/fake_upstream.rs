//! `fake-upstream` — the test MCP server over stdio.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rmcp::ServiceExt as _;
use trunks_testing::FakeUpstream;

#[tokio::main]
async fn main() {
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
