//! A fake OpenID Connect provider on wiremock: discovery, JWKS (one RSA key), a token endpoint
//! that signs RS256 ID tokens, and userinfo. Tests play the browser: they read `state` and
//! `nonce` from the authorization redirect, ask the fake for a `code` bound to an identity,
//! and call the application's callback.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const KEY_PEM: &str = include_str!("fixtures/test-idp-key.pem");
const KEY_N: &str = include_str!("fixtures/test-idp-key.n");
pub const CLIENT_ID: &str = "trunks-test-client";

/// What the provider will assert for one authorization code.
#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub sub: String,
    pub preferred_username: Option<String>,
    pub email: Option<String>,
    /// `None` = no groups claim in the ID token.
    pub groups: Option<Vec<String>>,
    /// Groups served by the userinfo endpoint instead (when the token carries none).
    pub userinfo_groups: Option<Vec<String>>,
    /// The `nonce` of the authorization request.
    pub nonce: String,
    /// Overrides the audience (default: the client id).
    pub aud: Option<String>,
    /// Corrupts the signature.
    pub tamper: bool,
    /// Issues an already expired token.
    pub expired: bool,
    /// Signs with HS256, using the provider's public key material as the shared secret (the
    /// classic algorithm-confusion attack).
    pub symmetric: bool,
}

#[derive(Debug, Default)]
struct State {
    codes: HashMap<String, Identity>,
    access: HashMap<String, Identity>,
    counter: u64,
}

#[derive(Debug)]
pub struct FakeIdp {
    pub server: MockServer,
    state: Arc<Mutex<State>>,
}

struct TokenEndpoint {
    issuer: String,
    state: Arc<Mutex<State>>,
}

fn sign(issuer: &str, id: &Identity) -> String {
    let now = chrono_now();
    let (iat, exp) = if id.expired { (now - 7200, now - 3600) } else { (now, now + 600) };
    let mut claims = json!({
        "iss": issuer,
        "sub": id.sub,
        "aud": id.aud.clone().unwrap_or_else(|| CLIENT_ID.to_owned()),
        "iat": iat,
        "exp": exp,
        "nonce": id.nonce,
    });
    if let Some(u) = &id.preferred_username {
        claims["preferred_username"] = Value::String(u.clone());
    }
    if let Some(e) = &id.email {
        claims["email"] = Value::String(e.clone());
    }
    if let Some(g) = &id.groups {
        claims["groups"] = json!(g);
    }
    let (alg, key) = if id.symmetric {
        (Algorithm::HS256, EncodingKey::from_secret(KEY_N.trim().as_bytes()))
    } else {
        (Algorithm::RS256, EncodingKey::from_rsa_pem(KEY_PEM.as_bytes()).unwrap())
    };
    let mut header = Header::new(alg);
    header.kid = Some("test".into());
    let mut jwt = jsonwebtoken::encode(&header, &claims, &key).unwrap();
    if id.tamper {
        // Change the first signature character: all of its six bits are signature data, so
        // the result is always well-formed base64url carrying a different signature. (The
        // last character also holds padding bits; changing it can yield a malformed token
        // instead of a wrong signature.)
        let at = jwt.rfind('.').unwrap() + 1;
        let replacement = if jwt[at..].starts_with('A') { "B" } else { "A" };
        jwt.replace_range(at..=at, replacement);
    }
    jwt
}

fn chrono_now() -> i64 {
    i64::try_from(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
    )
    .unwrap()
}

impl Respond for TokenEndpoint {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let form: HashMap<String, String> =
            url::form_urlencoded::parse(&req.body).into_owned().collect();
        if form.get("grant_type").map(String::as_str) != Some("authorization_code")
            || !form.contains_key("code_verifier")
        {
            return ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_request"}));
        }
        let mut st = self.state.lock().unwrap();
        // single use
        let Some(id) = form.get("code").and_then(|c| st.codes.remove(c)) else {
            return ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"}));
        };
        st.counter += 1;
        let access = format!("idp-access-{}", st.counter);
        st.access.insert(access.clone(), id.clone());
        ResponseTemplate::new(200).set_body_json(json!({
            "access_token": access,
            "token_type": "Bearer",
            "expires_in": 600,
            "id_token": sign(&self.issuer, &id),
        }))
    }
}

struct UserInfo {
    state: Arc<Mutex<State>>,
}

impl Respond for UserInfo {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let token = req
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        let st = self.state.lock().unwrap();
        let Some(id) = st.access.get(token) else {
            return ResponseTemplate::new(401);
        };
        let mut body = json!({"sub": id.sub});
        if let Some(g) = &id.userinfo_groups {
            body["groups"] = json!(g);
        }
        ResponseTemplate::new(200).set_body_json(body)
    }
}

impl FakeIdp {
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        let state = Arc::new(Mutex::new(State::default()));
        let issuer = server.uri();
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "issuer": issuer,
                "authorization_endpoint": format!("{issuer}/authorize"),
                "token_endpoint": format!("{issuer}/token"),
                "jwks_uri": format!("{issuer}/jwks"),
                "userinfo_endpoint": format!("{issuer}/userinfo"),
                "response_types_supported": ["code"],
                "subject_types_supported": ["public"],
                "id_token_signing_alg_values_supported": ["RS256"],
                "scopes_supported": ["openid", "profile", "email"],
                "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
                "code_challenge_methods_supported": ["S256"]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "keys": [{"kty": "RSA", "use": "sig", "alg": "RS256", "kid": "test", "n": KEY_N.trim(), "e": "AQAB"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(TokenEndpoint { issuer: issuer.clone(), state: state.clone() })
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .respond_with(UserInfo { state: state.clone() })
            .mount(&server)
            .await;
        Self { server, state }
    }

    /// Simulates a key rotation the application has not seen yet: the **next** JWKS request
    /// is answered with a set that lacks the signing key; later ones get the real set.
    pub async fn serve_stale_jwks_once(&self) {
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "keys": [
                    {"kty": "RSA", "use": "sig", "alg": "RS256", "kid": "retired", "n": KEY_N.trim(), "e": "AQAB"},
                    {"kty": "RSA", "use": "enc", "alg": "RSA-OAEP-512", "kid": "unusable", "n": "AQAB", "e": "AQAB"},
                    {"kty": "unknown-type", "kid": "exotic"}
                ]
            })))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&self.server)
            .await;
    }

    #[must_use]
    pub fn issuer(&self) -> String {
        self.server.uri()
    }

    /// The user "authenticated" at the provider: returns the authorization code the browser
    /// would be redirected back with.
    pub fn issue_code(&self, identity: Identity) -> String {
        let mut st = self.state.lock().unwrap();
        st.counter += 1;
        let code = format!("code-{}", st.counter);
        st.codes.insert(code.clone(), identity);
        code
    }

    /// How many times an endpoint was hit (e.g. `/jwks`).
    pub async fn hits(&self, endpoint: &str) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|r| r.url.path() == endpoint)
            .count()
    }
}
