//! Upstream OAuth (`docs/02-especificacao.md` §4.8), built on `rmcp::transport::auth`.
//!
//! Two phases, both surviving a restart because every piece of state lives in the store:
//! 1. **bootstrap** — the upstream answered 401; discover the authorization server, register
//!    a client (DCR) unless `oauth_client_id` was given, build the authorization URL (PKCE,
//!    `state`), and persist a pending session carrying the original registration input.
//! 2. **exchange** — with the `code` from the browser round-trip, trade it for tokens, which the
//!    library persists through [`DbCredentialStore`]; the caller then registers the server, and
//!    `UpstreamConn` wraps its HTTP client in [`AuthClient`] (bearer + transparent refresh).
//!
//! This is the third module allowed to import `rmcp` (only its `transport::auth` part).

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use oauth2::TokenResponse as _;
use rand::RngCore as _;
use rmcp::transport::auth::{
    AuthClient, AuthError, AuthorizationManager, CredentialStore, OAuthClientConfig,
    OAuthTokenResponse, StateStore, StoredAuthorizationState, StoredCredentials,
};
use secrecy::{ExposeSecret as _, SecretString};
use serde_json::Value;
use trunks_types::api::{AuthorizationRequired, RegisterServerInput};
use trunks_types::{ServerName, Transport};

use crate::ports::{OAuthCredentials, OAuthStore, PendingAuthorization};
use crate::{Error, Result};

/// Lifetime of a pending authorization.
pub const PENDING_TTL_MINUTES: i64 = 10;

fn oauth_err(server: &ServerName, e: &AuthError) -> Error {
    Error::Upstream { server: server.to_string(), reason: format!("oauth: {e}") }
}

fn store_err(e: &Error) -> AuthError {
    AuthError::InternalError(e.to_string())
}

/// 32 random bytes as lowercase hex (64 chars).
#[must_use]
pub fn session_id() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().fold(String::with_capacity(64), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// The library's view of our `upstream_oauth_tokens` row for one server.
#[derive(Clone)]
pub struct DbCredentialStore {
    store: Arc<dyn OAuthStore>,
    server: ServerName,
}

impl std::fmt::Debug for DbCredentialStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbCredentialStore").field("server", &self.server).finish_non_exhaustive()
    }
}

impl DbCredentialStore {
    #[must_use]
    pub fn new(store: Arc<dyn OAuthStore>, server: ServerName) -> Self {
        Self { store, server }
    }
}

fn to_stored(c: &OAuthCredentials) -> Option<StoredCredentials> {
    let client_id = c.client_id.clone()?;
    let token_response = c.access_token.as_ref().map(|at| {
        // Rebuild the library's token response from the columns; `expires_in` is derived from
        // `expires_at` so the refresh buffer keeps working after a restart.
        let mut tr = serde_json::json!({
            "access_token": at.expose_secret(),
            "token_type": c.token_type.clone().unwrap_or_else(|| "bearer".into()),
        });
        if let Some(rt) = &c.refresh_token {
            tr["refresh_token"] = Value::String(rt.expose_secret().to_owned());
        }
        if let Some(scope) = &c.scope {
            tr["scope"] = Value::String(scope.clone());
        }
        if let Some(exp) = c.expires_at {
            tr["expires_in"] = Value::from((exp - Utc::now()).num_seconds().max(0));
        }
        serde_json::from_value::<OAuthTokenResponse>(tr).ok()
    });
    let token_response = token_response.flatten();
    let granted_scopes = c
        .scope
        .as_deref()
        .map_or_else(|| c.scopes.clone(), |s| s.split_whitespace().map(str::to_owned).collect());
    let token_received_at = token_response
        .as_ref()
        .map(|_| SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()));
    Some(
        StoredCredentials::new(client_id, token_response, granted_scopes, token_received_at)
            .with_issuer(c.issuer.clone()),
    )
}

#[async_trait]
impl CredentialStore for DbCredentialStore {
    async fn load(&self) -> std::result::Result<Option<StoredCredentials>, AuthError> {
        let Some(c) = self.store.credentials(&self.server).await.map_err(|e| store_err(&e))? else {
            return Ok(None);
        };
        if !c.has_tokens() {
            return Ok(None);
        }
        Ok(to_stored(&c))
    }

    async fn save(&self, creds: StoredCredentials) -> std::result::Result<(), AuthError> {
        // Keep the registration metadata (secret, redirect, requested scopes) we already hold.
        let mut row = self
            .store
            .credentials(&self.server)
            .await
            .map_err(|e| store_err(&e))?
            .unwrap_or_default();
        row.client_id = Some(creds.client_id.clone());
        row.issuer = creds.issuer.clone().or(row.issuer);
        if let Some(tr) = &creds.token_response {
            row.access_token = Some(SecretString::from(tr.access_token().secret().clone()));
            row.token_type = Some("bearer".into());
            row.refresh_token = tr.refresh_token().map(|r| SecretString::from(r.secret().clone()));
            row.scope = if creds.granted_scopes.is_empty() {
                None
            } else {
                Some(creds.granted_scopes.join(" "))
            };
            row.expires_at = tr.expires_in().map(|d| {
                let received = creds
                    .token_received_at
                    .and_then(|s| DateTime::<Utc>::from_timestamp(i64::try_from(s).unwrap_or(0), 0))
                    .unwrap_or_else(Utc::now);
                received + ChronoDuration::seconds(i64::try_from(d.as_secs()).unwrap_or(0))
            });
        } else {
            row.access_token = None;
            row.token_type = None;
            row.refresh_token = None;
            row.scope = None;
            row.expires_at = None;
        }
        self.store.save_credentials(&self.server, &row).await.map_err(|e| store_err(&e))
    }

    async fn clear(&self) -> std::result::Result<(), AuthError> {
        self.store.clear_tokens(&self.server).await.map_err(|e| store_err(&e))
    }
}

/// `StateStore` backed by memory: during bootstrap it captures what the library generates (the
/// service then persists it inside the pending session); during the exchange it is pre-loaded
/// from that session. Keying by `state` is preserved.
#[derive(Clone, Default)]
pub struct CapturedStateStore(Arc<Mutex<Option<(String, StoredAuthorizationState)>>>);

impl std::fmt::Debug for CapturedStateStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapturedStateStore").field("captured", &self.captured().is_some()).finish()
    }
}

impl CapturedStateStore {
    #[must_use]
    pub fn preloaded(state: &str, st: StoredAuthorizationState) -> Self {
        Self(Arc::new(Mutex::new(Some((state.to_owned(), st)))))
    }

    fn captured(&self) -> Option<(String, StoredAuthorizationState)> {
        self.0.lock().ok().and_then(|g| g.clone())
    }
}

#[async_trait]
impl StateStore for CapturedStateStore {
    async fn save(
        &self,
        csrf: &str,
        st: StoredAuthorizationState,
    ) -> std::result::Result<(), AuthError> {
        if let Ok(mut g) = self.0.lock() {
            *g = Some((csrf.to_owned(), st));
        }
        Ok(())
    }

    async fn load(
        &self,
        csrf: &str,
    ) -> std::result::Result<Option<StoredAuthorizationState>, AuthError> {
        Ok(self.captured().filter(|(k, _)| k == csrf).map(|(_, v)| v))
    }

    async fn delete(&self, _csrf: &str) -> std::result::Result<(), AuthError> {
        if let Ok(mut g) = self.0.lock() {
            *g = None;
        }
        Ok(())
    }
}

/// Phase 1: builds the authorization URL and the pending session for `input`.
///
/// `www_authenticate` is the upstream's challenge (used for RFC 9728 discovery when present).
pub async fn bootstrap(
    store: &Arc<dyn OAuthStore>,
    server: &ServerName,
    url: &str,
    input: &RegisterServerInput,
    force: bool,
    www_authenticate: Option<&str>,
) -> Result<AuthorizationRequired> {
    let redirect = input
        .oauth_redirect_uri
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .ok_or_else(|| Error::UpstreamOAuthRequired {
            reason: format!(
                "server '{server}' requires OAuth; provide oauth_redirect_uri (the CLI does this automatically)"
            ),
        })?;
    let mut manager = AuthorizationManager::new_allowing_private_addresses(url)
        .await
        .map_err(|e| oauth_err(server, &e))?;
    let credentials = DbCredentialStore::new(store.clone(), server.clone());
    let captured = CapturedStateStore::default();
    manager.set_credential_store(credentials);
    manager.set_state_store(captured.clone());

    let resolution = match www_authenticate {
        Some(h) => manager.resolve_metadata_from_challenge(Some(h)).await,
        None => manager.resolve_metadata().await,
    }
    .map_err(|e| oauth_err(server, &e))?;
    let issuer = resolution.metadata.issuer.clone();
    manager.set_metadata(resolution.metadata);

    let scopes: Vec<&str> = input.oauth_scopes.iter().map(String::as_str).collect();
    let (client_id, client_secret) = if let Some(id) =
        input.oauth_client_id.as_deref().filter(|c| !c.is_empty())
    {
        let mut cfg = OAuthClientConfig::new(id, redirect);
        if let Some(secret) = input.oauth_client_secret.as_deref().filter(|s| !s.is_empty()) {
            cfg = cfg.with_client_secret(secret);
        }
        manager.configure_client(cfg).map_err(|e| oauth_err(server, &e))?;
        (id.to_owned(), input.oauth_client_secret.clone().filter(|s| !s.is_empty()))
    } else {
        let cfg = manager
                .register_client(&format!("trunks-{server}"), redirect, &scopes)
                .await
                .map_err(|e| {
                    Error::invalid_input(format!(
                        "upstream '{server}': dynamic client registration failed ({e}); provide oauth_client_id (and oauth_client_secret) of a pre-registered client"
                    ))
                })?;
        (cfg.client_id.clone(), cfg.client_secret.clone())
    };
    let authorization_url =
        manager.get_authorization_url(&scopes).await.map_err(|e| oauth_err(server, &e))?;
    let Some((state, st)) = captured.captured() else {
        return Err(Error::Internal {
            reason: "oauth: authorization state was not captured".into(),
        });
    };

    // Client registration is persisted now so the exchange (possibly after a restart) can
    // reuse it; the token fields stay empty until the exchange.
    let creds = OAuthCredentials {
        client_id: Some(client_id.clone()),
        client_secret: client_secret.clone().map(SecretString::from),
        redirect_uri: Some(redirect.to_owned()),
        scopes: input.oauth_scopes.clone(),
        issuer: issuer.clone(),
        ..Default::default()
    };
    store.save_credentials(server, &creds).await?;

    let expires_at = Utc::now() + ChronoDuration::minutes(PENDING_TTL_MINUTES);
    let mut server_input = serde_json::to_value(input)
        .map_err(|e| Error::Internal { reason: format!("serializing registration input: {e}") })?;
    // The pending row carries the client id so a restart can rebuild the OAuth client.
    server_input["oauth_client_id"] = Value::String(client_id.clone());
    let pending = PendingAuthorization {
        session_id: session_id(),
        server_name: server.clone(),
        transport: Transport::StreamableHttp,
        server_input,
        force,
        redirect_uri: redirect.to_owned(),
        client_id: Some(client_id),
        client_secret: client_secret.map(SecretString::from),
        scopes: st.requested_scopes.clone(),
        state,
        code_verifier: SecretString::from(st.pkce_verifier.clone()),
        expires_at,
        initiated_by: None,
    };
    store.save_pending(&pending).await?;
    Ok(AuthorizationRequired { session_id: pending.session_id, authorization_url, expires_at })
}

/// Phase 2: validates `state` against the pending session and exchanges `code` for tokens,
/// which are persisted through the credential store. Returns the registration input to replay.
pub async fn exchange(
    store: &Arc<dyn OAuthStore>,
    pending: &PendingAuthorization,
    url: &str,
    code: &str,
    state: &str,
) -> Result<()> {
    let server = &pending.server_name;
    if pending.expires_at < Utc::now() {
        store.delete_pending(&pending.session_id).await?;
        return Err(Error::invalid_input(format!(
            "the authorization session for '{server}' has expired; run register again"
        )));
    }
    if pending.state != state {
        return Err(Error::invalid_input("state does not match the pending authorization"));
    }
    let Some(client_id) = pending.client_id.clone() else {
        return Err(Error::Internal { reason: "pending authorization without client_id".into() });
    };
    let mut manager = AuthorizationManager::new_allowing_private_addresses(url)
        .await
        .map_err(|e| oauth_err(server, &e))?;
    let credentials = DbCredentialStore::new(store.clone(), server.clone());
    manager.set_credential_store(credentials);
    // Non-exhaustive in rmcp: build it through its public constructor.
    let st = StoredAuthorizationState::new(
        &oauth2::PkceCodeVerifier::new(pending.code_verifier.expose_secret().to_owned()),
        &oauth2::CsrfToken::new(pending.state.clone()),
    )
    .with_requested_scopes(pending.scopes.clone());
    manager.set_state_store(CapturedStateStore::preloaded(&pending.state, st));
    let resolution = manager.resolve_metadata().await.map_err(|e| oauth_err(server, &e))?;
    manager.set_metadata(resolution.metadata);
    let mut cfg = OAuthClientConfig::new(&client_id, &pending.redirect_uri);
    if let Some(secret) = &pending.client_secret {
        cfg = cfg.with_client_secret(secret.expose_secret());
    }
    manager.configure_client(cfg).map_err(|e| oauth_err(server, &e))?;
    manager.exchange_code_for_token(code, state).await.map_err(|e| {
        Error::invalid_input(format!("upstream '{server}': token exchange failed: {e}"))
    })?;
    Ok(())
}

/// Builds the HTTP client used by `UpstreamConn` when the server has stored tokens: the given
/// `reqwest::Client` (already carrying configured headers) wrapped so every request gets a
/// fresh bearer. `None` when there are no tokens.
pub async fn auth_client(
    store: &Arc<dyn OAuthStore>,
    server: &ServerName,
    url: &str,
    http: reqwest::Client,
) -> Result<Option<AuthClient<reqwest::Client>>> {
    let Some(creds) = store.credentials(server).await? else { return Ok(None) };
    if !creds.has_tokens() {
        return Ok(None);
    }
    let mut manager = AuthorizationManager::new_allowing_private_addresses(url)
        .await
        .map_err(|e| oauth_err(server, &e))?;
    manager.set_credential_store(DbCredentialStore::new(store.clone(), server.clone()));
    manager.set_state_store(CapturedStateStore::default());
    let restored = manager.initialize_from_store().await.map_err(|e| oauth_err(server, &e))?;
    if !restored {
        return Ok(None);
    }
    Ok(Some(AuthClient::new(http, manager)))
}
