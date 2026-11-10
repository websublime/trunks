//! Connections to upstream MCP servers.
//!
//! An `UpstreamConn` is built **only** from the stored configuration — it never sees the
//! downstream request, so no downstream header can reach an upstream (structural, not a
//! convention). This is one of the two modules allowed to import `rmcp` directly.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use rmcp::ServiceExt as _;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, GetPromptRequestParams, GetPromptResult, JsonObject,
    Prompt, ReadResourceRequestParams, ReadResourceResult, Resource, ServerCapabilities, Tool,
};
use rmcp::service::{RoleClient, RunningService, ServiceError};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{ConfigureCommandExt as _, StreamableHttpClientTransport, TokioChildProcess};
use secrecy::ExposeSecret as _;
use tokio::io::AsyncReadExt as _;
use trunks_types::ServerName;

use crate::domain::UpstreamConfig;
use crate::ports::OAuthStore;
use crate::{Error, Result};

/// Connection settings shared by every upstream.
#[derive(Clone)]
pub struct ConnectOptions {
    /// Timeout for the MCP `initialize` handshake.
    pub init_timeout: Duration,
    /// Upper bound for one request to an upstream after the handshake (tool call, prompt,
    /// resource read, listing). `None` = wait for as long as the upstream takes.
    pub call_timeout: Option<Duration>,
    /// Where stored OAuth tokens live; `None` disables OAuth-backed connections.
    pub oauth: Option<Arc<dyn OAuthStore>>,
}

impl std::fmt::Debug for ConnectOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectOptions")
            .field("init_timeout", &self.init_timeout)
            .field("call_timeout", &self.call_timeout)
            .field("oauth", &self.oauth.is_some())
            .finish()
    }
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            init_timeout: Duration::from_secs(30),
            call_timeout: Some(Duration::from_mins(5)),
            oauth: None,
        }
    }
}

/// Finds an upstream 401 challenge anywhere in an rmcp error chain.
fn auth_challenge(e: &(dyn std::error::Error + 'static)) -> Option<String> {
    let mut cur = Some(e);
    while let Some(c) = cur {
        if let Some(a) =
            c.downcast_ref::<rmcp::transport::streamable_http_client::AuthRequiredError>()
        {
            return Some(a.www_authenticate_header.clone());
        }
        cur = c.source();
    }
    None
}

fn init_error(server: &ServerName, e: &rmcp::service::ClientInitializeError) -> Error {
    if let rmcp::service::ClientInitializeError::TransportError { error, .. } = e
        && let Some(www_authenticate) = auth_challenge(error.error.as_ref())
    {
        return Error::UpstreamAuthRequired { server: server.to_string(), www_authenticate };
    }
    upstream_err(server, format!("initialize failed: {e}"))
}

/// Longest stderr line of an upstream process that is logged; the rest of the line is dropped.
const STDERR_LINE_LIMIT: usize = 4096;
/// `env` values shorter than this are not treated as secrets when redacting stderr: blanking
/// every `1` or `on` would make the output useless without protecting anything.
const MIN_REDACTED_LEN: usize = 4;
const REDACTED: &str = "[redacted]";

/// What must never be repeated from an upstream's stderr: every configured `env` value, and
/// each line of a multi-line one (a PEM key is printed line by line). Longest first, so a
/// value is replaced before any shorter value it contains.
fn redaction_needles(env: &BTreeMap<String, secrecy::SecretString>) -> Vec<String> {
    let mut needles: Vec<String> = env
        .values()
        .flat_map(|v| {
            let value = v.expose_secret();
            std::iter::once(value).chain(value.lines()).map(str::trim).map(str::to_owned)
        })
        .filter(|n| n.len() >= MIN_REDACTED_LEN)
        .collect();
    needles.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    needles.dedup();
    needles
}

/// The first `limit` bytes of `line` with every needle replaced. A needle that starts before
/// the limit is replaced whole, so cutting the line never leaves part of a secret behind.
fn redact(line: &str, needles: &[String], limit: usize) -> String {
    let mut out = String::with_capacity(line.len().min(limit));
    let mut rest = line;
    while line.len() - rest.len() < limit {
        if let Some(needle) = needles.iter().find(|n| rest.starts_with(n.as_str())) {
            out.push_str(REDACTED);
            rest = &rest[needle.len()..];
        } else if let Some(c) = rest.chars().next() {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        } else {
            break;
        }
    }
    out
}

/// Logs one stderr line of `server` (target `upstream.stderr`), redacted, bounded and with
/// control characters escaped so that an upstream cannot forge log lines or terminal output.
fn log_stderr_line(server: &ServerName, bytes: &[u8], needles: &[String]) {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_end();
    if text.is_empty() {
        return;
    }
    let line = redact(text, needles, STDERR_LINE_LIMIT);
    tracing::info!(target: "upstream.stderr", server = %server, "{}", line.escape_debug());
}

/// Forwards the stderr of an upstream process to the log, line by line, until it closes.
async fn forward_stderr(
    server: ServerName,
    mut stderr: tokio::process::ChildStderr,
    needles: Vec<String>,
) {
    // Enough to hold a secret that starts right before the limit.
    let capacity = STDERR_LINE_LIMIT + needles.first().map_or(0, String::len);
    let mut line: Vec<u8> = Vec::new();
    let mut dropping = false; // inside the tail of an over-long line
    let mut chunk = [0u8; 1024];
    loop {
        let read = match stderr.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for &byte in &chunk[..read] {
            if byte == b'\n' {
                if !dropping {
                    log_stderr_line(&server, &line, &needles);
                }
                line.clear();
                dropping = false;
            } else if !dropping {
                line.push(byte);
                if line.len() >= capacity {
                    log_stderr_line(&server, &line, &needles);
                    line.clear();
                    dropping = true;
                }
            }
        }
    }
    if !dropping {
        log_stderr_line(&server, &line, &needles);
    }
}

/// A live, initialized MCP client session with one upstream.
pub struct UpstreamConn {
    server: ServerName,
    peer: RunningService<RoleClient, ()>,
    call_timeout: Option<Duration>,
}

impl std::fmt::Debug for UpstreamConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpstreamConn").field("server", &self.server).finish_non_exhaustive()
    }
}

fn upstream_err(server: &ServerName, reason: impl Into<String>) -> Error {
    Error::Upstream { server: server.to_string(), reason: reason.into() }
}

/// The domain error for a failed request to `server`.
pub(crate) fn call_error(server: &ServerName, e: &ServiceError) -> Error {
    match e {
        ServiceError::Timeout { timeout } => {
            Error::UpstreamTimeout { server: server.to_string(), seconds: timeout.as_secs() }
        }
        other => upstream_err(server, other.to_string()),
    }
}

/// Whether an rmcp error means the connection itself is unusable (vs. a normal protocol error).
#[must_use]
pub fn is_connection_error(e: &ServiceError) -> bool {
    matches!(
        e,
        ServiceError::TransportSend(_)
            | ServiceError::TransportClosed
            | ServiceError::Timeout { .. }
            | ServiceError::Cancelled { .. }
    )
}

impl UpstreamConn {
    /// Connects and runs `initialize`, bounded by `opts.init_timeout`.
    pub async fn connect(
        server: &ServerName,
        config: &UpstreamConfig,
        opts: &ConnectOptions,
    ) -> Result<Self> {
        let server = server.clone();
        let connect = async {
            match config {
                UpstreamConfig::Stdio { command, args, env } => {
                    Self::connect_stdio(&server, command, args, env).await
                }
                UpstreamConfig::StreamableHttp { url, bearer_token, headers } => {
                    Self::connect_http(
                        &server,
                        url,
                        bearer_token.as_ref(),
                        headers,
                        opts.oauth.as_ref(),
                    )
                    .await
                }
                UpstreamConfig::Sse { .. } => Err(upstream_err(
                    &server,
                    "the 'sse' transport is not supported; re-register the server as streamable_http",
                )),
            }
        };
        match tokio::time::timeout(opts.init_timeout, connect).await {
            Ok(r) => r.map(|mut conn| {
                conn.call_timeout = opts.call_timeout;
                conn
            }),
            Err(_) => Err(upstream_err(
                &server,
                format!(
                    "initialize timed out after {}s (set TRUNKS_UPSTREAM_INIT_TIMEOUT_SEC to allow slower servers)",
                    opts.init_timeout.as_secs()
                ),
            )),
        }
    }

    async fn connect_stdio(
        server: &ServerName,
        command: &str,
        args: &[String],
        env: &BTreeMap<String, secrecy::SecretString>,
    ) -> Result<Self> {
        let cmd = tokio::process::Command::new(command).configure(|c| {
            c.args(args);
            for (k, v) in env {
                c.env(k, v.expose_secret());
            }
            c.kill_on_drop(true);
        });
        // stderr is piped and logged by us. Left to rmcp's default it would be inherited:
        // the upstream would write straight into this process's own stderr, unredacted.
        let (transport, stderr) = TokioChildProcess::builder(cmd)
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| upstream_err(server, format!("failed to spawn '{command}': {e}")))?;
        if let Some(stderr) = stderr {
            tokio::spawn(forward_stderr(server.clone(), stderr, redaction_needles(env)));
        }
        let peer =
            ().serve(transport)
                .await
                .map_err(|e| upstream_err(server, format!("initialize failed: {e}")))?;
        Ok(Self { server: server.clone(), peer, call_timeout: None })
    }

    async fn connect_http(
        server: &ServerName,
        url: &str,
        bearer: Option<&secrecy::SecretString>,
        headers: &BTreeMap<String, secrecy::SecretString>,
        oauth: Option<&Arc<dyn OAuthStore>>,
    ) -> Result<Self> {
        let mut hm = reqwest::header::HeaderMap::new();
        for (k, v) in headers {
            let name = reqwest::header::HeaderName::from_bytes(k.as_bytes())
                .map_err(|_| upstream_err(server, format!("invalid header name '{k}'")))?;
            let value = reqwest::header::HeaderValue::from_str(v.expose_secret())
                .map_err(|_| upstream_err(server, format!("invalid value for header '{k}'")))?;
            hm.insert(name, value);
        }
        if let Some(t) = bearer {
            if hm.contains_key(reqwest::header::AUTHORIZATION) {
                tracing::warn!(server = %server, "custom Authorization header will be used; bearer_token ignored");
            } else {
                let value = reqwest::header::HeaderValue::from_str(&format!(
                    "Bearer {}",
                    t.expose_secret()
                ))
                .map_err(|_| upstream_err(server, "invalid bearer token"))?;
                hm.insert(reqwest::header::AUTHORIZATION, value);
            }
        }
        let client = reqwest::Client::builder()
            .default_headers(hm)
            .build()
            .map_err(|e| upstream_err(server, e.to_string()))?;
        let cfg = StreamableHttpClientTransportConfig::with_uri(url.to_owned());
        // Stored OAuth tokens win over a static bearer: the wrapped client injects a fresh
        // bearer on every request and refreshes it when it is about to expire.
        let auth = match oauth {
            Some(store) => crate::oauth::auth_client(store, server, url, client.clone()).await?,
            None => None,
        };
        let peer = if let Some(auth_client) = auth {
            let transport = StreamableHttpClientTransport::with_client(auth_client, cfg);
            ().serve(transport).await.map_err(|e| init_error(server, &e))?
        } else {
            let transport = StreamableHttpClientTransport::with_client(client, cfg);
            ().serve(transport).await.map_err(|e| init_error(server, &e))?
        };
        Ok(Self { server: server.clone(), peer, call_timeout: None })
    }

    #[must_use]
    pub fn server(&self) -> &ServerName {
        &self.server
    }

    #[must_use]
    pub fn capabilities(&self) -> Option<Arc<ServerCapabilities>> {
        self.peer.peer_info().map(|i| Arc::new(i.capabilities.clone()))
    }

    /// Runs one upstream request under `call_timeout`. On expiry the request is abandoned
    /// and reported as [`ServiceError::Timeout`], which the session layer treats as a broken
    /// connection (a stateful session is reopened on the next call).
    async fn bounded<T>(
        &self,
        request: impl Future<Output = std::result::Result<T, ServiceError>>,
    ) -> std::result::Result<T, ServiceError> {
        match self.call_timeout {
            Some(limit) => tokio::time::timeout(limit, request)
                .await
                .unwrap_or(Err(ServiceError::Timeout { timeout: limit })),
            None => request.await,
        }
    }

    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        self.bounded(self.peer.list_all_tools()).await.map_err(|e| self.map(&e))
    }

    pub async fn list_prompts(&self) -> Result<Vec<Prompt>> {
        self.bounded(self.peer.list_all_prompts()).await.map_err(|e| self.map(&e))
    }

    pub async fn list_resources(&self) -> Result<Vec<Resource>> {
        self.bounded(self.peer.list_all_resources()).await.map_err(|e| self.map(&e))
    }

    /// Calls `name` (the upstream's own tool name). Returns the raw error too so the
    /// session layer can decide whether the connection is still healthy.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Option<JsonObject>,
    ) -> std::result::Result<CallToolResult, ServiceError> {
        let mut params = CallToolRequestParams::new(name.to_owned());
        params.arguments = arguments;
        self.bounded(self.peer.call_tool(params)).await
    }

    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<JsonObject>,
    ) -> std::result::Result<GetPromptResult, ServiceError> {
        let mut params = GetPromptRequestParams::new(name.to_owned());
        params.arguments = arguments;
        self.bounded(self.peer.get_prompt(params)).await
    }

    pub async fn read_resource(
        &self,
        uri: &str,
    ) -> std::result::Result<ReadResourceResult, ServiceError> {
        let params = ReadResourceRequestParams::new(uri.to_owned());
        self.bounded(self.peer.read_resource(params)).await
    }

    /// Closes the session (and kills a stdio child process).
    pub async fn close(self) {
        let _ = self.peer.cancel().await;
    }

    fn map(&self, e: &ServiceError) -> Error {
        call_error(&self.server, e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, secrecy::SecretString> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), secrecy::SecretString::from(*v))).collect()
    }

    #[test]
    fn stderr_redaction() {
        let needles = redaction_needles(&env(&[
            ("TOKEN", "tok-12345"),
            ("LONGER", "tok-12345-and-more"),
            ("FLAG", "1"),
            ("MODE", "on"),
            ("PEM", "-----BEGIN KEY-----\nMIIBVQ==\n-----END KEY-----"),
        ]));
        assert!(needles.iter().all(|n| n.len() >= MIN_REDACTED_LEN), "{needles:?}");
        let cases = [
            ("nothing secret here", "nothing secret here"),
            ("token=tok-12345 done", "token=[redacted] done"),
            ("tok-12345tok-12345", "[redacted][redacted]"),
            // the longer value wins over the shorter one it contains
            ("x tok-12345-and-more y", "x [redacted] y"),
            // short values are left alone
            ("retry 1 of 3, mode on", "retry 1 of 3, mode on"),
            // a multi-line value is recognised line by line
            ("key line: MIIBVQ==", "key line: [redacted]"),
            ("-----BEGIN KEY-----", "[redacted]"),
            ("ünïcödé tok-12345 ✓", "ünïcödé [redacted] ✓"),
        ];
        for (line, expected) in cases {
            assert_eq!(redact(line, &needles, STDERR_LINE_LIMIT), expected, "{line}");
        }
    }

    #[test]
    fn cutting_a_line_never_leaves_part_of_a_secret() {
        let needles = redaction_needles(&env(&[("TOKEN", "tok-12345")]));
        // the limit falls in the middle of the secret: it is replaced whole
        assert_eq!(redact("abc tok-12345 tail", &needles, 7), "abc [redacted]");
        // the limit falls before the secret starts: nothing of it is emitted
        assert_eq!(redact("abc tok-12345 tail", &needles, 4), "abc ");
        // multi-byte characters are never split
        assert_eq!(redact("ééé", &needles, 3), "éé");
        assert_eq!(redact("", &needles, 10), "");
    }
}
