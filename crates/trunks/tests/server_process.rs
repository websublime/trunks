//! The real `trunks start` process: what it writes to its own output while stdio upstreams
//! run under it, and how it stops.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;
use std::time::{Duration, Instant};

const UPSTREAM_SECRET: &str = "process-env-secret-4be1c9d2";

fn bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_trunks"))
}

async fn free_port() -> u16 {
    tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port()
}

#[tokio::test]
async fn upstream_stderr_is_logged_without_secrets_and_the_server_stops_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let out_path = dir.path().join("server.out");
    let out = std::fs::File::create(&out_path).unwrap();

    let mut server = Command::new(bin())
        .arg("start")
        .env("TRUNKS_PORT", port.to_string())
        .env("TRUNKS_DATABASE_URL", format!("sqlite://{}/t.db", dir.path().display()))
        .env("TRUNKS_LOG", "info")
        .env("NO_COLOR", "1")
        .env("HOME", dir.path())
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .spawn()
        .unwrap();
    // No connection reuse: the server closes idle connections after 30 s, and building the
    // fake upstream below can block for longer than that on a clean target.
    let http = reqwest::Client::builder().pool_max_idle_per_host(0).build().unwrap();
    for _ in 0..100 {
        if http.get(format!("{base}/health")).send().await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // a stdio upstream that prints one of its (secret) environment values to stderr
    let res = http
        .post(format!("{base}/api/v0/servers"))
        .json(&serde_json::json!({
            "name": "noisy", "transport": "stdio", "description": "",
            "command": trunks_testing::fake_upstream_bin(),
            "env": {"API_KEY": UPSTREAM_SECRET, "FAKE_UPSTREAM_STDERR_ENV": "API_KEY"},
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());
    let res = http
        .post(format!("{base}/api/v0/tools/invoke"))
        .json(&serde_json::json!({"name": "noisy__echo", "input": {"text": "hi"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    // the upstream's stderr is forwarded by a background task: wait until it shows up
    for _ in 0..100 {
        if std::fs::read_to_string(&out_path).unwrap().contains("upstream.stderr") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // stop: SIGTERM where it exists, so the graceful path is the one exercised
    let started = Instant::now();
    #[cfg(unix)]
    {
        let status =
            Command::new("kill").args(["-TERM", &server.id().to_string()]).status().unwrap();
        assert!(status.success());
    }
    #[cfg(not(unix))]
    server.kill().unwrap();
    let status = server.wait().unwrap();
    let output = std::fs::read_to_string(&out_path).unwrap();
    #[cfg(unix)]
    {
        assert!(status.success(), "exit status {status}:\n{output}");
        assert!(output.contains("server stopped"), "{output}");
        assert!(started.elapsed() < Duration::from_secs(10), "shutdown took too long");
    }
    let _ = (status, started);

    assert!(
        !output.contains(UPSTREAM_SECRET),
        "an upstream's secret reached the server's output:\n{output}"
    );
    let stderr_lines: Vec<&str> =
        output.lines().filter(|l| l.contains("upstream.stderr")).collect();
    assert!(
        // every `env` value is a secret, so the variable's name (itself the value of
        // FAKE_UPSTREAM_STDERR_ENV) is blanked too
        stderr_lines
            .iter()
            .any(|l| l.contains("starting with [redacted]=[redacted]") && l.contains("noisy")),
        "the upstream's stderr should be logged, redacted:\n{output}"
    );
    assert!(!output.contains('\u{1b}'), "control characters must be escaped:\n{output}");
}

/// Starts `trunks start` (development mode) and waits until it answers.
async fn start_server(dir: &std::path::Path) -> (std::process::Child, String) {
    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let server = Command::new(bin())
        .arg("start")
        .env("TRUNKS_PORT", port.to_string())
        .env("TRUNKS_DATABASE_URL", format!("sqlite://{}/t.db", dir.display()))
        .env("TRUNKS_LOG", "warn")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let http = reqwest::Client::builder().pool_max_idle_per_host(0).build().unwrap();
    for _ in 0..100 {
        if http.get(format!("{base}/health")).send().await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    (server, base)
}

#[tokio::test]
async fn small_mcp_answers_are_not_held_back_by_the_tcp_stack() {
    use rmcp::ServiceExt as _;
    use rmcp::model::CallToolRequestParams;
    use rmcp::transport::StreamableHttpClientTransport;
    use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

    let fake = trunks_testing::fake_upstream_bin();
    let dir = tempfile::tempdir().unwrap();
    let (mut server, base) = start_server(dir.path()).await;
    // an upstream whose session is kept open, so that a call costs next to nothing
    let res = reqwest::Client::new()
        .post(format!("{base}/api/v0/servers"))
        .json(&serde_json::json!({
            "name": "fs", "transport": "stdio", "session_mode": "stateful", "command": fake,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201, "{}", res.text().await.unwrap());

    // A client that keeps its connection open between calls, as most HTTP clients do. (The
    // default client of `rmcp` opens a new connection per request, and new connections are
    // acknowledged at once — it would never see the delay.)
    let client = ()
        .serve(StreamableHttpClientTransport::with_client(
            reqwest::Client::new(),
            StreamableHttpClientTransportConfig::with_uri(format!("{base}/mcp")),
        ))
        .await
        .unwrap();
    // The answer to a tool call is a response head, sent at once, followed by a small event
    // when the upstream has answered. Unless the server disables Nagle's algorithm, that
    // event waits for the client's delayed acknowledgement of the head, and every call takes
    // more than 40 ms on Linux — from the second call on a connection, which is why the
    // first ones are not measured. Only the fastest call is checked, so a busy machine cannot fail this —
    // but a server that delays every answer does.
    let mut fastest = Duration::MAX;
    for call in 0..25 {
        let started = Instant::now();
        client.call_tool(CallToolRequestParams::new("fs__echo")).await.unwrap();
        if call >= 5 {
            fastest = fastest.min(started.elapsed());
        }
    }
    let _ = client.cancel().await;
    server.kill().unwrap();
    server.wait().unwrap();
    assert!(
        fastest < Duration::from_millis(30),
        "even the fastest of 20 small MCP calls took {fastest:?}"
    );
}

#[tokio::test]
async fn the_database_url_file_wins_and_the_version_and_metrics_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let port = free_port().await;
    let base = format!("http://127.0.0.1:{port}");
    let url_file = dir.path().join("database-url");
    std::fs::write(&url_file, format!("sqlite://{}/from-file.db\n", dir.path().display())).unwrap();

    let mut server = Command::new(bin())
        .arg("start")
        .env("TRUNKS_PORT", port.to_string())
        .env("TRUNKS_DATABASE_URL", format!("sqlite://{}/from-env.db", dir.path().display()))
        .env("TRUNKS_DATABASE_URL_FILE", &url_file)
        .env("TRUNKS_METRICS_ENABLED", "true")
        .env("TRUNKS_LOG", "warn")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let http = reqwest::Client::builder().pool_max_idle_per_host(0).build().unwrap();
    for _ in 0..100 {
        if http.get(format!("{base}/health")).send().await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // the file, not the variable, decided where the database is
    assert!(dir.path().join("from-file.db").exists());
    assert!(!dir.path().join("from-env.db").exists());

    // one version everywhere: `v<package version>`, plus the commit for a non-release build
    let metadata: serde_json::Value =
        http.get(format!("{base}/metadata")).send().await.unwrap().json().await.unwrap();
    let version = metadata["version"].as_str().unwrap().to_owned();
    let package = format!("v{}", env!("CARGO_PKG_VERSION"));
    assert!(version == package || version.starts_with(&format!("{package}+")), "{version}");
    let metrics = http.get(format!("{base}/metrics")).send().await.unwrap().text().await.unwrap();
    assert!(
        metrics.contains(&format!("trunks_build_info{{version=\"{version}\"}} 1")),
        "{metrics}"
    );
    let cli = Command::new(bin()).arg("version").env("TRUNKS_REGISTRY", &base).output().unwrap();
    let cli = String::from_utf8_lossy(&cli.stdout).into_owned();
    assert!(cli.contains(&format!("CLI Version: {version}")), "{cli}");
    assert!(cli.contains(&format!("Server Version: {version}")), "{cli}");

    // the upstream metrics are exposed
    for metric in ["trunks_upstream_sessions_active", "trunks_upstream_connect_errors"] {
        assert!(metrics.contains(&format!("# TYPE {metric} ")), "{metric} missing:\n{metrics}");
    }

    server.kill().unwrap();
    server.wait().unwrap();

    // a file that cannot be read stops the start-up and is named in the error
    let out = Command::new(bin())
        .arg("start")
        .env("TRUNKS_PORT", port.to_string())
        .env("TRUNKS_DATABASE_URL_FILE", dir.path().join("missing"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(err.contains("cannot read the database URL file") && err.contains("missing"), "{err}");
}

/// Starts `trunks start` in `mode` with its own defaults for the listen address and the log
/// format, sends one request, stops it and returns what it wrote to standard output.
async fn output_of_a_default_start(mode: &str, port: u16) -> String {
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("server.out");
    let mut server = Command::new(bin())
        .arg("start")
        .env_remove("TRUNKS_BIND_HOST")
        .env("TRUNKS_MODE", mode)
        .env("TRUNKS_PORT", port.to_string())
        .env("TRUNKS_DATABASE_URL", format!("sqlite://{}/t.db", dir.path().display()))
        .env("TRUNKS_LOG", "info")
        .env("NO_COLOR", "1")
        .stdout(std::fs::File::create(&out_path).unwrap())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let http = reqwest::Client::builder().pool_max_idle_per_host(0).build().unwrap();
    let health = format!("http://127.0.0.1:{port}/health");
    let mut up = false;
    for _ in 0..100 {
        if http.get(&health).send().await.is_ok() {
            up = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    server.kill().unwrap();
    let _ = server.wait();
    let output = std::fs::read_to_string(&out_path).unwrap();
    assert!(up, "the {mode} server never answered on 127.0.0.1:\n{output}");
    output
}

/// Development mode has no authentication, so by default it listens on this machine only
/// and writes for a person. Enterprise mode listens everywhere and writes JSON, one object
/// per line and nothing else, for whatever collects the logs.
#[tokio::test]
async fn the_listen_address_and_the_log_format_follow_the_mode() {
    let port = free_port().await;
    let output = output_of_a_default_start("development", port).await;
    assert!(
        output.contains(&format!("Trunks HTTP server listening on 127.0.0.1:{port}")),
        "{output}"
    );
    let logged = output.lines().find(|l| l.contains("GET") && l.contains("/health"));
    let logged = logged.unwrap_or_else(|| panic!("no access log line:\n{output}"));
    assert!(serde_json::from_str::<serde_json::Value>(logged).is_err(), "text, not JSON: {logged}");

    let port = free_port().await;
    let output = output_of_a_default_start("enterprise", port).await;
    let events: Vec<serde_json::Value> = output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON ({e}): {l}")))
        .collect();
    assert!(events.iter().all(|e| e["level"].is_string() && e["timestamp"].is_string()));
    let message = |e: &serde_json::Value| e["fields"]["message"].as_str().unwrap_or("").to_owned();
    let listening = events.iter().find(|e| message(e) == "Trunks HTTP server listening");
    let listening = listening.unwrap_or_else(|| panic!("no listening event:\n{output}"));
    assert_eq!(listening["fields"]["addr"], format!("0.0.0.0:{port}"));
    assert!(
        events
            .iter()
            .any(|e| e["level"] == "WARN" && message(e).contains("Server is not initialized")),
        "{output}"
    );
    assert!(
        events.iter().any(|e| e.to_string().contains("/health")),
        "the access log is JSON too:\n{output}"
    );
}

/// `trunks list tools | head`: the reader goes away while the command still has lines to
/// print. That is not an error, and certainly not a panic.
#[test]
fn a_closed_output_pipe_ends_a_command_quietly() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(bin())
        .args(["--registry", "http://127.0.0.1:1", "version"])
        .env("TRUNKS_CONFIG", dir.path().join("config.toml"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{:?}: {stderr}", out.status);
    assert!(stderr.is_empty(), "nothing to report: {stderr}");
}
