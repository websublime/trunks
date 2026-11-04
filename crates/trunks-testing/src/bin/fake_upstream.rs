//! `fake-upstream` — the test MCP server over stdio.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rmcp::ServiceExt as _;
use trunks_testing::FakeUpstream;

#[tokio::main]
async fn main() {
    let service = FakeUpstream.serve(rmcp::transport::stdio()).await.expect("serve stdio");
    let _ = service.waiting().await;
}
