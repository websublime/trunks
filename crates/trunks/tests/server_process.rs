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
    let http = reqwest::Client::new();
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
