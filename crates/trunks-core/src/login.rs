//! Browser logins and sessions (D16/D17).
//!
//! Two ways in, one way of staying in: an OIDC round-trip or a Trunks access token both end in
//! a server-side session referenced by an opaque cookie value. Only the SHA-256 of that value
//! is stored. The CLI and MCP clients keep using bearer tokens.

use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use sha2::{Digest as _, Sha256};
use trunks_types::Role;

use crate::domain::UserRecord;
use crate::identity::{IdentityService, UserPrincipal, generate_token};
use crate::oidc::{OidcClient, OidcIdentity};
use crate::ports::{AccessStore, LoginState, SessionStore};
use crate::{Error, Result};

/// Lifetime of an in-flight login (redirect → callback).
const LOGIN_STATE_TTL_MINUTES: i64 = 10;

#[derive(Clone)]
pub struct LoginService {
    oidc: Option<Arc<OidcClient>>,
    identity: IdentityService,
    sessions: Arc<dyn SessionStore>,
    access: Arc<dyn AccessStore>,
    session_ttl: ChronoDuration,
}

impl std::fmt::Debug for LoginService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginService").field("oidc", &self.oidc.is_some()).finish_non_exhaustive()
    }
}

/// SHA-256 of the cookie value, lowercase hex.
fn hash(session: &str) -> String {
    use std::fmt::Write as _;
    Sha256::digest(session.as_bytes()).iter().fold(String::with_capacity(64), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// Only same-site relative paths are accepted as a post-login destination (no open redirect).
#[must_use]
pub fn safe_return_to(candidate: Option<&str>) -> String {
    match candidate.map(str::trim) {
        Some(p)
            if p.starts_with('/')
                && !p.starts_with("//")
                && !p.contains('\\')
                && !p.contains("://") =>
        {
            p.to_owned()
        }
        _ => "/".to_owned(),
    }
}

/// The team a group value maps to under `prefix`, if any: the prefix is stripped and the
/// rest must be a valid identity name (no whitespace, ≤ 100 chars).
#[must_use]
pub fn team_for_group(group: &str, prefix: &str) -> Option<String> {
    let rest = group.strip_prefix(prefix)?.trim();
    if rest.is_empty() || rest.chars().count() > 100 || rest.chars().any(char::is_whitespace) {
        return None;
    }
    Some(rest.to_owned())
}

impl LoginService {
    #[must_use]
    pub fn new(
        oidc: Option<Arc<OidcClient>>,
        identity: IdentityService,
        sessions: Arc<dyn SessionStore>,
        access: Arc<dyn AccessStore>,
        session_ttl: std::time::Duration,
    ) -> Self {
        let session_ttl =
            ChronoDuration::from_std(session_ttl).unwrap_or(ChronoDuration::hours(12));
        Self { oidc, identity, sessions, access, session_ttl }
    }

    #[must_use]
    pub fn oidc_enabled(&self) -> bool {
        self.oidc.is_some()
    }

    #[must_use]
    pub fn session_ttl_seconds(&self) -> i64 {
        self.session_ttl.num_seconds()
    }

    fn oidc(&self) -> Result<&Arc<OidcClient>> {
        self.oidc.as_ref().ok_or_else(|| Error::not_found("OIDC login (not configured)"))
    }

    /// Starts an OIDC login: persists `state`/`nonce`/PKCE verifier and returns the IdP URL.
    pub async fn begin_oidc(&self, return_to: Option<&str>) -> Result<String> {
        let start = self.oidc()?.begin().await?;
        self.sessions
            .save_login_state(&LoginState {
                state: start.state,
                nonce: start.nonce,
                code_verifier: start.code_verifier,
                return_to: safe_return_to(return_to),
                expires_at: Utc::now() + ChronoDuration::minutes(LOGIN_STATE_TTL_MINUTES),
            })
            .await?;
        Ok(start.authorization_url)
    }

    /// Finishes an OIDC login: consumes the state (single use), verifies the ID token,
    /// provisions the user, and opens a session. Returns `(session cookie value, return_to)`.
    pub async fn finish_oidc(&self, code: &str, state: &str) -> Result<(String, String)> {
        let oidc = self.oidc()?;
        let st =
            self.sessions.take_login_state(state).await?.ok_or_else(|| Error::Unauthorized {
                reason: "unknown or expired login state; start the login again".into(),
            })?;
        let identity = oidc.finish(code, &st.nonce, &st.code_verifier).await?;
        let user = self.provision(oidc, &identity).await?;
        let session = self.open_session(&user).await?;
        tracing::info!(user = %user.username, "OIDC login");
        Ok((session, st.return_to))
    }

    /// Exchanges a Trunks access token for a browser session.
    pub async fn login_with_token(&self, token: &str) -> Result<(String, UserRecord)> {
        let UserPrincipal::User { user, .. } = self
            .identity
            .authenticate_user(trunks_types::ServerMode::Enterprise, Some(token))
            .await?
        else {
            return Err(Error::Unauthorized {
                reason: "token login requires enterprise mode".into(),
            });
        };
        let session = self.open_session(&user).await?;
        Ok((session, (*user).clone()))
    }

    async fn open_session(&self, user: &UserRecord) -> Result<String> {
        let value = generate_token();
        self.sessions.create_session(&hash(&value), user.id, Utc::now() + self.session_ttl).await?;
        Ok(value)
    }

    /// The principal of a live session, with access resolved like any other user login.
    pub async fn authenticate_session(&self, cookie: &str) -> Result<UserPrincipal> {
        let user = self.sessions.session_user(&hash(cookie)).await?.ok_or_else(|| {
            Error::Unauthorized { reason: "the session has expired; log in again".into() }
        })?;
        self.identity.principal_of(user).await
    }

    /// Idempotent.
    pub async fn logout(&self, cookie: &str) -> Result<()> {
        self.sessions.delete_session(&hash(cookie)).await
    }

    pub async fn purge_expired(&self) -> Result<u64> {
        self.sessions.purge_expired_sessions().await
    }

    /// Just-in-time provisioning (D17): find or create the user bound to `(issuer, subject)`,
    /// then align role and claim-synced team memberships with the groups claim.
    async fn provision(&self, oidc: &OidcClient, id: &OidcIdentity) -> Result<UserRecord> {
        let cfg = oidc.config();
        let is_admin =
            |groups: &[String]| cfg.admin_group.as_ref().is_some_and(|g| groups.contains(g));
        let existing = self.sessions.user_by_oidc(&id.issuer, &id.subject).await?;
        let mut user = if let Some(u) = existing {
            u
        } else {
            let username = usable_username(&id.username, &id.subject);
            // Never link to an existing local account by name: that would let the IdP's
            // naming take over a local identity (e.g. the bootstrap `admin`).
            if self.identity.user_by_name(&username).await?.is_some() {
                return Err(Error::Forbidden {
                    reason: format!(
                        "a local user named '{username}' already exists; an administrator must remove or rename it before this identity can log in"
                    ),
                });
            }
            let role = match id.groups.as_deref() {
                Some(g) if is_admin(g) => Role::Admin,
                _ => Role::User,
            };
            self.sessions
                .create_oidc_user(&username, role, &generate_token(), &id.issuer, &id.subject)
                .await?
        };
        let Some(groups) = id.groups.as_deref() else { return Ok(user) };
        if cfg.admin_group.is_some() {
            let desired = if is_admin(groups) { Role::Admin } else { Role::User };
            // Never demote the last administrator: that would lock everyone out.
            let last_admin = desired == Role::User
                && user.role == Role::Admin
                && self.identity.users().await?.iter().filter(|u| u.role == Role::Admin).count()
                    <= 1;
            if last_admin {
                tracing::warn!(user = %user.username, "not demoting the last administrator despite the claims");
            } else if desired != user.role {
                self.sessions.set_user_role(user.id, desired).await?;
                tracing::info!(user = %user.username, role = %desired, "role synced from identity provider");
                user.role = desired;
            }
        }
        if cfg.sync_teams {
            let mut team_ids = Vec::new();
            for name in groups.iter().filter_map(|g| team_for_group(g, &cfg.team_prefix)) {
                let team = match self.access.team_by_name(&name).await? {
                    Some(t) => t,
                    None => {
                        self.access.create_team(&name, "Synced from the identity provider").await?
                    }
                };
                team_ids.push(team.id);
            }
            team_ids.sort_unstable();
            team_ids.dedup();
            self.sessions.sync_oidc_memberships(user.id, &team_ids).await?;
        }
        Ok(user)
    }
}

/// A username that satisfies the identity-name rules, falling back to the subject.
fn usable_username(candidate: &str, subject: &str) -> String {
    let c = candidate.trim();
    if !c.is_empty() && c.chars().count() <= 100 && !c.chars().any(char::is_whitespace) {
        c.to_owned()
    } else {
        subject.chars().filter(|ch| !ch.is_whitespace()).take(100).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_to_only_accepts_relative_paths() {
        let cases = [
            (Some("/servers"), "/servers"),
            (Some("/a?b=c#d"), "/a?b=c#d"),
            (Some("//evil.example"), "/"),
            (Some("https://evil.example/"), "/"),
            (Some("/\\evil"), "/"),
            (Some("/x?next=http://evil"), "/"),
            (Some("servers"), "/"),
            (Some(""), "/"),
            (None, "/"),
        ];
        for (input, expected) in cases {
            assert_eq!(safe_return_to(input), expected, "{input:?}");
        }
    }

    #[test]
    fn groups_map_to_teams_by_prefix() {
        let cases = [
            ("trunks-team-data", "trunks-team-", Some("data")),
            ("data", "", Some("data")),
            ("other-data", "trunks-team-", None),
            ("trunks-team-", "trunks-team-", None),
            ("trunks-team-has space", "trunks-team-", None),
        ];
        for (group, prefix, expected) in cases {
            assert_eq!(team_for_group(group, prefix).as_deref(), expected, "{group}");
        }
    }

    #[test]
    fn usernames_fall_back_to_the_subject() {
        assert_eq!(usable_username("alice@example.com", "sub-1"), "alice@example.com");
        assert_eq!(usable_username("Alice Smith", "sub-1"), "sub-1");
        assert_eq!(usable_username("", "sub 2"), "sub2");
    }

    #[test]
    fn session_hash_is_sha256_hex() {
        assert_eq!(hash("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
