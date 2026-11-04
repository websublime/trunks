//! Spike S3 — upstream OAuth with `rmcp::transport::auth` against a fake authorization server.
//!
//! Proves the two-phase flow the registry needs:
//!   bootstrap (401 → discovery → DCR → authorization URL, state persisted)
//!   … process restart …
//!   complete (code + state → token exchange → credentials persisted)
//!   use (AuthClient-wrapped transport talks MCP with the bearer; refresh when expired)
//! plus the pre-registered-client path (no DCR) and error cases (bad state, refresh rejected).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rmcp::transport::auth::{
    AuthClient, AuthError, AuthorizationManager, CredentialStore, OAuthClientConfig, StateStore,
    StoredAuthorizationState, StoredCredentials,
};
use oauth2::TokenResponse as _;
use serde_json::{Value, json};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

// ----------------------------------------------------------------------------- DB-shaped stores
/// Mirrors `upstream_oauth_pending_sessions` (state + code_verifier per server).
#[derive(Default, Clone)]
struct PendingStore(Arc<Mutex<HashMap<String, StoredAuthorizationState>>>);
#[async_trait::async_trait]
impl StateStore for PendingStore {
    async fn save(&self, csrf: &str, st: StoredAuthorizationState) -> Result<(), AuthError> {
        self.0.lock().unwrap().insert(csrf.to_owned(), st);
        Ok(())
    }
    async fn load(&self, csrf: &str) -> Result<Option<StoredAuthorizationState>, AuthError> {
        Ok(self.0.lock().unwrap().get(csrf).cloned())
    }
    async fn delete(&self, csrf: &str) -> Result<(), AuthError> {
        self.0.lock().unwrap().remove(csrf);
        Ok(())
    }
}

/// Mirrors `upstream_oauth_tokens` (one credential set per upstream server).
#[derive(Default, Clone)]
struct TokenStore(Arc<Mutex<Option<StoredCredentials>>>);
#[async_trait::async_trait]
impl CredentialStore for TokenStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        Ok(self.0.lock().unwrap().clone())
    }
    async fn save(&self, c: StoredCredentials) -> Result<(), AuthError> {
        *self.0.lock().unwrap() = Some(c);
        Ok(())
    }
    async fn clear(&self) -> Result<(), AuthError> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

// ----------------------------------------------------------------------------- fake AS + upstream
struct TokenEndpoint {
    calls: Arc<Mutex<Vec<String>>>, // grant_type per call
    expires_in: u64,
}
impl Respond for TokenEndpoint {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let body = String::from_utf8_lossy(&req.body).to_string();
        let form: HashMap<String, String> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
        let grant = form.get("grant_type").cloned().unwrap_or_default();
        self.calls.lock().unwrap().push(grant.clone());
        match grant.as_str() {
            "authorization_code" => {
                if form.get("code").map(String::as_str) != Some("good-code") || !form.contains_key("code_verifier") {
                    return ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"}));
                }
                ResponseTemplate::new(200).set_body_json(json!({
                    "access_token": "at-1", "token_type": "Bearer", "expires_in": self.expires_in,
                    "refresh_token": "rt-1", "scope": "mcp:use"
                }))
            }
            "refresh_token" => {
                if form.get("refresh_token").map(String::as_str) == Some("rt-revoked") {
                    return ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"}));
                }
                ResponseTemplate::new(200).set_body_json(json!({
                    "access_token": "at-2", "token_type": "Bearer", "expires_in": 3600,
                    "refresh_token": "rt-2", "scope": "mcp:use"
                }))
            }
            _ => ResponseTemplate::new(400).set_body_json(json!({"error": "unsupported_grant_type"})),
        }
    }
}

/// Minimal MCP upstream: 401 + WWW-Authenticate without a valid bearer; JSON-RPC echo otherwise.
struct McpUpstream {
    resource_metadata_url: String,
}
impl Respond for McpUpstream {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let auth = req.headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
        if !(auth == "Bearer at-1" || auth == "Bearer at-2") {
            return ResponseTemplate::new(401).insert_header(
                "www-authenticate",
                format!("Bearer resource_metadata=\"{}\", scope=\"mcp:use\"", self.resource_metadata_url).as_str(),
            );
        }
        let v: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        let id = v["id"].clone();
        match v["method"].as_str().unwrap_or("") {
            "initialize" => ResponseTemplate::new(200).set_body_json(json!({"jsonrpc":"2.0","id":id,"result":{
                "protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fake-upstream","version":"0"}}})),
            "tools/list" => ResponseTemplate::new(200).set_body_json(json!({"jsonrpc":"2.0","id":id,"result":{
                "tools":[{"name":"search","description":"protected tool","inputSchema":{"type":"object"}}]}})),
            _ => ResponseTemplate::new(202),
        }
    }
}

async fn fake_servers(expires_in: u64) -> (MockServer, MockServer, Arc<Mutex<Vec<String>>>) {
    let as_srv = MockServer::start().await;
    let up = MockServer::start().await;
    let token_calls = Arc::new(Mutex::new(Vec::new()));

    // authorization server metadata (RFC 8414)
    Mock::given(method("GET")).and(path("/.well-known/oauth-authorization-server")).respond_with(
        ResponseTemplate::new(200).set_body_json(json!({
            "issuer": as_srv.uri(),
            "authorization_endpoint": format!("{}/authorize", as_srv.uri()),
            "token_endpoint": format!("{}/token", as_srv.uri()),
            "registration_endpoint": format!("{}/register", as_srv.uri()),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "scopes_supported": ["mcp:use"],
            "token_endpoint_auth_methods_supported": ["none", "client_secret_basic"]
        })),
    ).mount(&as_srv).await;
    // dynamic client registration (RFC 7591)
    Mock::given(method("POST")).and(path("/register")).respond_with(
        ResponseTemplate::new(201).set_body_json(json!({
            "client_id": "dcr-client-123", "client_name": "trunks-fake", "redirect_uris": ["http://127.0.0.1:1/oauth/callback"],
            "token_endpoint_auth_method": "none", "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"]
        })),
    ).mount(&as_srv).await;
    Mock::given(method("POST")).and(path("/token"))
        .respond_with(TokenEndpoint { calls: token_calls.clone(), expires_in })
        .mount(&as_srv).await;

    // protected resource metadata (RFC 9728) + the MCP endpoint
    let prm = format!("{}/.well-known/oauth-protected-resource/mcp", up.uri());
    Mock::given(method("GET")).and(path("/.well-known/oauth-protected-resource/mcp")).respond_with(
        ResponseTemplate::new(200).set_body_json(json!({
            "resource": format!("{}/mcp", up.uri()),
            "authorization_servers": [as_srv.uri()],
            "scopes_supported": ["mcp:use"]
        })),
    ).mount(&up).await;
    Mock::given(method("POST")).and(path("/mcp")).respond_with(McpUpstream { resource_metadata_url: prm }).mount(&up).await;
    Mock::given(method("GET")).and(path("/mcp")).respond_with(ResponseTemplate::new(405)).mount(&up).await;
    Mock::given(method("DELETE")).and(path("/mcp")).respond_with(ResponseTemplate::new(200)).mount(&up).await;

    (as_srv, up, token_calls)
}

macro_rules! check {
    ($name:expr, $cond:expr) => {{
        let ok = $cond;
        println!("  [{}] {}", if ok { "ok" } else { "FAIL" }, $name);
        ok
    }};
}

async fn manager(upstream_mcp: &str, pending: &PendingStore, tokens: &TokenStore) -> AuthorizationManager {
    let mut m = AuthorizationManager::new_allowing_private_addresses(upstream_mcp).await.expect("manager");
    m.set_state_store(pending.clone());
    m.set_credential_store(tokens.clone());
    m
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into())).init();
    let mut all = true;

    // ================================================================ flow A: DCR, restart, refresh
    println!("== A. unauthenticated connect → 401 challenge");
    let (as_srv, up, token_calls) = fake_servers(1).await; // 1 s tokens → forces refresh
    let mcp_url = format!("{}/mcp", up.uri());
    {
        use rmcp::ServiceExt as _;
        use rmcp::transport::StreamableHttpClientTransport;
        let t = StreamableHttpClientTransport::from_uri(mcp_url.as_str());
        let r = ().serve(t).await;
        let msg = r.err().map(|e| e.to_string()).unwrap_or_default();
        all &= check!("plain connect fails with an auth-required error carrying WWW-Authenticate", msg.contains("resource_metadata") || msg.to_lowercase().contains("auth"));
        println!("     error: {}", msg.chars().take(140).collect::<String>());
    }

    println!("== A. bootstrap (phase 1)");
    let pending = PendingStore::default();
    let tokens = TokenStore::default();
    let auth_url = {
        let mut m = manager(&mcp_url, &pending, &tokens).await;
        let res = m.resolve_metadata_from_challenge(Some(&format!("Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\", scope=\"mcp:use\"", up.uri()))).await?;
        all &= check!("discovery via resource metadata → AS metadata", res.metadata.token_endpoint.ends_with("/token"));
        all &= check!("AS advertises DCR", res.metadata.registration_endpoint.is_some());
        m.set_metadata(res.metadata);
        let cfg = m.register_client("trunks-fake", "http://127.0.0.1:1/oauth/callback", &["mcp:use"]).await?;
        all &= check!("dynamic client registration returns client_id", cfg.client_id == "dcr-client-123");
        let url = m.get_authorization_url(&["mcp:use"]).await?;
        let u = url::Url::parse(&url)?;
        let q: HashMap<_, _> = u.query_pairs().into_owned().collect();
        all &= check!("authorization URL has code_challenge S256", q.get("code_challenge_method").map(String::as_str) == Some("S256"));
        all &= check!("authorization URL has state", q.contains_key("state"));
        all &= check!("authorization URL has resource indicator (RFC 8707)", q.get("resource").is_some_and(|r| r.ends_with("/mcp")));
        all &= check!("state + verifier persisted in the pending store", pending.0.lock().unwrap().len() == 1);
        println!("     client_id={} state={}", cfg.client_id, &q["state"]);
        (url, cfg.client_id.clone())
    };
    let state = url::Url::parse(&auth_url.0)?.query_pairs().find(|(k, _)| k == "state").map(|(_, v)| v.into_owned()).unwrap();

    println!("== A. complete (phase 2) after a simulated restart — new manager, same stores");
    {
        let mut m = manager(&mcp_url, &pending, &tokens).await;
        let res = m.resolve_metadata().await?;
        m.set_metadata(res.metadata);
        m.configure_client(OAuthClientConfig::new(&auth_url.1, "http://127.0.0.1:1/oauth/callback"))?;
        let bad = m.exchange_code_for_token("good-code", "wrong-state").await;
        all &= check!("wrong state is rejected before any token request", bad.is_err() && token_calls.lock().unwrap().is_empty());
        let tok = m.exchange_code_for_token("good-code", &state).await?;
        all &= check!("code exchange returns access token", tok.access_token().secret() == "at-1");
        let saved = tokens.0.lock().unwrap().clone();
        all &= check!("credentials persisted (access + refresh + client_id)", saved.as_ref().is_some_and(|c| c.client_id == "dcr-client-123" && c.token_response.as_ref().and_then(|t| t.refresh_token()).is_some()));
        all &= check!("pending state consumed", pending.0.lock().unwrap().is_empty());
    }

    println!("== A. use: AuthClient-wrapped transport reaches the protected upstream");
    {
        use rmcp::ServiceExt as _;
        use rmcp::transport::StreamableHttpClientTransport;
        use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
        let mut m = manager(&mcp_url, &pending, &tokens).await;
        let restored = m.initialize_from_store().await?;
        all &= check!("initialize_from_store restores credentials", restored);
        let auth_client = AuthClient::new(reqwest::Client::new(), m);
        let cfg = StreamableHttpClientTransportConfig::with_uri(mcp_url.as_str());
        let t = StreamableHttpClientTransport::with_client(auth_client, cfg);
        let client = ().serve(t).await?;
        let tools = client.list_all_tools().await?;
        all &= check!("initialize + tools/list succeed with the stored bearer", tools.len() == 1 && tools[0].name == "search");
        client.cancel().await?;
    }

    println!("== A. refresh: token issued with expires_in=1 must be refreshed transparently");
    {
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        let mut m = manager(&mcp_url, &pending, &tokens).await;
        m.initialize_from_store().await?;
        let at = m.get_access_token().await?;
        let refreshes = token_calls.lock().unwrap().iter().filter(|g| g == &"refresh_token").count();
        // rmcp refreshes when < 30 s remain (REFRESH_BUFFER_SECS), so a 1 s token is refreshed on first use.
        all &= check!("a refresh_token grant was issued transparently (on first use or now)", refreshes >= 1);
        all &= check!("new access token in use", at == "at-2");
        let saved = tokens.0.lock().unwrap().clone();
        all &= check!("refreshed credentials persisted (rt-2)", saved.and_then(|c| c.token_response).and_then(|t| t.refresh_token().map(|r| r.secret().clone())).as_deref() == Some("rt-2"));
    }

    println!("== A. refresh rejected (revoked refresh token) → AuthorizationRequired-class error");
    {
        // forge expired credentials with a revoked refresh token
        let mut creds = tokens.0.lock().unwrap().clone().unwrap();
        let tr: Value = json!({"access_token":"at-old","token_type":"bearer","expires_in":1,"refresh_token":"rt-revoked"});
        creds.token_response = Some(serde_json::from_value(tr)?);
        creds.token_received_at = Some(0);
        tokens.save(creds).await?;
        let mut m = manager(&mcp_url, &pending, &tokens).await;
        m.initialize_from_store().await?;
        let err = m.get_access_token().await.err().map(|e| e.to_string()).unwrap_or_default();
        all &= check!("revoked refresh token surfaces an error the registry can map to 're-authorize'", !err.is_empty());
        println!("     error: {err}");
    }
    drop(as_srv);

    // ================================================================ flow B: pre-registered client (no DCR)
    println!("== B. pre-registered client_id/secret — DCR must not be called");
    {
        let (as_srv, up, _) = fake_servers(3600).await;
        let mcp_url = format!("{}/mcp", up.uri());
        let pending = PendingStore::default();
        let tokens = TokenStore::default();
        let mut m = manager(&mcp_url, &pending, &tokens).await;
        let res = m.resolve_metadata().await?;
        m.set_metadata(res.metadata);
        m.configure_client(OAuthClientConfig::new("pre-registered-id", "http://127.0.0.1:1/oauth/callback").with_client_secret("s3cr3t"))?;
        let url = m.get_authorization_url(&["mcp:use"]).await?;
        all &= check!("authorization URL uses the pre-registered client_id", url.contains("client_id=pre-registered-id"));
        let reg_calls = as_srv.received_requests().await.unwrap_or_default().iter().filter(|r| r.url.path() == "/register").count();
        all &= check!("no call to the registration endpoint", reg_calls == 0);
        let state = url::Url::parse(&url)?.query_pairs().find(|(k, _)| k == "state").map(|(_, v)| v.into_owned()).unwrap();
        let tok = m.exchange_code_for_token("good-code", &state).await?;
        all &= check!("exchange works with client_secret", tok.access_token().secret() == "at-1");
    }

    // ================================================================ flow C: AS without DCR and no client_id
    println!("== C. AS without registration_endpoint and no client_id → actionable error");
    {
        let as_srv = MockServer::start().await;
        Mock::given(method("GET")).and(path("/.well-known/oauth-authorization-server")).respond_with(
            ResponseTemplate::new(200).set_body_json(json!({
                "issuer": as_srv.uri(), "authorization_endpoint": format!("{}/authorize", as_srv.uri()),
                "token_endpoint": format!("{}/token", as_srv.uri()), "response_types_supported": ["code"],
                "code_challenge_methods_supported": ["S256"]})),
        ).mount(&as_srv).await;
        let mut m = AuthorizationManager::new_allowing_private_addresses(as_srv.uri()).await?;
        let res = m.resolve_metadata().await?;
        m.set_metadata(res.metadata);
        let err = m.register_client("x", "http://127.0.0.1:1/cb", &[]).await.err().map(|e| e.to_string()).unwrap_or_default();
        all &= check!("register_client reports DCR unsupported", err.contains("Dynamic client registration not supported"));
        let _ = (header("x", "y"), body_string_contains("x")); // keep matchers import used in all cfgs
    }

    println!();
    println!("{}", if all { "S3: ALL CHECKS PASSED" } else { "S3: FAILURES" });
    std::process::exit(if all { 0 } else { 1 });
}
