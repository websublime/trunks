//! End-to-end latency of the gateway, measured from outside the real `trunks start` process
//! (optimized build) with an MCP client over Streamable HTTP:
//!
//! - `tools/list` with a 500-tool catalog (10 upstreams × 50 tools), in development mode and
//!   in enterprise mode with and without an allow-list;
//! - `tools/call` through stateless and stateful upstream sessions, over stdio and HTTP,
//!   next to the same call made straight to the upstream.
//!
//! Run with `cargo bench -p trunks --bench gateway`; the output is the Markdown published in
//! `docs/07-desempenho.md`. Requests are sequential, on one MCP session.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::too_many_lines)]
#![allow(clippy::cast_precision_loss, clippy::print_stdout)]

#[allow(dead_code)]
#[path = "../tests/conformance/gateway.rs"]
mod gateway;

use std::time::{Duration, Instant};

use gateway::{Backend, Gateway, McpClient, Mode};
use reqwest::Method;
use rmcp::ServiceExt as _;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use serde_json::json;

const SERVERS: usize = 10;
const TOOLS_PER_SERVER: usize = 50;
const WARM_UP: usize = 20;

/// One measured operation.
struct Sample {
    what: String,
    latencies: Vec<Duration>,
}

impl Sample {
    fn percentile(&self, p: f64) -> Duration {
        let mut sorted = self.latencies.clone();
        sorted.sort();
        let rank = (p / 100.0 * (sorted.len() - 1) as f64).round();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        sorted[rank as usize]
    }

    fn mean(&self) -> Duration {
        self.latencies.iter().sum::<Duration>() / u32::try_from(self.latencies.len()).unwrap()
    }

    fn row(&self) -> String {
        let ms = |d: Duration| format!("{:.2}", d.as_secs_f64() * 1000.0);
        format!(
            "| {} | {} | {} | {} | {} | {} |",
            self.what,
            self.latencies.len(),
            ms(self.mean()),
            ms(self.percentile(50.0)),
            ms(self.percentile(95.0)),
            ms(self.percentile(99.0)),
        )
    }
}

/// Runs `op` `WARM_UP` times unmeasured, then `iterations` times measured.
async fn measure<F, Fut>(what: impl Into<String>, iterations: usize, mut op: F) -> Sample
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    for _ in 0..WARM_UP {
        op().await;
    }
    let mut latencies = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        op().await;
        latencies.push(started.elapsed());
    }
    Sample { what: what.into(), latencies }
}

async fn register(gw: &Gateway, token: Option<&str>, definition: serde_json::Value) {
    let (status, body) = gw.rest(Method::POST, "/api/v0/servers", token, Some(definition)).await;
    assert_eq!(status, 201, "{body}");
}

/// Registers the 10 × 50 catalog.
async fn register_catalog(gw: &Gateway, token: Option<&str>, fake: &str) {
    for i in 0..SERVERS {
        register(
            gw,
            token,
            json!({
                "name": format!("bulk{i}"), "transport": "stdio", "command": fake,
                "env": {"FAKE_UPSTREAM_EXTRA_TOOLS": (TOOLS_PER_SERVER - 3).to_string()},
            }),
        )
        .await;
    }
}

async fn list_tools(what: &str, client: &McpClient, expected: usize) -> Sample {
    let listed = client.list_all_tools().await.unwrap();
    assert_eq!(listed.len(), expected, "{what}");
    let bytes = serde_json::to_vec(&listed).unwrap().len();
    let what = format!("{what} — {expected} tools, {} KiB", bytes / 1024);
    measure(what, 300, || async {
        assert_eq!(client.list_all_tools().await.unwrap().len(), expected);
    })
    .await
}

async fn call_echo(what: &str, iterations: usize, client: &McpClient, tool: &str) -> Sample {
    let params = || {
        CallToolRequestParams::new(tool.to_owned())
            .with_arguments(json!({"text": "ping"}).as_object().unwrap().clone())
    };
    measure(what, iterations, || async {
        let result = client.call_tool(params()).await.unwrap();
        assert_eq!(gateway::text_of(&result), "ping");
    })
    .await
}

fn machine() -> String {
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|info| {
            info.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, model)| model.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown CPU".to_owned());
    let cores = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    format!("{cpu}, {cores} logical cores, {} {}", std::env::consts::OS, std::env::consts::ARCH)
}

#[tokio::main]
async fn main() {
    // `cargo bench` passes `--bench`; `cargo test --benches` passes nothing and only wants to
    // know that this target runs.
    if !std::env::args().any(|a| a == "--bench") {
        println!("gateway benchmark: pass --bench (cargo bench -p trunks --bench gateway) to run");
        return;
    }
    let fake = trunks_testing::fake_upstream_bin().to_string_lossy().into_owned();
    let web = trunks_testing::serve_http().await;
    let mut listing = Vec::new();
    let mut calling = Vec::new();

    // ── development mode ─────────────────────────────────────────────────────────────
    let dev = Gateway::start(Backend::Sqlite, Mode::Development).await.unwrap();
    register_catalog(&dev, None, &fake).await;
    for (name, transport, mode) in [
        ("stdio-stateless", "stdio", "stateless"),
        ("stdio-stateful", "stdio", "stateful"),
        ("http-stateless", "streamable_http", "stateless"),
        ("http-stateful", "streamable_http", "stateful"),
    ] {
        register(
            &dev,
            None,
            json!({
                "name": name, "transport": transport, "session_mode": mode,
                "command": fake, "url": web,
            }),
        )
        .await;
    }
    let total = SERVERS * TOOLS_PER_SERVER + 4 * 3;
    let client = dev.mcp("/mcp", None).await.unwrap();
    listing.push(list_tools("development", &client, total).await);

    calling.push(
        call_echo("stdio, stateless (a process per call)", 200, &client, "stdio-stateless__echo")
            .await,
    );
    calling.push(
        call_echo("stdio, stateful (session reused)", 1000, &client, "stdio-stateful__echo").await,
    );
    calling.push(
        call_echo("HTTP, stateless (a session per call)", 500, &client, "http-stateless__echo")
            .await,
    );
    calling.push(
        call_echo("HTTP, stateful (session reused)", 1000, &client, "http-stateful__echo").await,
    );
    let _ = client.cancel().await;

    // the same call without the gateway in between
    let direct: McpClient =
        ().serve(TokioChildProcess::new(tokio::process::Command::new(&fake)).unwrap())
            .await
            .unwrap();
    calling.push(call_echo("stdio, straight to the upstream", 1000, &direct, "echo").await);
    let _ = direct.cancel().await;
    let direct: McpClient = ()
        .serve(StreamableHttpClientTransport::with_client(
            reqwest::Client::new(),
            StreamableHttpClientTransportConfig::with_uri(web.clone()),
        ))
        .await
        .unwrap();
    calling.push(call_echo("HTTP, straight to the upstream", 1000, &direct, "echo").await);
    let _ = direct.cancel().await;
    dev.finish().await;

    // ── enterprise mode: the listing is filtered per client ──────────────────────────
    let ent = Gateway::start(Backend::Sqlite, Mode::Enterprise).await.unwrap();
    let (status, init) =
        ent.rest(Method::POST, "/init", None, Some(json!({"mode": "enterprise"}))).await;
    assert_eq!(status, 200, "{init}");
    let admin = init["admin_access_token"].as_str().unwrap().to_owned();
    register_catalog(&ent, Some(&admin), &fake).await;
    for (name, allow, token) in [
        ("everything", "*", "bench-client-everything"),
        ("one", "bulk0", "bench-client-one-server"),
    ] {
        let (status, body) = ent
            .rest(
                Method::POST,
                "/api/v0/clients",
                Some(&admin),
                Some(json!({"name": name, "allow_list": [allow], "access_token": token})),
            )
            .await;
        assert_eq!(status, 201, "{body}");
    }
    let client = ent.mcp("/mcp", Some("bench-client-everything")).await.unwrap();
    listing.push(
        list_tools("enterprise, client allowed everything", &client, SERVERS * TOOLS_PER_SERVER)
            .await,
    );
    let _ = client.cancel().await;
    let client = ent.mcp("/mcp", Some("bench-client-one-server")).await.unwrap();
    listing.push(
        list_tools("enterprise, client allowed 1 of 10 servers", &client, TOOLS_PER_SERVER).await,
    );
    let _ = client.cancel().await;
    ent.finish().await;

    // ── report ───────────────────────────────────────────────────────────────────────
    let header = "| | n | mean | p50 | p95 | p99 |\n|---|---:|---:|---:|---:|---:|";
    println!("Machine: {}. Latencies in milliseconds.\n", machine());
    println!("### `tools/list`\n\n{header}");
    for sample in &listing {
        println!("{}", sample.row());
    }
    println!("\n### `tools/call` (`echo`)\n\n{header}");
    for sample in &calling {
        println!("{}", sample.row());
    }
}
