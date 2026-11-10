//! Enterprise mode: initialization, tokens, teams and grants, MCP client allow-lists and
//! ownership, browser sessions, persistence across a restart and deletion cascades.

use reqwest::Method;
use rmcp::model::CallToolRequestParams;
use serde_json::{Value, json};

use crate::gateway::{Backend, Gateway, Mode, text_of, tool_names};

const ANA: &str = "ana-token-0123456789";
const IDE: &str = "ide-token-0123456789";
const ANA_IDE: &str = "ana-ide-token-0123456789";
const LOCAL_TOOLS: [&str; 3] = ["local__echo", "local__echo_headers", "local__fail"];

/// The value printed after `label` on its own line of a CLI output.
fn printed(output: &str, label: &str) -> String {
    output
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .unwrap_or_else(|| panic!("`{label}` not found in:\n{output}"))
        .trim()
        .to_owned()
}

/// The canonical names in a `GET /api/v0/tools` answer, sorted.
fn names(tools: &Value) -> Vec<String> {
    let mut names: Vec<String> = tools
        .as_array()
        .unwrap_or_else(|| panic!("not a list: {tools}"))
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

async fn visible_tools(gw: &Gateway, token: &str) -> Vec<String> {
    let (status, tools) = gw.rest(Method::GET, "/api/v0/tools", Some(token), None).await;
    assert_eq!(status, 200, "{tools}");
    names(&tools)
}

pub async fn scenario(backend: Backend) {
    let Some(mut gw) = Gateway::start(backend, Mode::Enterprise).await else { return };
    let fake = trunks_testing::fake_upstream_bin().to_string_lossy().into_owned();

    // ── initialization ───────────────────────────────────────────────────────────────
    let (status, body) = gw.rest(Method::GET, "/api/v0/servers", None, None).await;
    assert_eq!((status, body["error"].as_str()), (403, Some("server is not initialized")));
    let err = gw.cli(&["list", "servers"]).await.failure();
    assert!(err.contains("server is not initialized"), "{err}");
    let out = gw.cli(&["init-server"]).await.success();
    assert!(out.contains("Server initialized successfully"), "{out}");
    let admin = printed(&out, "Admin access token:");
    let err = gw.cli(&["init-server"]).await.failure();
    assert!(err.contains("Server is already initialized"), "{err}");

    // ── nothing without a token ──────────────────────────────────────────────────────
    let (status, body) = gw.rest(Method::GET, "/api/v0/servers", None, None).await;
    assert_eq!((status, body["error"].as_str()), (401, Some("missing access token")));
    let (status, _) = gw.rest(Method::GET, "/api/v0/servers", Some("not-a-real-token"), None).await;
    assert_eq!(status, 401);
    let refused = gw.mcp("/mcp", None).await.err().unwrap();
    assert!(
        refused.contains("401") && refused.contains("missing MCP client access token"),
        "{refused}"
    );

    // ── the admin sets things up (the CLI stored the admin token at init) ────────────
    for server in ["local", "other"] {
        gw.cli(&["register", "--name", server, "--transport", "stdio", "--command", &fake])
            .await
            .success();
    }
    let out = gw.cli(&["create", "user", "ana", "--access-token", ANA]).await.success();
    assert!(out.contains("User ana created.") && out.contains(ANA), "{out}");
    let rui = printed(&gw.cli(&["create", "user", "rui"]).await.success(), "Access token:");
    assert!(rui.len() >= 32, "a generated token: {rui}");
    let users = gw.cli(&["list", "users"]).await.success();
    for expected in ["admin", "ana", "rui"] {
        assert!(users.contains(expected), "{users}");
    }
    assert!(!users.contains(ANA) && !users.contains(&rui), "tokens are never listed:\n{users}");

    // ── access: nothing by default, then through a team and through a direct grant ───
    assert!(visible_tools(&gw, ANA).await.is_empty(), "no grant, no tools");
    assert_eq!(visible_tools(&gw, &admin).await.len(), 6, "admins see everything");
    gw.cli(&["create", "team", "platform"]).await.success();
    let out = gw.cli(&["add-team-member", "platform", "ana"]).await.success();
    assert!(out.contains("ana is now a member of team platform."), "{out}");
    let out = gw.cli(&["grant", "--team", "platform", "--server", "local"]).await.success();
    assert!(out.contains("Granted: team platform → server local"), "{out}");
    assert_eq!(visible_tools(&gw, ANA).await, LOCAL_TOOLS);
    let (status, me) = gw.rest(Method::GET, "/api/v0/users/whoami", Some(ANA), None).await;
    assert_eq!(status, 200);
    assert_eq!(me["username"], "ana");
    assert_eq!(me["role"], "user");
    assert_eq!(me["teams"], json!([{"name": "platform", "role": "member"}]));
    assert_eq!(me["access"]["servers"], json!(["local"]));

    let invoke = |token: &'static str, tool: &'static str| {
        gw.rest(
            Method::POST,
            "/api/v0/tools/invoke",
            Some(token),
            Some(json!({"name": tool, "text": "hello"})),
        )
    };
    let (status, body) = invoke(ANA, "local__echo").await;
    assert_eq!((status, body["content"][0]["text"].as_str()), (200, Some("hello")));
    let (status, body) = invoke(ANA, "other__echo").await;
    assert_eq!(status, 404, "a tool outside the grants reads as missing: {body}");
    assert_eq!(body["error"], "tool 'other__echo' not found");

    gw.cli(&["grant", "--user", "rui", "--server", "other"]).await.success();
    assert_eq!(visible_tools(&gw, &rui).await.len(), 3);
    assert!(visible_tools(&gw, &rui).await.iter().all(|t| t.starts_with("other__")));
    gw.cli(&["revoke", "--user", "rui", "--server", "other"]).await.success();
    assert!(visible_tools(&gw, &rui).await.is_empty(), "a revoked grant stops working");
    let grants = gw.cli(&["list", "grants"]).await.success();
    assert!(grants.contains("team platform → server local") && !grants.contains("rui"), "{grants}");

    // ── users are not administrators ─────────────────────────────────────────────────
    let (status, body) = gw.rest(Method::GET, "/api/v0/users", Some(ANA), None).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], "user is not authorized to perform this action");
    let (status, _) = gw
        .rest(
            Method::POST,
            "/api/v0/servers",
            Some(ANA),
            Some(json!({"name": "mine", "transport": "stdio", "command": fake})),
        )
        .await;
    assert_eq!(status, 403);

    // ── MCP clients: allow-list, and never more than their owner ─────────────────────
    let out = gw
        .cli(&["create", "mcp-client", "ide", "--allow", "local", "--access-token", IDE])
        .await
        .success();
    assert!(
        out.contains("MCP client ide created.") && out.contains("Allowed servers: local"),
        "{out}"
    );
    gw.cli(&[
        "create",
        "mcp-client",
        "ana-ide",
        "--allow",
        "*",
        "--owner",
        "user:ana",
        "--access-token",
        ANA_IDE,
    ])
    .await
    .success();
    for (client, token) in [("ide", IDE), ("ana-ide", ANA_IDE)] {
        let mcp = gw.mcp("/mcp", Some(token)).await.unwrap();
        assert_eq!(tool_names(&mcp).await, LOCAL_TOOLS, "{client}");
        let echoed = mcp
            .call_tool(
                CallToolRequestParams::new("local__echo")
                    .with_arguments(json!({"text": client}).as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_eq!(text_of(&echoed), client);
        let denied = mcp.call_tool(CallToolRequestParams::new("other__echo")).await.unwrap_err();
        assert!(
            denied.to_string().contains("not authorized to access MCP server other"),
            "{client}: {denied}"
        );
        let _ = mcp.cancel().await;
    }
    let refused = gw.mcp("/mcp", Some(ANA)).await.err().unwrap();
    assert!(
        refused.contains("invalid MCP client token"),
        "user tokens are not MCP tokens: {refused}"
    );

    // ── browser session: a cookie instead of the token ───────────────────────────────
    let res = gw
        .http
        .post(format!("{}/auth/token", gw.base))
        .json(&json!({"access_token": ANA}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let set_cookie = res.headers()["set-cookie"].to_str().unwrap().to_owned();
    assert!(set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Lax"), "{set_cookie}");
    let cookie = set_cookie.split(';').next().unwrap().to_owned();
    let with_cookie = |method: Method, path: &'static str| {
        gw.http.request(method, format!("{}{path}", gw.base)).header("cookie", &cookie)
    };
    let res = with_cookie(Method::GET, "/api/v0/users/whoami").send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.json::<Value>().await.unwrap()["username"], "ana");
    // a change made with the cookie must come from the same origin
    let res = with_cookie(Method::POST, "/api/v0/tools/invoke")
        .header("origin", "https://evil.example")
        .json(&json!({"name": "local__echo"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
    let res = with_cookie(Method::POST, "/api/v0/tools/invoke")
        .header("origin", &gw.base)
        .json(&json!({"name": "local__echo", "text": "same origin"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    // and the MCP endpoints never take cookies
    let res = with_cookie(Method::POST, "/mcp")
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);

    // ── everything survives a restart ────────────────────────────────────────────────
    gw.restart().await;
    assert_eq!(visible_tools(&gw, ANA).await, LOCAL_TOOLS, "grants and memberships persisted");
    assert_eq!(visible_tools(&gw, &admin).await.len(), 6);
    let res = gw
        .http
        .get(format!("{}/api/v0/users/whoami", gw.base))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "the browser session persisted");
    let mcp = gw.mcp("/mcp", Some(ANA_IDE)).await.unwrap();
    assert_eq!(tool_names(&mcp).await, LOCAL_TOOLS, "the owned client persisted");
    let _ = mcp.cancel().await;

    // ── logging out and deleting ─────────────────────────────────────────────────────
    let res = gw
        .http
        .post(format!("{}/auth/logout", gw.base))
        .header("cookie", &cookie)
        .header("origin", &gw.base)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    let res = gw
        .http
        .get(format!("{}/api/v0/users/whoami", gw.base))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401, "a logged-out session is gone");

    let err = gw.cli(&["delete", "user", "admin"]).await.failure();
    assert!(err.contains("the last administrator cannot be deleted"), "{err}");
    let out = gw.cli(&["delete", "user", "ana"]).await.success();
    assert!(out.contains("User ana deleted."), "{out}");
    let (status, _) = gw.rest(Method::GET, "/api/v0/users/whoami", Some(ANA), None).await;
    assert_eq!(status, 401, "a deleted user's token stops working");
    let refused = gw.mcp("/mcp", Some(ANA_IDE)).await.err().unwrap();
    assert!(refused.contains("401"), "a deleted user's clients go with her: {refused}");
    let mcp = gw.mcp("/mcp", Some(IDE)).await.unwrap();
    assert_eq!(tool_names(&mcp).await, LOCAL_TOOLS, "other clients are untouched");
    let _ = mcp.cancel().await;
    let teams = gw.cli(&["list", "teams"]).await.success();
    assert!(teams.contains("platform") && !teams.contains("ana"), "{teams}");

    gw.finish().await;
}
