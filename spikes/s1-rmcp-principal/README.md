# Spike S1 — per-request `Principal` through `rmcp`'s Streamable HTTP server

Question: can a `GatewayHandler` behind `rmcp::StreamableHttpService` read a `Principal` that
axum middleware attached to the HTTP request — per request, in both `legacy_session_mode`
(stateful sessions) and stateless mode?

Answer: **yes, directly.** `rmcp` injects `http::request::Parts` into
`RequestContext::extensions` for every request, and axum extensions travel inside
`Parts.extensions`. Result: `docs/spikes/S1-rmcp-principal.md`.

```bash
cd spikes/s1-rmcp-principal
cargo run                                   # 19 checks
MCP_REMOTE_BIN=$(npm root)/.bin/mcp-remote cargo run   # +2 checks through the Node bridge
```

Not part of the workspace (see `exclude` in the root `Cargo.toml`); it pins its own deps.
