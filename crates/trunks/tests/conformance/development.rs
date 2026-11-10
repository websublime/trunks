//! Development mode: no authentication, everything visible. Covers registration, listings,
//! invocation over CLI, REST and MCP, enable/disable, tool groups, persistence across a
//! restart and deregistration.

use reqwest::Method;
use rmcp::model::{CallToolRequestParams, GetPromptRequestParams, ReadResourceRequestParams};
use serde_json::json;

use crate::gateway::{Backend, Gateway, Mode, text_of, tool_names};

const LOCAL_RESOURCE: &str = "trunks://res/local/bWVtOi8vaGVsbG8";
const ALL_TOOLS: [&str; 6] = [
    "local__echo",
    "local__echo_headers",
    "local__fail",
    "web__echo",
    "web__echo_headers",
    "web__fail",
];

/// A call to `tool`, passing `text` when there is one.
fn call(tool: &str, text: Option<&str>) -> CallToolRequestParams {
    let params = CallToolRequestParams::new(tool.to_owned());
    match text {
        Some(text) => params.with_arguments(json!({"text": text}).as_object().unwrap().clone()),
        None => params,
    }
}

pub async fn scenario(backend: Backend) {
    // First, because it may have to build the binary, which blocks for a while.
    let fake = trunks_testing::fake_upstream_bin().to_string_lossy().into_owned();
    let Some(mut gw) = Gateway::start(backend, Mode::Development).await else { return };
    let web = trunks_testing::serve_http().await;

    // ── registration ─────────────────────────────────────────────────────────────────
    let out = gw
        .cli(&["register", "--name", "local", "--transport", "stdio", "--command", &fake])
        .await
        .success();
    assert!(out.contains("Server local registered successfully!"), "{out}");
    assert!(out.contains("Tools (3):") && out.contains("Prompts (1):"), "{out}");
    assert!(out.contains(LOCAL_RESOURCE), "{out}");
    gw.cli(&["register", "--name", "web", "--transport", "streamable-http", "--url", &web])
        .await
        .success();
    let err = gw
        .cli(&["register", "--name", "local", "--transport", "stdio", "--command", &fake])
        .await
        .failure();
    assert!(err.contains("server 'local' already exists; use force to replace it"), "{err}");

    // ── listings ─────────────────────────────────────────────────────────────────────
    let servers = gw.cli(&["list", "servers"]).await.success();
    for expected in ["local", "stdio", "web", "streamable_http", "enabled", "stateless"] {
        assert!(servers.contains(expected), "`{expected}` missing from:\n{servers}");
    }
    let tools = gw.cli(&["list", "tools"]).await.success();
    for tool in ALL_TOOLS {
        assert!(tools.contains(tool), "`{tool}` missing from:\n{tools}");
    }
    let prompts = gw.cli(&["list", "prompts"]).await.success();
    assert!(prompts.contains("local__greet") && prompts.contains("web__greet"), "{prompts}");
    let resources = gw.cli(&["list", "resources"]).await.success();
    assert!(resources.contains(LOCAL_RESOURCE), "{resources}");
    let (status, listed) = gw.rest(Method::GET, "/api/v0/tools", None, None).await;
    assert_eq!(status, 200);
    assert_eq!(listed.as_array().map(Vec::len), Some(6), "{listed}");

    // ── invocation: CLI, REST and MCP agree ──────────────────────────────────────────
    let out = gw.cli(&["invoke", "local__echo", "--input", r#"{"text":"over the cli"}"#]).await;
    assert!(out.success().contains("over the cli"));
    let out = gw.cli(&["invoke", "local__fail"]).await.success();
    assert!(out.contains("The tool returned an error:") && out.contains("boom"), "{out}");
    let err = gw.cli(&["invoke", "nope__x"]).await.failure();
    assert!(err.contains("tool 'nope__x' not found"), "{err}");
    let (status, body) = gw
        .rest(
            Method::POST,
            "/api/v0/tools/invoke",
            None,
            Some(json!({"name": "web__echo", "text": "over rest"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["content"][0]["text"], "over rest");
    assert_eq!(body["isError"], false);
    let out = gw.cli(&["get", "prompt", "local__greet"]).await.success();
    assert!(out.contains("Hello, world!"), "{out}");
    let out = gw.cli(&["get", "resource", LOCAL_RESOURCE, "--read"]).await.success();
    assert!(out.contains("hello from upstream"), "{out}");

    let mcp = gw.mcp("/mcp", None).await.unwrap();
    assert_eq!(tool_names(&mcp).await, ALL_TOOLS);
    for tool in ["local__echo", "web__echo"] {
        let result = mcp.call_tool(call(tool, Some("over mcp"))).await.unwrap();
        assert_eq!(text_of(&result), "over mcp", "{tool}");
    }
    let failed = mcp.call_tool(call("web__fail", None)).await.unwrap();
    assert_eq!(failed.is_error, Some(true));
    let unknown = mcp.call_tool(call("nope__x", None)).await.unwrap_err();
    assert!(unknown.to_string().contains("tool 'nope__x' not found"), "{unknown}");
    assert_eq!(mcp.list_all_resources().await.unwrap().len(), 2);
    let read = mcp.read_resource(ReadResourceRequestParams::new(LOCAL_RESOURCE)).await.unwrap();
    assert!(format!("{read:?}").contains("hello from upstream"), "{read:?}");
    let prompt = mcp.get_prompt(GetPromptRequestParams::new("web__greet")).await.unwrap();
    assert!(format!("{prompt:?}").contains("Hello, world!"), "{prompt:?}");

    // ── enable / disable ─────────────────────────────────────────────────────────────
    let out = gw.cli(&["disable", "tool", "local__fail"]).await.success();
    assert!(out.contains("Tools disabled (1):") && out.contains("local__fail"), "{out}");
    let err = gw.cli(&["invoke", "local__fail"]).await.failure();
    assert!(err.contains("tool 'local__fail' not found"), "{err}");
    assert!(!tool_names(&mcp).await.contains(&"local__fail".to_owned()));
    let out = gw.cli(&["disable", "server", "web"]).await.success();
    assert!(out.contains("Server web disabled.") && out.contains("Tools affected (3):"), "{out}");
    assert_eq!(tool_names(&mcp).await, ["local__echo", "local__echo_headers"]);
    gw.cli(&["enable", "server", "web"]).await.success();
    assert_eq!(tool_names(&mcp).await.len(), 5, "everything but the disabled tool is back");

    // ── tool groups ──────────────────────────────────────────────────────────────────
    let conf = gw.path("group.json");
    std::fs::write(
        &conf,
        json!({"name": "readers", "description": "read only", "included_tools": ["local__echo"]})
            .to_string(),
    )
    .unwrap();
    let out = gw.cli(&["create", "group", "-c", &conf.to_string_lossy()]).await.success();
    assert!(out.contains("Tool group readers created."), "{out}");
    assert!(
        out.contains("/v0/groups/readers/mcp") && out.contains("effective tools (1):"),
        "{out}"
    );
    let group = gw.mcp("/v0/groups/readers/mcp", None).await.unwrap();
    assert_eq!(tool_names(&group).await, ["local__echo"]);
    let denied = group.call_tool(call("web__echo", None)).await.unwrap_err();
    assert!(denied.to_string().contains("not found"), "{denied}");
    let err = gw.cli(&["invoke", "web__echo", "--group", "readers"]).await.failure();
    assert!(err.contains("tool 'web__echo' is not an effective tool of group 'readers'"), "{err}");
    assert!(gw.mcp("/v0/groups/missing/mcp", None).await.is_err(), "unknown groups do not exist");

    // ── everything survives a restart ────────────────────────────────────────────────
    let _ = mcp.cancel().await;
    let _ = group.cancel().await;
    gw.restart().await;
    let tools = gw.cli(&["list", "tools"]).await.success();
    for tool in ALL_TOOLS {
        assert!(tools.contains(tool), "`{tool}` lost in the restart:\n{tools}");
    }
    assert!(
        tools.lines().any(|l| l.contains("local__fail") && l.contains("disabled")),
        "the disabled flag was lost in the restart:\n{tools}"
    );
    let mcp = gw.mcp("/mcp", None).await.unwrap();
    assert_eq!(tool_names(&mcp).await.len(), 5);
    for tool in ["local__echo", "web__echo"] {
        let result = mcp.call_tool(call(tool, Some("after restart"))).await.unwrap();
        assert_eq!(text_of(&result), "after restart", "{tool}");
    }
    let group = gw.mcp("/v0/groups/readers/mcp", None).await.unwrap();
    assert_eq!(tool_names(&group).await, ["local__echo"]);
    let out = gw.cli(&["get", "group", "readers"]).await.success();
    assert!(out.contains("read only") && out.contains("local__echo"), "{out}");

    // ── deregistration ───────────────────────────────────────────────────────────────
    let out = gw.cli(&["deregister", "web"]).await.success();
    assert!(out.contains("Server web deregistered successfully!"), "{out}");
    assert_eq!(tool_names(&mcp).await, ["local__echo", "local__echo_headers"]);
    let servers = gw.cli(&["list", "servers"]).await.success();
    assert!(!servers.contains("streamable_http"), "{servers}");

    let _ = mcp.cancel().await;
    let _ = group.cancel().await;
    gw.finish().await;
}
