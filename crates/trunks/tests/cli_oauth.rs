//! The CLI's OAuth round-trip against a live `trunks start` and the fake authorization server:
//! `register` starts the loopback listener, prints the authorization URL, and completes the
//! registration once the "browser" hits the callback.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead as _, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_trunks"))
}

async fn free_port() -> u16 {
    tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port()
}

#[tokio::test]
async fn register_completes_oauth_through_the_loopback_callback() {
    let fake = trunks_testing::oauth::Fake::start(true, 3600).await;
    let dir = tempfile::tempdir().unwrap();
    let port = free_port().await;
    let registry = format!("http://127.0.0.1:{port}");

    let mut server = Command::new(bin())
        .arg("start")
        .env("TRUNKS_PORT", port.to_string())
        .env("TRUNKS_DATABASE_URL", format!("sqlite://{}/t.db", dir.path().display()))
        .env("TRUNKS_LOG", "warn")
        .env("HOME", dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let http = reqwest::Client::new();
    for _ in 0..100 {
        if http.get(format!("{registry}/health")).send().await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // `register` blocks waiting for the callback; read its stdout on a thread.
    let mut reg = Command::new(bin())
        .args(["register", "--name", "prot", "--url", &fake.mcp_url(), "--oauth-scope", "mcp:use"])
        .env("TRUNKS_REGISTRY", &registry)
        .env("HOME", dir.path())
        .env("PATH", "/nonexistent") // no browser opener: the URL is printed instead
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = reg.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let mut lines = Vec::new();
    let auth_url = loop {
        let line = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("register printed the authorization URL");
        lines.push(line.clone());
        if let Some(u) = line.trim().strip_prefix("http") {
            break format!("http{u}");
        }
    };
    let u = url::Url::parse(&auth_url).unwrap();
    let q: std::collections::HashMap<String, String> = u.query_pairs().into_owned().collect();
    let redirect = q["redirect_uri"].clone();
    assert!(redirect.starts_with("http://127.0.0.1:"), "{redirect}");
    assert!(redirect.ends_with("/oauth/callback"));

    // the "browser" is redirected back with the code
    let cb =
        http.get(format!("{redirect}?code=good-code&state={}", q["state"])).send().await.unwrap();
    assert_eq!(cb.status(), 200);
    assert!(cb.text().await.unwrap().contains("Authorization received"));

    let status = reg.wait().unwrap();
    while let Ok(l) = rx.recv_timeout(Duration::from_secs(2)) {
        lines.push(l);
    }
    let out = lines.join("\n");
    assert!(status.success(), "register failed:\n{out}");
    assert!(out.contains("Server prot registered successfully!"), "{out}");
    assert!(out.contains("prot__search"), "{out}");

    // and the registered server is callable through the gateway with the bearer
    let inv = Command::new(bin())
        .args(["invoke", "prot__search", "--input", r#"{"q":"cli"}"#])
        .env("TRUNKS_REGISTRY", &registry)
        .env("HOME", dir.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&inv.stdout);
    assert!(text.contains("secret result for cli"), "{text}");
    assert!(fake.bearers.snapshot().iter().any(|b| b == "Bearer at-1"));

    let _ = server.kill();
    let _ = server.wait();
}
