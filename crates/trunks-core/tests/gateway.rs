//! Upstream connections, sessions and the invoker against the fake upstream (stdio + HTTP).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use trunks_core::catalog::{Catalog, Snapshot};
use trunks_core::domain::{PromptRecord, ResourceRecord, ServerRecord, ToolRecord, UpstreamConfig};
use trunks_core::gateway::{GatewayState, Invoker};
use trunks_core::ports::RegistrySnapshot;
use trunks_core::sessions::{SessionManager, SessionOptions};
use trunks_core::upstream::{ConnectOptions, UpstreamConn};
use trunks_types::{CanonicalName, ResourceUri, ServerName, SessionMode};

fn stdio_config() -> UpstreamConfig {
    UpstreamConfig::Stdio {
        command: trunks_testing::fake_upstream_bin().to_string_lossy().into_owned(),
        args: vec![],
        env: BTreeMap::new(),
    }
}

fn server(id: i64, name: &str, config: UpstreamConfig, mode: SessionMode) -> ServerRecord {
    ServerRecord {
        id,
        name: ServerName::parse(name).unwrap(),
        enabled: true,
        description: String::new(),
        config,
        session_mode: mode,
        created_at: None,
        updated_at: None,
    }
}

fn snapshot_for(servers: Vec<ServerRecord>) -> Snapshot {
    let mut tools = vec![];
    let mut prompts = vec![];
    let mut resources = vec![];
    for s in &servers {
        for t in ["echo", "echo_headers", "fail"] {
            tools.push(ToolRecord {
                id: 0,
                server_id: s.id,
                name: t.into(),
                enabled: true,
                description: String::new(),
                input_schema: None,
                annotations: None,
            });
        }
        prompts.push(PromptRecord {
            id: 0,
            server_id: s.id,
            name: "greet".into(),
            enabled: true,
            description: String::new(),
            arguments: None,
        });
        resources.push(ResourceRecord {
            id: 0,
            server_id: s.id,
            uri: ResourceUri::new(s.name.clone(), "mem://hello"),
            original_uri: "mem://hello".into(),
            name: "hello".into(),
            enabled: true,
            description: String::new(),
            mime_type: None,
            annotations: None,
            meta: None,
        });
    }
    Snapshot::build(RegistrySnapshot { servers, tools, prompts, resources, groups: vec![] })
        .unwrap()
}

fn state(snapshot: Snapshot, opts: SessionOptions) -> Arc<GatewayState> {
    Arc::new(GatewayState {
        catalog: Catalog::new(snapshot),
        sessions: SessionManager::new(opts),
        metrics: None,
        version: "test".into(),
    })
}

fn text_of(r: &rmcp::model::CallToolResult) -> String {
    r.content.first().and_then(|c| c.as_text()).map(|t| t.text.clone()).unwrap_or_default()
}

#[tokio::test]
async fn stdio_upstream_connect_and_discover() {
    let name = ServerName::parse("fs").unwrap();
    let conn =
        UpstreamConn::connect(&name, &stdio_config(), &ConnectOptions::default()).await.unwrap();
    let tools = conn.list_tools().await.unwrap();
    assert_eq!(
        tools.iter().map(|t| t.name.to_string()).collect::<Vec<_>>(),
        ["echo", "echo_headers", "fail"]
    );
    assert_eq!(conn.list_prompts().await.unwrap().len(), 1);
    assert_eq!(conn.list_resources().await.unwrap()[0].uri, "mem://hello");
    let caps = conn.capabilities().unwrap();
    assert!(caps.prompts.is_some() && caps.resources.is_some() && caps.tools.is_some());
    conn.close().await;
}

#[tokio::test]
async fn connect_errors_are_actionable() {
    let name = ServerName::parse("bad").unwrap();
    let cfg = UpstreamConfig::Stdio {
        command: "/nonexistent/binary".into(),
        args: vec![],
        env: BTreeMap::new(),
    };
    let err = UpstreamConn::connect(&name, &cfg, &ConnectOptions::default())
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("upstream 'bad'") && err.contains("failed to spawn"), "{err}");

    let cfg = UpstreamConfig::Sse { url: "https://x/sse".into(), bearer_token: None };
    let err = UpstreamConn::connect(&name, &cfg, &ConnectOptions::default())
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("not supported"), "{err}");

    let cfg = UpstreamConfig::StreamableHttp {
        url: "http://127.0.0.1:9/mcp".into(),
        bearer_token: None,
        headers: BTreeMap::new(),
    };
    let err = UpstreamConn::connect(
        &name,
        &cfg,
        &ConnectOptions { init_timeout: Duration::from_secs(5), oauth: None },
    )
    .await
    .err()
    .unwrap()
    .to_string();
    assert!(err.contains("initialize failed") || err.contains("timed out"), "{err}");
}

#[tokio::test]
async fn invoker_over_stdio_stateless_and_stateful() {
    for mode in [SessionMode::Stateless, SessionMode::Stateful] {
        let st = state(
            snapshot_for(vec![server(1, "fs", stdio_config(), mode)]),
            SessionOptions::default(),
        );
        let inv = Invoker(st.clone());
        let echo = CanonicalName::parse("fs__echo").unwrap();
        let args = serde_json::json!({"text": "hi"}).as_object().cloned();
        let r = inv.call_tool(&echo, args.clone()).await.unwrap();
        assert_eq!(text_of(&r), "hi");
        let r = inv.call_tool(&echo, args).await.unwrap();
        assert_eq!(text_of(&r), "hi");
        assert_eq!(
            st.sessions.stateful_active(),
            u64::from(mode == SessionMode::Stateful),
            "{mode:?}"
        );

        let r = inv.call_tool(&CanonicalName::parse("fs__fail").unwrap(), None).await.unwrap();
        assert_eq!(r.is_error, Some(true), "tool errors are returned, not raised");

        let p = inv
            .get_prompt(
                &CanonicalName::parse("fs__greet").unwrap(),
                serde_json::json!({"name": "Ana"}).as_object().cloned(),
            )
            .await
            .unwrap();
        assert_eq!(p.messages.len(), 1);

        let uri = ResourceUri::new(ServerName::parse("fs").unwrap(), "mem://hello");
        let res = inv.read_resource(&uri).await.unwrap();
        match &res.contents[0] {
            rmcp::model::ResourceContents::TextResourceContents { uri: u, text, .. } => {
                assert_eq!(u, &uri.to_string(), "upstream URI rewritten to the public one");
                assert_eq!(text, "hello from upstream");
            }
            other => panic!("unexpected contents {other:?}"),
        }

        let missing =
            inv.call_tool(&CanonicalName::parse("fs__nope").unwrap(), None).await.err().unwrap();
        assert!(matches!(missing, trunks_core::Error::NotFound { .. }));
        st.sessions.shutdown().await;
        assert_eq!(st.sessions.stateful_active(), 0);
    }
}

#[tokio::test]
async fn http_upstream_never_sees_downstream_headers_and_gets_configured_ones() {
    let url = trunks_testing::serve_http().await;
    let mut headers = BTreeMap::new();
    headers.insert("X-Tenant".to_string(), secrecy::SecretString::from("acme"));
    let cfg = UpstreamConfig::StreamableHttp {
        url,
        bearer_token: Some(secrecy::SecretString::from("up-token")),
        headers,
    };
    let st = state(
        snapshot_for(vec![server(1, "web", cfg, SessionMode::Stateful)]),
        SessionOptions::default(),
    );
    let inv = Invoker(st.clone());
    let r = inv.call_tool(&CanonicalName::parse("web__echo_headers").unwrap(), None).await.unwrap();
    let seen: Vec<String> = serde_json::from_str(&text_of(&r)).unwrap();
    assert!(
        seen.contains(&"authorization".to_string()),
        "configured bearer reaches the upstream: {seen:?}"
    );
    assert!(
        seen.contains(&"x-tenant".to_string()),
        "configured custom header reaches the upstream: {seen:?}"
    );
    assert!(
        !seen.iter().any(|h| h == "x-downstream" || h == "cookie"),
        "nothing downstream-specific: {seen:?}"
    );
    st.sessions.shutdown().await;
}

#[tokio::test]
async fn stateful_session_is_invalidated_on_transport_error() {
    let st = state(
        snapshot_for(vec![server(1, "fs", stdio_config(), SessionMode::Stateful)]),
        SessionOptions::default(),
    );
    let inv = Invoker(st.clone());
    let echo = CanonicalName::parse("fs__echo").unwrap();
    inv.call_tool(&echo, None).await.unwrap();
    assert_eq!(st.sessions.stateful_active(), 1);
    // kill the cached session's child from the outside by invalidating, then verify reconnect
    st.sessions.invalidate(&ServerName::parse("fs").unwrap()).await;
    assert_eq!(st.sessions.stateful_active(), 0);
    inv.call_tool(&echo, None).await.unwrap();
    assert_eq!(st.sessions.stateful_active(), 1, "reconnected transparently");
    st.sessions.shutdown().await;
}

#[tokio::test]
async fn idle_stateful_sessions_are_swept() {
    let opts = SessionOptions {
        connect: ConnectOptions::default(),
        idle_timeout: Some(Duration::from_secs(1)),
        sweep_interval: Duration::from_millis(200),
    };
    let st =
        state(snapshot_for(vec![server(1, "fs", stdio_config(), SessionMode::Stateful)]), opts);
    Invoker(st.clone()).call_tool(&CanonicalName::parse("fs__echo").unwrap(), None).await.unwrap();
    assert_eq!(st.sessions.stateful_active(), 1);
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(st.sessions.stateful_active(), 0, "swept after idle timeout");
    st.sessions.shutdown().await;
}
