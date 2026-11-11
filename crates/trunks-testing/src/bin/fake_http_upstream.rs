//! `fake-http-upstream` — the test MCP server over Streamable HTTP, for tests that drive the
//! real binary from outside (the web console's smoke tests). Prints its MCP URL on the first
//! line of stdout and serves until it is killed.
//!
//! With `--oauth` the server is protected: it answers 401 with an OAuth challenge until it is
//! given a token from the fake provider started next to it (dynamic client registration, an
//! authorization endpoint that approves at once).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::io::Write as _;

#[tokio::main]
async fn main() {
    // Kept alive until the process ends: dropping it stops the provider and the upstream.
    let _protected;
    let url = if std::env::args().any(|arg| arg == "--oauth") {
        let fake = trunks_testing::oauth::Fake::start(true, 3600).await;
        let url = fake.mcp_url();
        _protected = Some(fake);
        url
    } else {
        _protected = None;
        trunks_testing::serve_http().await
    };
    println!("{url}");
    std::io::stdout().flush().expect("stdout");
    std::future::pending::<()>().await;
}
