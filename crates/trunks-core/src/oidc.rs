//! OIDC login for humans (D16): Authorization Code + PKCE against an external identity
//! provider. This module only talks to the IdP and returns a verified identity; sessions and
//! provisioning live in [`crate::login`].
//!
//! The ID token is verified here with `jsonwebtoken`: signature against the provider's JWKS
//! (asymmetric algorithms only), `iss`, `aud`, `azp`, expiry and `nonce`. Extra claims
//! (`groups`) are read from the payload **after** that verification, or from the userinfo
//! endpoint when the token does not carry them.

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use rand::RngCore as _;
use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::sync::Mutex;

use crate::{Error, Result};

/// Static configuration of the identity provider integration.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<SecretString>,
    /// `<public url>/auth/callback` — must be registered at the IdP.
    pub redirect_url: String,
    /// Requested in addition to `openid`.
    pub scopes: Vec<String>,
    /// Claim holding the user's groups/roles (array of strings, or one string).
    pub groups_claim: String,
    /// A value of the groups claim that makes the user an administrator. `None` = roles are
    /// never changed by login.
    pub admin_group: Option<String>,
    /// Whether to sync team memberships from the groups claim (D17).
    pub sync_teams: bool,
    /// Only groups starting with this prefix become teams (the prefix is stripped).
    pub team_prefix: String,
}

/// What the provider vouched for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcIdentity {
    pub issuer: String,
    pub subject: String,
    /// `preferred_username`, else `email`, else the subject.
    pub username: String,
    /// `None` when the claim is absent everywhere — then nothing is synced.
    pub groups: Option<Vec<String>>,
}

/// Values to persist between the redirect and the callback.
#[derive(Debug)]
pub struct LoginStart {
    pub authorization_url: String,
    pub state: String,
    pub nonce: String,
    pub code_verifier: SecretString,
}

/// How long discovery (metadata + JWKS) is trusted.
const METADATA_TTL: Duration = Duration::from_hours(1);
/// Minimum interval between refreshes forced by a token whose key is unknown or whose
/// signature does not verify, so failed logins cannot be used to hammer the provider.
const MIN_REFRESH: Duration = Duration::from_secs(30);
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// Signature algorithms accepted for ID tokens. Symmetric (`HS*`) algorithms are never
/// accepted: with them, anything knowing the client secret could mint identities.
const ALGORITHMS: [Algorithm; 9] = [
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::PS256,
    Algorithm::PS384,
    Algorithm::PS512,
    Algorithm::ES256,
    Algorithm::ES384,
    Algorithm::EdDSA,
];

/// The subset of the provider metadata (OpenID Connect Discovery 1.0) that is used.
#[derive(Debug, Clone, Deserialize)]
struct Metadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    userinfo_endpoint: Option<String>,
    #[serde(default)]
    token_endpoint_auth_methods_supported: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RawJwks {
    #[serde(default)]
    keys: Vec<Value>,
}

#[derive(Clone)]
struct Provider {
    metadata: Metadata,
    keys: Arc<Vec<Jwk>>,
    fetched: Instant,
}

#[derive(Default)]
struct Cache {
    provider: Option<Provider>,
    /// When a refresh was last forced (see [`MIN_REFRESH`]).
    forced: Option<Instant>,
}

/// Why an ID token was refused. `stale_keys` marks the failures a key rotation would explain
/// (unknown `kid`, signature mismatch) — the only ones worth a JWKS refresh.
#[derive(Debug)]
struct Rejection {
    reason: String,
    stale_keys: bool,
}

impl Rejection {
    fn claims(reason: impl std::fmt::Display) -> Self {
        Self { reason: reason.to_string(), stale_keys: false }
    }

    fn keys(reason: impl std::fmt::Display) -> Self {
        Self { reason: reason.to_string(), stale_keys: true }
    }
}

impl From<Rejection> for Error {
    fn from(r: Rejection) -> Self {
        rejected(format!("the ID token could not be verified: {}", r.reason))
    }
}

/// The token endpoint's answer. Never logged: both tokens are credentials.
#[derive(Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: Option<SecretString>,
    #[serde(default)]
    id_token: Option<SecretString>,
}

#[derive(Debug, Default, Deserialize)]
struct TokenError {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

/// Talks to the identity provider. Discovery (metadata + JWKS) is cached for an hour and
/// refreshed early when a token points at a key the cache does not have (key rotation).
pub struct OidcClient {
    cfg: OidcConfig,
    http: reqwest::Client,
    cache: Mutex<Cache>,
}

impl std::fmt::Debug for OidcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OidcClient").field("issuer", &self.cfg.issuer).finish_non_exhaustive()
    }
}

fn idp_err(context: &str, e: impl std::fmt::Display) -> Error {
    Error::Upstream { server: "identity provider".into(), reason: format!("{context}: {e}") }
}

fn rejected(reason: impl Into<String>) -> Error {
    Error::Unauthorized { reason: reason.into() }
}

/// 32 random bytes, base64url without padding (43 chars) — also a valid PKCE verifier.
fn random_value() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// `S256` challenge of a PKCE verifier (RFC 7636 §4.2).
fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// `application/x-www-form-urlencoded` encoding of one value.
fn form_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

impl OidcClient {
    pub fn new(cfg: OidcConfig) -> Result<Arc<Self>> {
        // Redirects are never followed, as the OAuth security guidance requires.
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|e| Error::Internal {
                reason: format!("building the OIDC HTTP client: {e}"),
            })?;
        Ok(Arc::new(Self { cfg, http, cache: Mutex::new(Cache::default()) }))
    }

    #[must_use]
    pub fn config(&self) -> &OidcConfig {
        &self.cfg
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str, what: &str) -> Result<T> {
        let res = self.http.get(url).send().await.map_err(|e| idp_err(what, e))?;
        if !res.status().is_success() {
            return Err(idp_err(what, format!("HTTP {}", res.status())));
        }
        res.json().await.map_err(|e| idp_err(what, e))
    }

    /// Metadata and signing keys, from the cache when it is fresh. `force` asks for a refresh
    /// (key rotation); it is honoured at most once every [`MIN_REFRESH`], otherwise the cached
    /// value is returned unchanged.
    async fn provider(&self, force: bool) -> Result<Provider> {
        let mut guard = self.cache.lock().await;
        if let Some(p) = guard.provider.as_ref() {
            let fresh = p.fetched.elapsed() < METADATA_TTL;
            let cooling = guard.forced.is_some_and(|at| at.elapsed() < MIN_REFRESH);
            if (!force && fresh) || (force && cooling) {
                return Ok(p.clone());
            }
        }
        if force {
            guard.forced = Some(Instant::now());
        }
        let url =
            format!("{}/.well-known/openid-configuration", self.cfg.issuer.trim_end_matches('/'));
        let metadata: Metadata = self.get_json(&url, "discovery failed").await?;
        // OpenID Connect Discovery §4.3: the issuer in the document must be the one asked for.
        if metadata.issuer != self.cfg.issuer {
            return Err(idp_err(
                "discovery failed",
                format!(
                    "the provider reports issuer '{}', expected '{}'",
                    metadata.issuer, self.cfg.issuer
                ),
            ));
        }
        let raw: RawJwks = self.get_json(&metadata.jwks_uri, "fetching the JWKS failed").await?;
        // Keys this build cannot use (encryption keys, unknown types) are skipped one by one
        // instead of failing the whole set.
        let keys: Vec<Jwk> =
            raw.keys.into_iter().filter_map(|k| serde_json::from_value(k).ok()).collect();
        let provider = Provider { metadata, keys: Arc::new(keys), fetched: Instant::now() };
        guard.provider = Some(provider.clone());
        Ok(provider)
    }

    /// Builds the authorization URL (PKCE S256, fresh `state` and `nonce`).
    pub async fn begin(&self) -> Result<LoginStart> {
        let provider = self.provider(false).await?;
        let state = random_value();
        let nonce = random_value();
        let verifier = random_value();
        let mut scopes = vec!["openid"];
        scopes.extend(self.cfg.scopes.iter().map(String::as_str).filter(|s| *s != "openid"));
        let mut url = url::Url::parse(&provider.metadata.authorization_endpoint)
            .map_err(|e| idp_err("invalid authorization endpoint", e))?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.cfg.client_id)
            .append_pair("redirect_uri", &self.cfg.redirect_url)
            .append_pair("scope", &scopes.join(" "))
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge", &pkce_challenge(&verifier))
            .append_pair("code_challenge_method", "S256");
        Ok(LoginStart {
            authorization_url: url.into(),
            state,
            nonce,
            code_verifier: SecretString::from(verifier),
        })
    }

    /// Exchanges `code` and verifies the ID token against `nonce`.
    pub async fn finish(
        &self,
        code: &str,
        nonce: &str,
        verifier: &SecretString,
    ) -> Result<OidcIdentity> {
        let provider = self.provider(false).await?;
        let tokens = self.exchange(&provider.metadata, code, verifier).await?;
        let id_token = tokens
            .id_token
            .ok_or_else(|| rejected("the identity provider returned no ID token"))?;
        let claims = match self.verify(&provider.keys, id_token.expose_secret(), nonce) {
            Ok(claims) => claims,
            // The provider may have rotated its signing keys: refresh the JWKS once and verify
            // the same token again (the code is single-use, so the token is not re-requested).
            Err(first) if first.stale_keys => match self.provider(true).await {
                Ok(fresh) if !Arc::ptr_eq(&fresh.keys, &provider.keys) => {
                    self.verify(&fresh.keys, id_token.expose_secret(), nonce)?
                }
                _ => return Err(first.into()),
            },
            Err(definitive) => return Err(definitive.into()),
        };
        self.identity(&provider.metadata, &claims, tokens.access_token.as_ref()).await
    }

    /// The authorization-code grant (RFC 6749 §4.1.3) with the PKCE verifier. A confidential
    /// client authenticates with HTTP Basic unless the provider only supports
    /// `client_secret_post`; a public client sends just its id.
    async fn exchange(
        &self,
        metadata: &Metadata,
        code: &str,
        verifier: &SecretString,
    ) -> Result<TokenResponse> {
        let basic = metadata
            .token_endpoint_auth_methods_supported
            .as_ref()
            .is_none_or(|m| m.iter().any(|x| x == "client_secret_basic"));
        let mut req = self.http.post(&metadata.token_endpoint);
        // The serializer is not `Send`: it must be gone before the request is awaited.
        let body = {
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            form.append_pair("grant_type", "authorization_code")
                .append_pair("code", code)
                .append_pair("redirect_uri", &self.cfg.redirect_url)
                .append_pair("code_verifier", verifier.expose_secret());
            match &self.cfg.client_secret {
                // RFC 6749 §2.3.1: id and secret are form-encoded before Basic encoding.
                Some(secret) if basic => {
                    req = req.basic_auth(
                        form_encode(&self.cfg.client_id),
                        Some(form_encode(secret.expose_secret())),
                    );
                }
                Some(secret) => {
                    form.append_pair("client_id", &self.cfg.client_id)
                        .append_pair("client_secret", secret.expose_secret());
                }
                None => {
                    form.append_pair("client_id", &self.cfg.client_id);
                }
            }
            form.finish()
        };
        let res = req
            .header(http::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(http::header::ACCEPT, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| idp_err("the token endpoint is unreachable", e))?;
        let status = res.status();
        if !status.is_success() {
            // Only the standard error fields are surfaced, never the raw body.
            let err: TokenError = res.json().await.unwrap_or_default();
            let detail = match (err.error, err.error_description) {
                (Some(e), Some(d)) => format!("{e}: {d}"),
                (Some(e), None) => e,
                _ => format!("HTTP {status}"),
            };
            return Err(rejected(format!("code exchange failed: {detail}")));
        }
        res.json().await.map_err(|e| idp_err("invalid token response", e))
    }

    /// Verifies signature, `iss`, `aud`, `azp`, expiry and `nonce`; returns the payload.
    fn verify(
        &self,
        keys: &[Jwk],
        id_token: &str,
        nonce: &str,
    ) -> std::result::Result<Value, Rejection> {
        let header = jsonwebtoken::decode_header(id_token).map_err(Rejection::claims)?;
        if !ALGORITHMS.contains(&header.alg) {
            return Err(Rejection::claims(format_args!(
                "algorithm {:?} is not accepted",
                header.alg
            )));
        }
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[&self.cfg.issuer]);
        validation.set_audience(&[&self.cfg.client_id]);
        // jsonwebtoken only checks `iss`/`aud` when present, so their presence is required.
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);

        // With a `kid`, only that key; without one, every key of the set is a candidate.
        let candidates = keys.iter().filter(|k| match &header.kid {
            Some(kid) => k.common.key_id.as_ref() == Some(kid),
            None => true,
        });
        let mut failure = Rejection::keys("no matching key in the provider's JWKS");
        let mut payload = None;
        for jwk in candidates {
            let Ok(key) = DecodingKey::from_jwk(jwk) else { continue };
            match jsonwebtoken::decode::<Value>(id_token, &key, &validation) {
                Ok(data) => {
                    payload = Some(data.claims);
                    break;
                }
                // Not this key (or not a key for this algorithm): try the next one.
                Err(e)
                    if matches!(
                        e.kind(),
                        ErrorKind::InvalidSignature
                            | ErrorKind::InvalidAlgorithm
                            | ErrorKind::InvalidKeyFormat
                            | ErrorKind::InvalidRsaKey(_)
                            | ErrorKind::InvalidEcdsaKey
                            | ErrorKind::InvalidEddsaKey
                    ) =>
                {
                    failure = Rejection::keys(e);
                }
                // The signature verified and a claim was refused: no other key changes that.
                Err(e) => return Err(Rejection::claims(e)),
            }
        }
        let Some(claims) = payload else { return Err(failure) };

        if claims.get("nonce").and_then(Value::as_str) != Some(nonce) {
            return Err(Rejection::claims("nonce mismatch"));
        }
        // OIDC Core §3.1.3.7: when an authorized party is named, it must be this client.
        if let Some(azp) = claims.get("azp")
            && azp.as_str() != Some(self.cfg.client_id.as_str())
        {
            return Err(Rejection::claims("the token was issued to another client (azp)"));
        }
        if claims.get("sub").and_then(Value::as_str).is_none_or(str::is_empty) {
            return Err(Rejection::claims("missing subject"));
        }
        Ok(claims)
    }

    /// Builds the identity from **verified** claims.
    async fn identity(
        &self,
        metadata: &Metadata,
        claims: &Value,
        access_token: Option<&SecretString>,
    ) -> Result<OidcIdentity> {
        let text = |name: &str| {
            claims.get(name).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_owned)
        };
        let subject = text("sub").ok_or_else(|| rejected("the ID token has no subject"))?;
        let username =
            text("preferred_username").or_else(|| text("email")).unwrap_or_else(|| subject.clone());
        let mut groups = groups_from(claims, &self.cfg.groups_claim);
        if groups.is_none()
            && let (Some(url), Some(token)) = (&metadata.userinfo_endpoint, access_token)
        {
            groups = self.userinfo_groups(url, token, &subject).await;
        }
        Ok(OidcIdentity { issuer: self.cfg.issuer.clone(), subject, username, groups })
    }

    /// Groups from the userinfo endpoint. Best effort: any failure means "not provided".
    async fn userinfo_groups(
        &self,
        url: &str,
        access_token: &SecretString,
        subject: &str,
    ) -> Option<Vec<String>> {
        let res = self.http.get(url).bearer_auth(access_token.expose_secret()).send().await.ok()?;
        if !res.status().is_success() {
            return None;
        }
        let v: Value = res.json().await.ok()?;
        // OIDC Core §5.3.2: a userinfo response for another subject must not be used.
        if v.get("sub").and_then(Value::as_str) != Some(subject) {
            return None;
        }
        groups_from(&v, &self.cfg.groups_claim)
    }
}

/// Reads a groups claim: an array of strings, or a single (comma/space separated) string.
/// `None` when the claim is absent, so callers can tell "no groups" from "not provided".
fn groups_from(claims: &Value, name: &str) -> Option<Vec<String>> {
    match claims.get(name)? {
        Value::Array(items) => {
            Some(items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        }
        Value::String(s) => Some(
            s.split([',', ' '])
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_owned)
                .collect(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_claim_shapes() {
        let cases = [
            (serde_json::json!({"groups": ["a", "b"]}), Some(vec!["a", "b"])),
            (serde_json::json!({"groups": "a, b c"}), Some(vec!["a", "b", "c"])),
            (serde_json::json!({"groups": []}), Some(vec![])),
            (serde_json::json!({"roles": ["a"]}), None),
            (serde_json::json!({"groups": 3}), None),
        ];
        for (claims, expected) in cases {
            let got = groups_from(&claims, "groups");
            assert_eq!(
                got,
                expected.map(|v| v.into_iter().map(str::to_owned).collect::<Vec<_>>()),
                "{claims}"
            );
        }
    }

    #[test]
    fn pkce_challenge_matches_the_rfc_example() {
        // RFC 7636, appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_values_are_valid_pkce_verifiers() {
        let v = random_value();
        assert_eq!(v.len(), 43);
        assert!(v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(v, random_value());
    }

    #[test]
    fn symmetric_algorithms_are_never_accepted() {
        for alg in [Algorithm::HS256, Algorithm::HS384, Algorithm::HS512] {
            assert!(!ALGORITHMS.contains(&alg));
        }
    }
}
