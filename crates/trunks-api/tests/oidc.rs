//! OIDC login for humans (D16/D17) against the fake identity provider: the tests play the
//! browser (no redirect following, explicit cookie handling).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::too_many_lines)]

use std::collections::HashMap;
use std::sync::Arc;

use secrecy::SecretString;
use trunks_api::AppState;
use trunks_api::mcp::{McpMounts, McpOptions};
use trunks_core::catalog::Catalog;
use trunks_core::dashboard::RuntimeInfo;
use trunks_core::gateway::GatewayState;
use trunks_core::groups::GroupService;
use trunks_core::identity::IdentityService;
use trunks_core::login::LoginService;
use trunks_core::mode::ModeService;
use trunks_core::oidc::{OidcClient, OidcConfig};
use trunks_core::registry::RegistryService;
use trunks_core::sessions::{SessionManager, SessionOptions};
use trunks_core::upstream::ConnectOptions;
use trunks_db::{Db, Store};
use trunks_testing::oidc::{CLIENT_ID, FakeIdp, Identity};
use trunks_types::ServerMode;

struct App {
    base: String,
    admin_token: String,
    http: reqwest::Client,
    _dir: tempfile::TempDir,
}

async fn boot(idp: &FakeIdp, admin_group: Option<&str>, sync_teams: bool) -> App {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::connect(&format!("sqlite://{}/db.sqlite", dir.path().display()), 4).await.unwrap();
    db.migrate().await.unwrap();
    let store = Arc::new(Store(db));
    let identity = IdentityService::new(store.clone(), store.clone());
    let mode_svc = ModeService::load(store.clone(), identity.clone()).await.unwrap();
    let init = mode_svc.init(ServerMode::Enterprise).await.unwrap();
    let catalog = Catalog::empty();
    catalog.reload(store.as_ref()).await.unwrap();
    let sessions = SessionManager::new(SessionOptions::default());
    let gateway = Arc::new(GatewayState {
        catalog: catalog.clone(),
        sessions: sessions.clone(),
        metrics: None,
        version: "vtest".into(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let oidc = OidcClient::new(OidcConfig {
        issuer: idp.issuer(),
        client_id: CLIENT_ID.into(),
        client_secret: Some(SecretString::from("idp-secret".to_owned())),
        redirect_url: format!("{base}/auth/callback"),
        scopes: vec!["profile".into(), "email".into()],
        groups_claim: "groups".into(),
        admin_group: admin_group.map(str::to_owned),
        sync_teams,
        team_prefix: "trunks-team-".into(),
    })
    .unwrap();
    let state = AppState {
        catalog: catalog.clone(),
        gateway: gateway.clone(),
        mcp: McpMounts::new(gateway, McpOptions::default()),
        registry_service: RegistryService::new(
            store.clone(),
            store.clone(),
            catalog.clone(),
            sessions,
            ConnectOptions::default(),
        ),
        groups: GroupService::new(store.clone(), catalog),
        registry: store.clone(),
        login: LoginService::new(
            Some(oidc),
            identity.clone(),
            store.clone(),
            store.clone(),
            std::time::Duration::from_hours(1),
        ),
        public_url: Some(base.clone()),
        identity,
        mode: mode_svc,
        metrics: None,
        runtime: RuntimeInfo {
            mode: ServerMode::Enterprise,
            version: "vtest".into(),
            started_at: chrono::Utc::now(),
            database_backend: "sqlite".into(),
            database_url: "sqlite://test".into(),
            migrations_latest: None,
            bind_host: "127.0.0.1".into(),
            bind_port: addr.port(),
            allowed_hosts: vec![],
            upstream_init_timeout_sec: 30,
            session_idle_timeout_sec: 0,
            metrics_enabled: false,
        },
    };
    let app = trunks_api::router(state);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    App {
        base,
        admin_token: init.admin_access_token.unwrap(),
        http: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        _dir: dir,
    }
}

/// `GET /auth/login`: returns `(state, nonce)` from the redirect to the provider.
async fn start_login(app: &App, return_to: Option<&str>) -> (String, String) {
    let url = match return_to {
        Some(r) => format!("{}/auth/login?return_to={}", app.base, urlencode(r)),
        None => format!("{}/auth/login", app.base),
    };
    let res = app.http.get(url).send().await.unwrap();
    assert_eq!(res.status(), 302, "login redirects to the provider");
    let loc = res.headers()["location"].to_str().unwrap().to_owned();
    let u = url::Url::parse(&loc).unwrap();
    assert!(u.path().ends_with("/authorize"), "{loc}");
    let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
    assert_eq!(q["client_id"], CLIENT_ID);
    assert_eq!(q["code_challenge_method"], "S256");
    assert!(q["scope"].contains("openid") && q["scope"].contains("email"));
    assert!(q["redirect_uri"].ends_with("/auth/callback"));
    (q["state"].clone(), q["nonce"].clone())
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// `GET /auth/callback`: returns `(status, location, session cookie value, body)`.
async fn callback(
    app: &App,
    code: &str,
    state: &str,
) -> (u16, Option<String>, Option<String>, String) {
    let res = app
        .http
        .get(format!("{}/auth/callback?code={code}&state={state}", app.base))
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    let location = res.headers().get("location").map(|v| v.to_str().unwrap().to_owned());
    let cookie =
        res.headers().get("set-cookie").map(|v| v.to_str().unwrap().to_owned()).and_then(|c| {
            c.split(';').next().and_then(|kv| kv.split_once('=')).map(|(_, v)| v.to_owned())
        });
    (status, location, cookie, res.text().await.unwrap())
}

fn ident(sub: &str, username: &str, groups: Option<&[&str]>, nonce: &str) -> Identity {
    Identity {
        sub: sub.into(),
        preferred_username: Some(username.into()),
        email: Some(format!("{username}@example.com")),
        groups: groups.map(|g| g.iter().map(|s| (*s).to_owned()).collect()),
        nonce: nonce.into(),
        ..Identity::default()
    }
}

async fn login(app: &App, idp: &FakeIdp, id: impl FnOnce(&str) -> Identity) -> String {
    let (state, nonce) = start_login(app, None).await;
    let code = idp.issue_code(id(&nonce));
    let (status, _, cookie, body) = callback(app, &code, &state).await;
    assert_eq!(status, 302, "{body}");
    cookie.expect("a session cookie")
}

async fn whoami(app: &App, cookie: &str) -> serde_json::Value {
    let res = app
        .http
        .get(format!("{}/api/v0/users/whoami", app.base))
        .header("cookie", format!("trunks_session={cookie}"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    res.json().await.unwrap()
}

#[tokio::test]
async fn oidc_login_provisions_the_user_and_opens_a_session() {
    let idp = FakeIdp::start().await;
    let app = boot(&idp, Some("trunks-admins"), true).await;

    // what the web app sees before anyone is logged in
    let cfg: serde_json::Value = app
        .http
        .get(format!("{}/auth/config", app.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        cfg,
        serde_json::json!({"mode": "enterprise", "login_required": true, "oidc": true})
    );

    // full round-trip; `return_to` survives, and only relative paths are honoured
    let (state, nonce) = start_login(&app, Some("/servers?tab=tools")).await;
    let code =
        idp.issue_code(ident("sub-alice", "alice", Some(&["trunks-team-data", "other"]), &nonce));
    let res = app
        .http
        .get(format!("{}/auth/callback?code={code}&state={state}", app.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 302);
    assert_eq!(res.headers()["location"], "/servers?tab=tools");
    let set_cookie = res.headers()["set-cookie"].to_str().unwrap().to_owned();
    assert!(set_cookie.starts_with("trunks_session="));
    assert!(
        set_cookie.contains("HttpOnly")
            && set_cookie.contains("SameSite=Lax")
            && set_cookie.contains("Path=/")
    );
    assert!(!set_cookie.contains("Secure"), "http public URL → no Secure flag");
    let cookie = set_cookie.split(';').next().unwrap().split_once('=').unwrap().1.to_owned();

    // JIT: user created with role `user`, team synced from the prefixed group only
    let me = whoami(&app, &cookie).await;
    assert_eq!(me["username"], "alice");
    assert_eq!(me["role"], "user");
    assert_eq!(me["teams"], serde_json::json!([{"name": "data", "role": "member"}]));
    assert_eq!(me["access"]["all"], false);

    // the same subject logs in again: same user, no duplicate
    let c2 =
        login(&app, &idp, |n| ident("sub-alice", "alice", Some(&["trunks-team-data"]), n)).await;
    assert_ne!(c2, cookie, "a new session each login");
    let users: serde_json::Value = app
        .http
        .get(format!("{}/api/v0/users", app.base))
        .bearer_auth(&app.admin_token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(users.as_array().unwrap().len(), 2, "admin + alice: {users}");

    // an open-redirect attempt falls back to "/"
    let (state, nonce) = start_login(&app, Some("https://evil.example/")).await;
    let code = idp.issue_code(ident("sub-alice", "alice", None, &nonce));
    let (status, location, _, _) = callback(&app, &code, &state).await;
    assert_eq!((status, location.as_deref()), (302, Some("/")));

    // logout closes the session server-side
    let res = app
        .http
        .post(format!("{}/auth/logout", app.base))
        .header("cookie", format!("trunks_session={cookie}"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    assert!(res.headers()["set-cookie"].to_str().unwrap().contains("Max-Age=0"));
    let res = app
        .http
        .get(format!("{}/api/v0/users/whoami", app.base))
        .header("cookie", format!("trunks_session={cookie}"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401, "the old cookie is dead");
    assert_eq!(whoami(&app, &c2).await["username"], "alice", "other sessions are unaffected");
}

#[tokio::test]
async fn claims_sync_role_and_teams_on_every_login() {
    let idp = FakeIdp::start().await;
    let app = boot(&idp, Some("trunks-admins"), true).await;

    // admin group → admin role; two teams
    let c = login(&app, &idp, |n| {
        ident("sub-bob", "bob", Some(&["trunks-admins", "trunks-team-data", "trunks-team-web"]), n)
    })
    .await;
    let me = whoami(&app, &c).await;
    assert_eq!(me["role"], "admin");
    assert_eq!(me["access"]["all"], true);
    let teams: Vec<&str> =
        me["teams"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(teams, ["data", "web"]);

    // an admin adds bob to a team by hand: that membership is not the IdP's to remove
    let res = app
        .http
        .post(format!("{}/api/v0/teams", app.base))
        .bearer_auth(&app.admin_token)
        .json(&serde_json::json!({"name": "manual", "description": ""}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let res = app
        .http
        .post(format!("{}/api/v0/teams/manual/members", app.base))
        .bearer_auth(&app.admin_token)
        .json(&serde_json::json!({"username": "bob", "role": "maintainer"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    // next login: no longer in the admin group, lost `web`, gained nothing → demoted, synced
    let c = login(&app, &idp, |n| ident("sub-bob", "bob", Some(&["trunks-team-data"]), n)).await;
    let me = whoami(&app, &c).await;
    assert_eq!(me["role"], "user", "role follows the claim both ways");
    let teams: Vec<(&str, &str)> = me["teams"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["name"].as_str().unwrap(), t["role"].as_str().unwrap()))
        .collect();
    assert_eq!(teams, [("data", "member"), ("manual", "maintainer")]);

    // no groups claim at all (neither token nor userinfo) → nothing is touched
    let c = login(&app, &idp, |n| ident("sub-bob", "bob", None, n)).await;
    assert_eq!(whoami(&app, &c).await["teams"].as_array().unwrap().len(), 2);

    // groups only in userinfo are honoured
    let c = login(&app, &idp, |n| Identity {
        userinfo_groups: Some(vec!["trunks-admins".into()]),
        ..ident("sub-bob", "bob", None, n)
    })
    .await;
    let me = whoami(&app, &c).await;
    assert_eq!(me["role"], "admin");
    let teams: Vec<&str> =
        me["teams"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(teams, ["manual"], "claim-synced `data` removed, manual kept");
}

#[tokio::test]
async fn a_rotated_signing_key_is_picked_up_without_a_restart() {
    let idp = FakeIdp::start().await;
    // the first key set the application caches lacks the signing key, and carries entries
    // it cannot use at all
    idp.serve_stale_jwks_once().await;
    let app = boot(&idp, None, false).await;

    let c = login(&app, &idp, |n| ident("sub-rot", "rot", None, n)).await;
    assert_eq!(whoami(&app, &c).await["username"], "rot");
    assert_eq!(idp.hits("/jwks").await, 2, "one refresh, triggered by the unknown key id");
}

#[tokio::test]
async fn invalid_tokens_and_states_are_rejected() {
    let idp = FakeIdp::start().await;
    let app = boot(&idp, None, false).await;

    // wrong nonce (token minted for another login)
    let (state, _nonce) = start_login(&app, None).await;
    let code = idp.issue_code(ident("sub-x", "x", None, "some-other-nonce"));
    let (status, _, cookie, body) = callback(&app, &code, &state).await;
    assert_eq!(status, 401, "{body}");
    assert!(cookie.is_none() && body.contains("Sign-in failed"));

    // wrong audience, tampered signature, expired token, symmetric algorithm
    for breakage in ["aud", "tamper", "expired", "hs256"] {
        let (state, nonce) = start_login(&app, None).await;
        let mut id = ident("sub-x", "x", None, &nonce);
        match breakage {
            "aud" => id.aud = Some("someone-else".into()),
            "tamper" => id.tamper = true,
            "expired" => id.expired = true,
            _ => id.symmetric = true,
        }
        let code = idp.issue_code(id);
        let (status, _, cookie, body) = callback(&app, &code, &state).await;
        assert_eq!(status, 401, "{breakage}: {body}");
        assert!(cookie.is_none(), "{breakage}");
    }
    assert_eq!(
        idp.hits("/jwks").await,
        2,
        "only a signature failure refreshes the JWKS, and at most once per cool-down"
    );

    // the state is single-use and must exist
    let (state, nonce) = start_login(&app, None).await;
    let code = idp.issue_code(ident("sub-y", "y", None, &nonce));
    let (status, ..) = callback(&app, &code, &state).await;
    assert_eq!(status, 302);
    let code2 = idp.issue_code(ident("sub-y", "y", None, &nonce));
    let (status, _, cookie, _) = callback(&app, &code2, &state).await;
    assert_eq!(status, 401, "replayed state");
    assert!(cookie.is_none());
    let (status, ..) = callback(&app, "whatever", "unknown-state").await;
    assert_eq!(status, 401);

    // provider-side errors and malformed callbacks render the page
    let res = app
        .http
        .get(format!("{}/auth/callback?error=access_denied&error_description=nope", app.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    assert!(res.text().await.unwrap().contains("access_denied"));
    let res = app.http.get(format!("{}/auth/callback", app.base)).send().await.unwrap();
    assert_eq!(res.status(), 400);

    // an IdP identity can never take over a local account by name (the bootstrap admin)
    let (state, nonce) = start_login(&app, None).await;
    let code = idp.issue_code(ident("sub-evil", "admin", None, &nonce));
    let (status, _, cookie, body) = callback(&app, &code, &state).await;
    assert_eq!(status, 403, "{body}");
    assert!(cookie.is_none() && body.contains("already exists"));

    // none of the failures created a user
    let users: serde_json::Value = app
        .http
        .get(format!("{}/api/v0/users", app.base))
        .bearer_auth(&app.admin_token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let names: Vec<&str> =
        users.as_array().unwrap().iter().map(|u| u["username"].as_str().unwrap()).collect();
    assert_eq!(names, ["admin", "y"]);
}

#[tokio::test]
async fn cookie_sessions_need_same_origin_for_changes_and_tokens_can_open_sessions() {
    let idp = FakeIdp::start().await;
    let app = boot(&idp, Some("trunks-admins"), false).await;
    let cookie =
        login(&app, &idp, |n| ident("sub-root", "root", Some(&["trunks-admins"]), n)).await;
    let ck = format!("trunks_session={cookie}");
    let teams = format!("{}/api/v0/teams", app.base);
    let body = serde_json::json!({"name": "t1", "description": ""});

    // reads work with the cookie alone
    assert_eq!(app.http.get(&teams).header("cookie", &ck).send().await.unwrap().status(), 200);
    // a state-changing request without Origin, or from another origin, is refused
    let res = app.http.post(&teams).header("cookie", &ck).json(&body).send().await.unwrap();
    assert_eq!(res.status(), 403);
    assert!(res.text().await.unwrap().contains("cross-origin"));
    let res = app
        .http
        .post(&teams)
        .header("cookie", &ck)
        .header("origin", "https://evil.example")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
    // same origin passes
    let res = app
        .http
        .post(&teams)
        .header("cookie", &ck)
        .header("origin", &app.base)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    // a bearer token is not subject to the origin check (not ambient authority)
    let res = app
        .http
        .post(&teams)
        .bearer_auth(&app.admin_token)
        .header("origin", "https://evil.example")
        .json(&serde_json::json!({"name": "t2", "description": ""}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);

    // a garbage cookie is a 401, not a fallback to anonymous
    let res = app.http.get(&teams).header("cookie", "trunks_session=nope").send().await.unwrap();
    assert_eq!(res.status(), 401);
    // MCP endpoints never accept the session cookie
    let res = app
        .http
        .post(format!("{}/mcp", app.base))
        .header("cookie", &ck)
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);

    // token → session (the web app's non-SSO login)
    let res = app
        .http
        .post(format!("{}/auth/token", app.base))
        .json(&serde_json::json!({"access_token": "wrong-token"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
    let res = app
        .http
        .post(format!("{}/auth/token", app.base))
        .json(&serde_json::json!({"access_token": app.admin_token}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let set_cookie = res.headers()["set-cookie"].to_str().unwrap().to_owned();
    let who: serde_json::Value = res.json().await.unwrap();
    assert_eq!(who, serde_json::json!({"username": "admin", "role": "admin"}));
    let admin_cookie = set_cookie.split(';').next().unwrap().split_once('=').unwrap().1.to_owned();
    assert_eq!(whoami(&app, &admin_cookie).await["username"], "admin");

    // deleting a user kills their sessions
    let res = app
        .http
        .delete(format!("{}/api/v0/users/root", app.base))
        .bearer_auth(&app.admin_token)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    let res = app.http.get(&teams).header("cookie", &ck).send().await.unwrap();
    assert_eq!(res.status(), 401);
}
