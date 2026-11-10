//! A real `trunks start` process on a chosen database backend, driven from the outside:
//! the CLI, the REST API and an MCP client.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use rmcp::ServiceExt as _;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use trunks_db::{Db, with_pool};

/// Where the PostgreSQL cells find a server on which they may create databases.
const PG_URL_VAR: &str = "TRUNKS_TEST_PG_URL";
/// Set in CI jobs that must not skip the PostgreSQL cells.
const PG_REQUIRED_VAR: &str = "TRUNKS_TEST_REQUIRE_PG";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Sqlite,
    Postgres,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Development,
    Enterprise,
}

impl Mode {
    fn flag(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Enterprise => "enterprise",
        }
    }
}

/// What a CLI invocation produced.
#[derive(Debug)]
pub struct CliOutput {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl CliOutput {
    /// The standard output of a command that must have succeeded.
    #[track_caller]
    pub fn success(self) -> String {
        assert!(self.ok, "the command failed.\nstdout: {}\nstderr: {}", self.stdout, self.stderr);
        self.stdout
    }

    /// The standard error of a command that must have failed.
    #[track_caller]
    pub fn failure(self) -> String {
        assert!(!self.ok, "the command should have failed.\nstdout: {}", self.stdout);
        self.stderr
    }
}

pub type McpClient = rmcp::service::RunningService<rmcp::RoleClient, ()>;

pub struct Gateway {
    pub base: String,
    pub http: reqwest::Client,
    mode: Mode,
    port: u16,
    database_url: String,
    /// `(admin URL, database name)` of the throwaway PostgreSQL database.
    postgres: Option<(String, String)>,
    dir: tempfile::TempDir,
    child: Option<std::process::Child>,
}

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_trunks"))
}

fn unique() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();
    std::process::id().wrapping_mul(0x9E37_79B1) ^ nanos
}

impl Gateway {
    /// Starts a server on an empty database. `None` when the cell is for PostgreSQL and no
    /// server is configured (a hard failure when `TRUNKS_TEST_REQUIRE_PG` is set).
    pub async fn start(backend: Backend, mode: Mode) -> Option<Self> {
        let dir = tempfile::tempdir().unwrap();
        let (database_url, postgres) = match backend {
            Backend::Sqlite => (format!("sqlite://{}/trunks.db", dir.path().display()), None),
            Backend::Postgres => {
                let Ok(admin_url) = std::env::var(PG_URL_VAR) else {
                    assert!(
                        std::env::var_os(PG_REQUIRED_VAR).is_none(),
                        "{PG_REQUIRED_VAR} is set but {PG_URL_VAR} is not"
                    );
                    eprintln!("{PG_URL_VAR} not set — skipping the PostgreSQL cell");
                    return None;
                };
                let name = format!("trunks_conformance_{}", unique());
                let admin = Db::connect(&admin_url, 1).await.unwrap();
                with_pool!(&admin, |p| sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                    "CREATE DATABASE {name}"
                )))
                .execute(p)
                .await
                .map(|_| ())
                .unwrap());
                let base = admin_url.rsplit_once('/').unwrap().0;
                (format!("{base}/{name}"), Some((admin_url, name)))
            }
        };
        let port = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut gateway = Self {
            base: format!("http://127.0.0.1:{port}"),
            // No connection reuse: the server closes connections idle for 30 s, and a test
            // can block for longer than that (building the fake upstream on a clean target)
            // or restart the server — either way a pooled connection would be dead.
            http: reqwest::Client::builder().pool_max_idle_per_host(0).build().unwrap(),
            mode,
            port,
            database_url,
            postgres,
            dir,
            child: None,
        };
        gateway.spawn().await;
        Some(gateway)
    }

    async fn spawn(&mut self) {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.path().join("server.log"))
            .unwrap();
        let child = std::process::Command::new(bin())
            .arg("start")
            .env("TRUNKS_PORT", self.port.to_string())
            .env("TRUNKS_BIND_HOST", "127.0.0.1")
            .env("TRUNKS_MODE", self.mode.flag())
            .env("TRUNKS_DATABASE_URL", &self.database_url)
            .env("TRUNKS_LOG", "info")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        self.child = Some(child);
        for _ in 0..200 {
            if self.http.get(format!("{}/health", self.base)).send().await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the server did not start:\n{}", self.server_log());
    }

    /// Everything the server has logged so far (for failure messages).
    pub fn server_log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("server.log")).unwrap_or_default()
    }

    fn stop(&mut self) {
        let Some(mut child) = self.child.take() else { return };
        // Ask nicely where that is possible, so the graceful path is the one exercised.
        #[cfg(unix)]
        let _ =
            std::process::Command::new("kill").args(["-TERM", &child.id().to_string()]).status();
        #[cfg(not(unix))]
        let _ = child.kill();
        let _ = child.wait();
    }

    /// Stops the server and starts it again on the same database and port.
    pub async fn restart(&mut self) {
        self.stop();
        self.spawn().await;
    }

    /// Stops the server and removes its database.
    pub async fn finish(mut self) {
        self.stop();
        if let Some((admin_url, name)) = self.postgres.take() {
            let admin = Db::connect(&admin_url, 1).await.unwrap();
            with_pool!(&admin, |p| sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DROP DATABASE {name} WITH (FORCE)"
            )))
            .execute(p)
            .await
            .map(|_| ())
            .unwrap());
        }
    }

    /// A file inside the test's private directory.
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Runs the management CLI against this server, with its own config file.
    pub async fn cli(&self, args: &[&str]) -> CliOutput {
        let output = tokio::process::Command::new(bin())
            .args(args)
            .env("TRUNKS_REGISTRY", &self.base)
            .env("TRUNKS_CONFIG", self.path("cli-config.toml"))
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .output()
            .await
            .unwrap();
        CliOutput {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// `method path` on the REST API with an optional bearer token and JSON body.
    pub async fn rest(
        &self,
        method: reqwest::Method,
        path: &str,
        token: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> (u16, serde_json::Value) {
        let mut req = self.http.request(method, format!("{}{path}", self.base));
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        let res = req.send().await.unwrap();
        let status = res.status().as_u16();
        let text = res.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)))
    }

    /// An initialized MCP client on `path` (`/mcp` or a group endpoint). `Err` carries the
    /// reason the handshake was refused.
    pub async fn mcp(&self, path: &str, token: Option<&str>) -> Result<McpClient, String> {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(token) = token {
            headers
                .insert(reqwest::header::AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
        }
        let client = reqwest::Client::builder().default_headers(headers).build().unwrap();
        let transport = StreamableHttpClientTransport::with_client(
            client,
            StreamableHttpClientTransportConfig::with_uri(format!("{}{path}", self.base)),
        );
        ().serve(transport).await.map_err(|e| e.to_string())
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        // A failed assertion must not leave the server behind.
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The names of the tools an MCP client sees, sorted.
pub async fn tool_names(client: &McpClient) -> Vec<String> {
    let mut names: Vec<String> =
        client.list_all_tools().await.unwrap().into_iter().map(|t| t.name.to_string()).collect();
    names.sort();
    names
}

/// The text content of a tool result.
pub fn text_of(result: &rmcp::model::CallToolResult) -> String {
    result.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect()
}
