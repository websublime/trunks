//! A fake OAuth authorization server (RFC 8414 metadata, RFC 7591 DCR, token endpoint) and a
//! protected MCP upstream (401 + `WWW-Authenticate`, RFC 9728 metadata), on wiremock.
//! Accepts `code = "good-code"`; issues `at-1` then `at-2` on refresh. `GET /authorize` approves
//! at once and redirects back with that code, like a provider where the person clicked "Allow".

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

#[derive(Clone, Default, Debug)]
pub struct Calls(pub Arc<Mutex<Vec<String>>>);

impl Calls {
    pub fn snapshot(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

#[derive(Debug)]
pub struct TokenEndpoint {
    pub calls: Calls,
    pub expires_in: u64,
}
impl Respond for TokenEndpoint {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let form: HashMap<String, String> =
            url::form_urlencoded::parse(&req.body).into_owned().collect();
        let grant = form.get("grant_type").cloned().unwrap_or_default();
        self.calls.0.lock().unwrap().push(grant.clone());
        match grant.as_str() {
            "authorization_code" => {
                if form.get("code").map(String::as_str) != Some("good-code")
                    || !form.contains_key("code_verifier")
                {
                    return ResponseTemplate::new(400)
                        .set_body_json(json!({"error": "invalid_grant"}));
                }
                ResponseTemplate::new(200).set_body_json(json!({
                    "access_token": "at-1", "token_type": "Bearer", "expires_in": self.expires_in,
                    "refresh_token": "rt-1", "scope": "mcp:use"
                }))
            }
            "refresh_token" => ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "at-2", "token_type": "Bearer", "expires_in": 3600,
                "refresh_token": "rt-2", "scope": "mcp:use"
            })),
            _ => {
                ResponseTemplate::new(400).set_body_json(json!({"error": "unsupported_grant_type"}))
            }
        }
    }
}

/// The authorization endpoint of a provider where the person always says yes: sends the
/// browser straight back to `redirect_uri` with the code the token endpoint accepts.
#[derive(Debug)]
pub struct AuthorizeEndpoint;
impl Respond for AuthorizeEndpoint {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let query: HashMap<String, String> = req.url.query_pairs().into_owned().collect();
        let (Some(redirect), Some(state)) = (query.get("redirect_uri"), query.get("state")) else {
            return ResponseTemplate::new(400)
                .set_body_string("redirect_uri and state are required");
        };
        let Ok(mut back) = url::Url::parse(redirect) else {
            return ResponseTemplate::new(400).set_body_string("redirect_uri is not a URL");
        };
        back.query_pairs_mut().append_pair("code", "good-code").append_pair("state", state);
        ResponseTemplate::new(302).insert_header("location", back.as_str())
    }
}

#[derive(Debug)]
pub struct McpUpstream {
    pub resource_metadata_url: String,
    pub seen_bearers: Calls,
}
impl Respond for McpUpstream {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let auth = req.headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
        self.seen_bearers.0.lock().unwrap().push(auth.to_owned());
        if !(auth == "Bearer at-1" || auth == "Bearer at-2") {
            return ResponseTemplate::new(401).insert_header(
                "www-authenticate",
                format!(
                    "Bearer resource_metadata=\"{}\", scope=\"mcp:use\"",
                    self.resource_metadata_url
                )
                .as_str(),
            );
        }
        let v: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        let id = v["id"].clone();
        match v["method"].as_str().unwrap_or("") {
            "initialize" => ResponseTemplate::new(200).set_body_json(json!({"jsonrpc":"2.0","id":id,"result":{
                "protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"protected","version":"0"}}})),
            "tools/list" => ResponseTemplate::new(200).set_body_json(json!({"jsonrpc":"2.0","id":id,"result":{
                "tools":[{"name":"search","description":"protected tool","inputSchema":{"type":"object"}}]}})),
            "tools/call" => ResponseTemplate::new(200).set_body_json(json!({"jsonrpc":"2.0","id":id,"result":{
                "content":[{"type":"text","text":format!("secret result for {}", v["params"]["arguments"]["q"].as_str().unwrap_or(""))}]}})),
            _ => ResponseTemplate::new(202),
        }
    }
}

#[derive(Debug)]
pub struct Fake {
    pub as_srv: MockServer,
    pub up: MockServer,
    pub token_calls: Calls,
    pub bearers: Calls,
}

impl Fake {
    pub async fn start(dcr: bool, expires_in: u64) -> Self {
        let as_srv = MockServer::start().await;
        let up = MockServer::start().await;
        let token_calls = Calls::default();
        let bearers = Calls::default();
        let mut meta = json!({
            "issuer": as_srv.uri(),
            "authorization_endpoint": format!("{}/authorize", as_srv.uri()),
            "token_endpoint": format!("{}/token", as_srv.uri()),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "scopes_supported": ["mcp:use"],
            "token_endpoint_auth_methods_supported": ["none", "client_secret_basic"]
        });
        if dcr {
            meta["registration_endpoint"] = Value::String(format!("{}/register", as_srv.uri()));
        }
        Mock::given(method("GET"))
            .and(path("/.well-known/oauth-authorization-server"))
            .respond_with(ResponseTemplate::new(200).set_body_json(meta))
            .mount(&as_srv)
            .await;
        Mock::given(method("POST")).and(path("/register")).respond_with(
            ResponseTemplate::new(201).set_body_json(json!({
                "client_id": "dcr-client-123", "client_name": "trunks-prot", "redirect_uris": ["http://127.0.0.1:1/oauth/callback"],
                "token_endpoint_auth_method": "none", "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"]
            })),
        ).mount(&as_srv).await;
        Mock::given(method("GET"))
            .and(path("/authorize"))
            .respond_with(AuthorizeEndpoint)
            .mount(&as_srv)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(TokenEndpoint { calls: token_calls.clone(), expires_in })
            .mount(&as_srv)
            .await;
        let prm = format!("{}/.well-known/oauth-protected-resource/mcp", up.uri());
        Mock::given(method("GET"))
            .and(path("/.well-known/oauth-protected-resource/mcp"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "resource": format!("{}/mcp", up.uri()),
                "authorization_servers": [as_srv.uri()],
                "scopes_supported": ["mcp:use"]
            })))
            .mount(&up)
            .await;
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .respond_with(McpUpstream { resource_metadata_url: prm, seen_bearers: bearers.clone() })
            .mount(&up)
            .await;
        Mock::given(method("GET"))
            .and(path("/mcp"))
            .respond_with(ResponseTemplate::new(405))
            .mount(&up)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/mcp"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&up)
            .await;
        Self { as_srv, up, token_calls, bearers }
    }

    pub fn mcp_url(&self) -> String {
        format!("{}/mcp", self.up.uri())
    }

    pub async fn register_calls(&self) -> usize {
        self.as_srv
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|r| r.url.path() == "/register")
            .count()
    }
}
