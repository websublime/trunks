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
use trunks_types::ServerName;

use crate::domain::UpstreamConfig;
use crate::{Error, Result};

/// Connection settings shared by every upstream.
#[derive(Debug, Clone)]
pub struct ConnectOptions {
    /// Timeout for the MCP `initialize` handshake.
    pub init_timeout: Duration,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self { init_timeout: Duration::from_secs(30) }
    }
}

/// A live, initialized MCP client session with one upstream.
pub struct UpstreamConn {
    server: ServerName,
    peer: RunningService<RoleClient, ()>,
}

impl std::fmt::Debug for UpstreamConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpstreamConn").field("server", &self.server).finish_non_exhaustive()
    }
}

fn upstream_err(server: &ServerName, reason: impl Into<String>) -> Error {
    Error::Upstream { server: server.to_string(), reason: reason.into() }
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
                    Self::connect_http(&server, url, bearer_token.as_ref(), headers).await
                }
                UpstreamConfig::Sse { .. } => Err(upstream_err(
                    &server,
                    "the 'sse' transport is not supported; re-register the server as streamable_http",
                )),
            }
        };
        match tokio::time::timeout(opts.init_timeout, connect).await {
            Ok(r) => r,
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
            c.stderr(std::process::Stdio::null());
        });
        let transport = TokioChildProcess::new(cmd)
            .map_err(|e| upstream_err(server, format!("failed to spawn '{command}': {e}")))?;
        let peer =
            ().serve(transport)
                .await
                .map_err(|e| upstream_err(server, format!("initialize failed: {e}")))?;
        Ok(Self { server: server.clone(), peer })
    }

    async fn connect_http(
        server: &ServerName,
        url: &str,
        bearer: Option<&secrecy::SecretString>,
        headers: &BTreeMap<String, secrecy::SecretString>,
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
        let transport = StreamableHttpClientTransport::with_client(client, cfg);
        let peer =
            ().serve(transport)
                .await
                .map_err(|e| upstream_err(server, format!("initialize failed: {e}")))?;
        Ok(Self { server: server.clone(), peer })
    }

    #[must_use]
    pub fn server(&self) -> &ServerName {
        &self.server
    }

    #[must_use]
    pub fn capabilities(&self) -> Option<Arc<ServerCapabilities>> {
        self.peer.peer_info().map(|i| Arc::new(i.capabilities.clone()))
    }

    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        self.peer.list_all_tools().await.map_err(|e| self.map(&e))
    }

    pub async fn list_prompts(&self) -> Result<Vec<Prompt>> {
        self.peer.list_all_prompts().await.map_err(|e| self.map(&e))
    }

    pub async fn list_resources(&self) -> Result<Vec<Resource>> {
        self.peer.list_all_resources().await.map_err(|e| self.map(&e))
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
        self.peer.call_tool(params).await
    }

    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<JsonObject>,
    ) -> std::result::Result<GetPromptResult, ServiceError> {
        let mut params = GetPromptRequestParams::new(name.to_owned());
        params.arguments = arguments;
        self.peer.get_prompt(params).await
    }

    pub async fn read_resource(
        &self,
        uri: &str,
    ) -> std::result::Result<ReadResourceResult, ServiceError> {
        let params = ReadResourceRequestParams::new(uri.to_owned());
        self.peer.read_resource(params).await
    }

    /// Closes the session (and kills a stdio child process).
    pub async fn close(self) {
        let _ = self.peer.cancel().await;
    }

    fn map(&self, e: &ServiceError) -> Error {
        upstream_err(&self.server, e.to_string())
    }
}
