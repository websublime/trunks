# S1 — `Principal` por pedido através do `rmcp` Streamable HTTP server

> 2026-11-04 · `rmcp` 3.8.0 · axum 0.8 · código em `spikes/s1-rmcp-principal/`

## Pergunta
A arquitectura (`04-arquitectura.md` A3) assume que o `GatewayHandler` recebe um `Principal` tipado
produzido pelo middleware axum, **por pedido**, mesmo quando o `rmcp` mantém sessões downstream
(`legacy_session_mode = true`). Se o `rmcp` só expusesse o contexto da sessão, o plano B seria um
`StreamableHttpService` por principal.

## Resultado: confirmado, sem plano B
`StreamableHttpService` injecta `http::request::Parts` em `RequestContext::extensions` em **todos**
os pedidos (POST, GET stream, DELETE); as extensions do axum viajam dentro de `Parts.extensions`.
No handler:

```rust
fn principal(ctx: &RequestContext<RoleServer>) -> Option<Principal> {
    ctx.extensions.get::<http::request::Parts>()?.extensions.get::<Principal>().cloned()
}
```

Checks (todos verdes), em enterprise com `legacy_session_mode` **true e false**, mais dev mode:

| Check | Resultado |
|---|---|
| `initialize` sem token → 401 antes do handler | ok |
| cliente `alpha` (allow `fs`) vê só `fs__read`; chama `fs__read`; negado em `gh__search` | ok |
| **token `root` na sessão iniciada por `alpha` vê 2 tools e chama `gh__search` como `root`** — o principal segue o pedido, não a sessão | ok |
| token inválido numa sessão existente → 401 | ok |
| dev mode sem token lista tudo | ok |
| cliente `rmcp` real (`StreamableHttpClientTransport`) com `Authorization` por defeito | ok |
| `mcp-remote` 0.16 (bridge Node usada pelo Claude Desktop) → stdio → gateway | ok |

## Consequências para a implementação
1. `trunks-api` monta `/mcp` com `Router::nest_service` + `middleware::from_fn(mcp_client_auth)`; o
   middleware insere `Principal` nas extensions e devolve 401 ele próprio (o `rmcp` nunca vê pedidos
   não autenticados).
2. `GatewayHandler::principal(ctx)` devolve `Option<Principal>`; `None` ⇒ fail-closed (lista vazia /
   erro `-32603 not authorized`).
3. `get_info` usa `ServerConfig` (o alias `ServerInfo` está deprecado em 3.8); `CallToolResponse::Complete(CallToolResult::success(vec![ContentBlock::text(..)]))`.
4. `StreamableHttpServerConfig` e `ServerConfig` são `#[non_exhaustive]`: construir com `Default::default()` e atribuir campos.
5. **`rmcp` 3.8 exige `reqwest` 0.13** (feature `rustls`, não `rustls-tls`); workspace alinhado.
6. `legacy_session_mode = true` (default) serve clientes pré-2026-07-28 com `Mcp-Session-Id`; o
   modo stateless não devolve session id. Ambos funcionam com o mesmo handler.
