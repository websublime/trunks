# Trunks — Especificação Técnica

> Especificação técnica de Trunks. Versão 2.0 (2026-11-04).
> Pressupõe as decisões D1–D12 de `01-produto.md`. D11 fechada: `sqlx` sem ORM (`06-analise-db.md`).
> Compatibilidade de **dados** com deployments legacy (D3) é tratada por migração no arranque; nomes (env vars, ficheiros, métricas, esquemas de URI) são exclusivamente os de Trunks.

---

## 1. Visão geral da arquitectura

```
┌──────────────┐   HTTP/REST    ┌─────────────────────────────────────────────────────┐
│ trunks CLI    │ ─────────────▶ │  trunks start (um processo tokio)                    │
│ dashboard UI │                │                                                     │
└──────────────┘                │  axum Router                                        │
                                │   ├─ /health /metadata /init /metrics               │
┌──────────────┐  Streamable    │   ├─ /api/v0/**        (REST, auth user)            │
│ Claude/Cursor│ ───HTTP/MCP──▶ │   ├─ /api/dashboard/** (dev only)                   │
│ agentes      │                │   ├─ /mcp              (auth mcp-client) ─┐         │
└──────────────┘                │   └─ /v0/groups/:g/mcp (auth mcp-client) ─┤         │
                                │                                           ▼         │
                                │                 rmcp StreamableHttpService          │
                                │                        GatewayHandler{scope}        │
                                │                             │                       │
                                │   ┌─────────────────────────┼─────────────────┐     │
                                │   │ Catalog (ArcSwap)       │ SessionManager  │     │
                                │   │ tools/prompts/resources │ stateful conns  │     │
                                │   └────────────┬────────────┴────────┬────────┘     │
                                │                │ Registry service    │ UpstreamConn │
                                │                ▼                     ▼              │
                                │     trunks-db (SQLite|Postgres)      rmcp client    │
                                └─────────────────────────────────────┬───────────────┘
                                                                      │ stdio / streamable_http
                                                                      ▼
                                                              upstream MCP servers
```

Escolhas estruturais que definem a implementação:

| Área | Escolha |
|---|---|
| Gateway | **1 `GatewayHandler`** parametrizado por `Scope::Global \| Scope::Group(name)`; `list_tools`/`call_tool` consultam o `Catalog` e filtram. Grupos são **vistas** sobre o catálogo, não servidores MCP separados. |
| Catálogo | `ArcSwap<CatalogSnapshot>` em memória, reconstruído após cada mutação do registry; a DB não é consultada por chamada. |
| Autorização | `Principal` tipado injectado por middleware; o handler **não compila** sem ele. |
| Sessões | `DashMap<ServerName, Arc<Mutex<Slot>>>` — lock por servidor, nunca global. |
| Endpoints MCP | Um `StreamableHttpService` por scope, criado on-demand e cacheado. |

## 2. Workspace Cargo

```
trunks/
├── Cargo.toml                 # [workspace], resolver = "3", lints partilhados
├── crates/
│   ├── trunks-types/           # contratos públicos: tipos REST (serde), enums, erros de API
│   ├── trunks-core/            # domínio: Registry, Catalog, Gateway, Sessions, OAuth, Auth
│   ├── trunks-db/              # acesso a dados (SQL explícito, sem ORM) + migrações + repositórios
│   ├── trunks-api/             # axum: router, middleware, handlers REST/dashboard, montagem MCP
│   ├── trunks-client/          # SDK HTTP para a REST API (usado pelo CLI; publicável)
│   ├── trunks/             # clap: comandos; `start` liga tudo. Produz o binário.
│   └── trunks-dashboard/       # rust-embed do bundle da web app
├── web/app/                   # web app em Lit + line://ui (D2)
├── tests/conformance/         # testes black-box contra o binário (ambos os modos, ambas as DBs)
└── xtask/                     # build do dashboard, geração de docs, release helpers
```

Regras de dependência: `types ← core ← {db, api}`; `api ← cli`; `client ← cli`. `trunks-core` **não** depende de axum nem da lib de DB directamente — recebe traits (`RegistryStore`, `IdentityStore`, `OAuthStore`) que `trunks-db` implementa. Isto permite testar o domínio com stores em memória, como os testes Go fazem com SQLite `:memory:`.

Nome do binário: `trunks` (D1).

## 3. Stack

| Área | Crate | Versão mín. | Notas |
|---|---|---|---|
| Runtime | `tokio` | 1.4x | `rt-multi-thread`, `signal`, `process` |
| HTTP server | `axum` | 0.8 | + `tower`, `tower-http` (`trace`, `cors` desligado por defeito, `limit`) |
| MCP | `rmcp` | 3.9 | features: `server`, `client`, `transport-streamable-http-server`, `transport-streamable-http-client-reqwest`, `transport-child-process`, `auth`, `schemars` |
| HTTP client | `reqwest` | 0.13 | `rustls`, `json`, `stream` (versão exigida pelo `rmcp` 3.9) |
| DB | `sqlx` | 0.9 | `runtime-tokio`, `tls-rustls`, `sqlite` (bundled), `postgres`, `json`, `chrono`, `migrate`, `macros`. Pools nativos atrás de `enum Db`; SQL escrito uma vez com `$N`, sempre `&'static str` (o 0.9 recusa SQL construído em runtime); sem `query!` compile-time. Ver `06-analise-db.md` §5 |
| CLI | `clap` | 4 | derive, `env` |
| Serialização | `serde`, `serde_json` | — | `serde_json::Value` para JSON opaco (schemas, configs) |
| Config | `dotenvy` | — | precedência implementada à mão (ver §9) |
| Erros | `snafu` | 0.8 | context selectors por crate; `ensure!`; `Whatever` só no binário; `Report` para saída humana no CLI |
| Logging | `tracing`, `tracing-subscriber` | — | `fmt` (dev) / `json` (enterprise); `tower-http::trace` para access log |
| Métricas | `prometheus-client` | 0.23 | registry próprio; endpoint `/metrics` em text format. Sem OTel SDK em v1 (OTLP está no roadmap). |
| Secrets | `secrecy` | — | `SecretString` para tokens/bearers; `Debug` redigido |
| Embed | `rust-embed` | — | dashboard |
| Random/crypto | `rand`, `base64`, `sha2`, `hex` | — | tokens, PKCE, URIs |
| JWT | `jsonwebtoken` | 11 | backend `aws_lc_rs` (o mesmo do `rustls`); só verificação de ID tokens OIDC (§4.9b). Sem `openidconnect`: arrastava `rsa`, com advisory sem correcção |
| Concorrência | `arc-swap`, `dashmap`, `tokio-util` (`CancellationToken`) | — | |
| Testes | `tokio::test`, `wiremock`, `assert_cmd`, `insta` | — | upstream fake MCP via `rmcp` server em processo |

Toolchain: Rust stable ≥ 1.94 (edition 2024). `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo deny check` (licenças + advisories) obrigatórios no CI.

## 4. Domínio (`trunks-core`)

### 4.1 Newtypes e enums (em `trunks-types`)

```rust
pub struct ServerName(String);        // validado: ^[a-zA-Z0-9_-]+$, sem "__", não termina em "_"
pub struct CanonicalName { server: ServerName, local: String }   // Display = "{server}__{local}"
impl CanonicalName { pub fn parse(s: &str) -> Result<Self>  /* split no PRIMEIRO "__" */ }
pub struct ResourceUri { server: ServerName, original: String }  // Display = "trunks://res/{server}/{b64url_nopad(original)}"
pub struct GroupName(String);         // ^[a-zA-Z0-9][a-zA-Z0-9_-]*$

#[serde(rename_all = "snake_case")] pub enum Transport { Stdio, StreamableHttp, Sse }
#[serde(rename_all = "snake_case")] pub enum SessionMode { #[default] Stateless, Stateful }
#[serde(rename_all = "lowercase")]   pub enum Role { Admin, User }
#[serde(rename_all = "lowercase")]   pub enum ServerMode { Development, Enterprise }
// "production" aceite na desserialização e normalizado para Enterprise
```

`Sse` mantém-se no enum para desserializar DBs legacy; o arranque falha com erro explícito se existir um servidor `sse` activo (D4).

### 4.2 Modelo de dados

Schema de tabelas e colunas compatível com DBs legacy (D3) para que a migração de dados seja só normalização. Todas as tabelas têm `id BIGSERIAL/INTEGER PK`, `created_at`, `updated_at`, `deleted_at NULL` (coluna de soft-delete herdada do schema; Trunks faz hard-delete e filtra `deleted_at IS NULL` nas leituras).

| Tabela | Colunas relevantes | Notas |
|---|---|---|
| `mcp_servers` | `name UNIQUE`, `transport varchar(30)`, `enabled bool default true`, `description`, `config JSON NOT NULL`, `session_mode varchar(20) default 'stateless'` | `config` é `StreamableHttpConfig{url, bearer_token?, headers?}` \| `StdioConfig{command, args?, env?}` \| `SseConfig{url, bearer_token?}` |
| `tools` | `name` (bare), `enabled`, `description`, `input_schema JSON`, `annotations JSON`, `server_id FK` | índice único `(server_id, name)` adicionado por migração |
| `prompts` | `name`, `enabled`, `description`, `arguments JSON`, `server_id` | idem |
| `resources` | `uri` (público `trunks://`), `original_uri`, `name`, `enabled`, `description`, `mime_type`, `annotations JSON`, `meta JSON`, `server_id` | idem, UNIQUE(uri) |
| `server_configs` | `mode varchar(12)`, `initialized bool` | 0 ou 1 linha |
| `users` | `username UNIQUE`, `role`, `access_token UNIQUE`, `oidc_issuer`, `oidc_subject` (migração `0006`; índice único parcial no par) | plaintext na v1 (compat); hash em D8. O par OIDC é NULL em users locais |
| `sessions` | `id_hash PK` (SHA-256 hex do valor do cookie), `user_id`, `created_at`, `expires_at` | sessões de browser (D16); o valor do cookie nunca é guardado |
| `oidc_login_states` | `state PK`, `nonce`, `code_verifier`, `return_to`, `expires_at` | login OIDC em curso; uso único, TTL 10 min |
| `mcp_clients` | `name UNIQUE`, `description`, `access_token UNIQUE`, `allow_list JSON NOT NULL` | `["*"]` ou lista de nomes |
| `tool_groups` | `name UNIQUE`, `description`, `included_tools JSON`, `included_servers JSON`, `excluded_tools JSON` | |
| `upstream_oauth_pending_sessions` | `session_id UNIQUE` (64 hex), `server_name IDX`, `transport`, `server_input JSON`, `force`, `redirect_uri`, `client_id`, `client_secret`, `scopes JSON`, `state`, `code_verifier`, `expires_at IDX`, `initiated_by` | TTL 10 min |
| `upstream_oauth_tokens` | `server_name UNIQUE`, `transport`, `client_id`, `client_secret`, `redirect_uri`, `scopes JSON`, `access_token`, `token_type`, `refresh_token`, `scope`, `expires_at`, **`issuer`** (novo, migração `0004`) | |

JSON: Postgres `jsonb`; SQLite `TEXT`. Lido/escrito como `sqlx::types::Json<serde_json::Value>` em ambos, sem casts.

### 4.3 Catalog

```rust
pub struct CatalogSnapshot {
    servers: HashMap<ServerName, Arc<ServerRecord>>,          // só enabled
    tools:   BTreeMap<CanonicalName, Arc<ToolEntry>>,         // só enabled && server.enabled
    prompts: BTreeMap<CanonicalName, Arc<PromptEntry>>,
    resources: BTreeMap<ResourceUri, Arc<ResourceEntry>>,
    groups:  HashMap<GroupName, Arc<GroupView>>,              // effective tools pré-resolvidas
}
pub struct Catalog(ArcSwap<CatalogSnapshot>);
```

- Construído a partir da DB no arranque e **reconstruído por inteiro** após qualquer mutação (register/deregister/enable/disable/group CRUD). Mutações são raras; leituras são por pedido. Reconstruir é O(n) com n ~ centenas — trivial.
- Um grupo cuja resolução falha (servidor incluído já não existe) resulta num `GroupView` vazio + `warn!` — preserva o comportamento "startup degradado mas não falha".
- `GroupView.effective_tools` = `included_tools ∪ ⋃ tools(included_servers) − excluded_tools`, só com tools presentes no snapshot (i.e. enabled). Ordem lexicográfica.

### 4.4 Registry service

API interna (chamada pelos handlers REST e dashboard):

```rust
pub async fn register(&self, input: RegisterServerInput, force: bool, initiated_by: &str)
    -> Result<RegisterOutcome, RegistryError>;   // Registered(ServerSummary) | OAuthRequired{session_id, authorization_url, expires_at}
pub async fn deregister(&self, name: &ServerName) -> Result<(), RegistryError>;
pub async fn set_server_enabled(&self, name, enabled) -> Result<EnableDisableResult>;
pub async fn set_entities_enabled(&self, kind: EntityKind, entity: &str, enabled) -> Result<Vec<String>>; // entity = server | canonical | trunks:// URI
```

Implementação (Fase 2b): `RegistryService { store: Arc<dyn RegistryStore>, catalog, sessions, connect }`. O port `RegistryStore` ganha `create_server(NewServer, Discovered, replace)`, `delete_server`, `set_server_enabled`, `set_{tools,prompts,resources}_enabled(server_id, key?, enabled) -> nomes alterados` — cada um numa transacção. A flag do servidor **não** altera as linhas das entidades: o estado efectivo é `entity.enabled && server.enabled`, calculado no catálogo, pelo que reactivar um servidor repõe o estado individual anterior. O `register` sem OAuth rejeita (400) qualquer campo `oauth_*` com `OAUTH_NOT_SUPPORTED_YET` até à Fase 5.

Invariantes (verificadas por testes):

1. `register`: validar nome e URL → **ligar e `initialize` ao upstream** (timeout `TRUNKS_UPSTREAM_INIT_TIMEOUT_SEC`) → só depois persistir. Falha de connect = nada persistido.
2. Descoberta: `tools/list` obrigatório (falha → falha o registo); `prompts/list` e `resources/list` só se o upstream anunciar a capability e são best-effort (warn, não falham). Resources sem `uri` ou com `uri` vazio são ignorados (não têm URI público: `trunks://res/<server>/` não é parseável).
3. Registo e deregisto correm numa **transacção** (servidor + entidades filhas + tokens/sessões OAuth).
4. `force=true`: deregista o existente antes de registar; persistido na sessão OAuth pendente para repetir no `complete`.
5. Enable/disable por entidade devolve só os nomes cujo estado **efectivo** mudou no nível servidor, e `[entity]` sempre no nível individual (mesmo se já estava nesse estado). Desactivar um servidor invalida a sua sessão stateful.
6. Resource enable/disable aceita nome de servidor **ou** URI `trunks://`; outra string → `InvalidInput`.
7. Headers `Authorization` explícitos ganham sobre `bearer_token` (log `warn!`).

### 4.5 Gateway handler (rmcp `ServerHandler`)

```rust
pub struct GatewayHandler { scope: Scope, state: Arc<GatewayState> }
pub enum Scope { Global, Group(GroupName) }

impl ServerHandler for GatewayHandler {
    fn get_info(&self) -> ServerInfo  // name: "Trunks MCP Gateway" | "Trunks gateway for tool group: {g}"; version = build
                                      // capabilities: tools(list_changed=false), prompts(list_changed=false), resources(subscribe=false, list_changed=false)
    async fn list_tools(&self, _, ctx) -> ListToolsResult;        // Catalog ∩ scope ∩ principal.allow_list
    async fn call_tool(&self, req, ctx) -> CallToolResult;
    async fn list_prompts(&self, _, ctx); async fn get_prompt(&self, req, ctx);
    async fn list_resources(&self, _, ctx); async fn read_resource(&self, req, ctx);
}
```

**Principal** — extraído de `ctx.extensions`: o `rmcp` propaga `http::request::Parts` por pedido e o middleware axum insere `Principal` em `Parts.extensions` (verificado no Spike S1):

```rust
pub enum Principal { DevOpen, McpClient(Arc<McpClientRecord>), /* nunca User aqui */ }
impl Principal { fn may_access(&self, server: &ServerName) -> bool }
```

Ausência de `Principal` em modo enterprise → `list_*` devolve vazio e `call_*` devolve erro `-32603 "not authorized"` — **fail-closed**. Em modo dev, `Principal::DevOpen`.

Regras de filtragem:
- `list_tools`, `list_prompts`, `list_resources`: filtram pelo que o cliente pode usar (allow-list ∩ acesso do dono). Tool cujo nome não tem `__` é negada.
- `call_tool`/`get_prompt`/`read_resource`: verificam o acesso **antes** de criar qualquer sessão upstream. O que o cliente não pode usar responde **exactamente como o que não existe** — `-32602 "tool '<nome>' not found"` (idem `prompt '…'`, `resource '…'`) —, tal como a REST API: nem as listagens nem os erros revelam o que está registado fora do alcance do cliente.
- `Scope::Group` restringe `list_tools`/`call_tool` às effective tools; `list_prompts`/`list_resources` em scope de grupo devolvem **vazio** (grupos só expõem tools em v1).

Encaminhamento de uma chamada:
1. `CanonicalName::parse` → `(server, local)`; não encontrado no catálogo → `-32602` (equivalente a tool desconhecida).
2. Autorização.
3. `start = Instant::now()`; `session = sessions.acquire(server).await?`.
4. Reescrever `params.name = local` (ou `params.uri = original`); **descartar todos os headers/meta do pedido downstream** — o `UpstreamConn` só conhece as credenciais configuradas.
5. `session.call_tool(params).await`; em erro, `session.mark_unhealthy_if_connection_error(&err)`.
6. Resources: reescrever `contents[*].uri` upstream → `trunks://` na resposta.
7. `metrics.record(server, local, outcome, elapsed)`; `session.release()` (fecha se stateless).

### 4.6 Sessões upstream

```rust
pub struct SessionManager {
    slots: DashMap<ServerName, Arc<tokio::sync::Mutex<Option<Arc<UpstreamConn>>>>>,
    idle_timeout: Option<Duration>,    // None = nunca expira (TRUNKS_SESSION_IDLE_TIMEOUT_SEC = 0)
    init_timeout: Duration,            // TRUNKS_UPSTREAM_INIT_TIMEOUT_SEC, default 30s
    cancel: CancellationToken,
}
pub struct UpstreamConn { peer: rmcp::service::RunningService<RoleClient, ()>, last_used: AtomicU64, kind: ConnKind }
```

- `acquire(server)`: `Stateless` → nova `UpstreamConn` (dropped no `release`). `Stateful` → lock **do slot desse servidor**, reutiliza ou cria. O connect de um servidor lento não bloqueia os outros.
- Sweeper: `tokio::time::interval(60s)` fecha slots com `now - last_used > idle_timeout`.
- `invalidate(server)` no deregister; `shutdown()` fecha tudo (chamado antes do drain HTTP).
- Classificação de erro de ligação: `rmcp::ServiceError::Transport(_)`, `TransportClosed`, `Timeout`, erros `std::io` com kinds `ConnectionRefused/Reset/Aborted/BrokenPipe/UnexpectedEof/TimedOut` → invalidar. **Não** se faz substring matching em mensagens de erro.
- Sem retry automático da chamada falhada em v1 (roadmap: reconnect com backoff).

### 4.7 Ligações a upstreams (`UpstreamConn::connect`)

| Transporte | Implementação |
|---|---|
| `streamable_http` | `rmcp::transport::StreamableHttpClientTransport` com `reqwest::Client` por servidor: headers configurados + `Authorization: Bearer` (se não houver `Authorization` explícito). Se existir `upstream_oauth_tokens` para o servidor → wrapper `AuthClient` do módulo `rmcp::transport::auth` com `CredentialStore` persistente em DB (refresh automático). `initialize` com `tokio::time::timeout(init_timeout)`; erro mapeado para mensagem que menciona `TRUNKS_UPSTREAM_INIT_TIMEOUT_SEC`; `ConnectionRefused` + URL loopback → hint `host.docker.internal`. |
| `stdio` | `rmcp::transport::TokioChildProcess` sobre `tokio::process::Command`: `env` configurado **sobre** o ambiente herdado do processo (documentar; opção `inherit_env: false` no roadmap), `kill_on_drop`. O processo é a raiz de uma **árvore que morre inteira** (`process-wrap`): em Unix lidera o seu próprio process group; em Windows corre num job object que termina os seus processos quando o handle fecha. Ao fechar a ligação: o rmcp fecha o stdin, espera, envia `SIGTERM` (Unix) e por fim mata o processo; a seguir tudo o que reste do grupo recebe `SIGKILL`. O mesmo acontece se a ligação for largada sem fecho (timeout, erro no `initialize`) — nenhum descendente (`npx` → `node`, script → servidor) fica órfão. Stderr **nunca herdado**: é ligado por pipe e uma task regista cada linha em `INFO` com `target = "upstream.stderr"` e `server` (o stderr é o canal normal de log de um servidor stdio, não um sinal de erro). Antes de registar: todos os valores de `env` configurados com ≥ 4 caracteres (e cada linha de um valor multi-linha) são substituídos por `[redacted]`; a linha é cortada aos 4096 bytes sem nunca deixar parte de um segredo; caracteres de controlo são escapados. Silenciar: `TRUNKS_LOG=info,upstream.stderr=off`. `clientInfo = {name: "trunks upstream client", version}`. |
| `sse` | **[BREAKING, D4=(a)]** não implementado. `connect` devolve `UnsupportedTransport`. |

Depois do `initialize`, cada pedido a um upstream (`tools/call`, `prompts/get`, `resources/read` e as listagens da descoberta) tem um limite `call_timeout` = `TRUNKS_REQUEST_TIMEOUT_SEC` (0 = sem limite). Ao expirar, o pedido é abandonado e devolve `UpstreamTimeout` — `upstream '{server}' did not answer within {n}s (TRUNKS_REQUEST_TIMEOUT_SEC)`: 504 em REST, erro JSON-RPC interno em MCP. Conta como erro de ligação: fica nas notas de saúde do upstream e uma sessão stateful é fechada e reaberta na chamada seguinte.

### 4.8 OAuth upstream

Fluxo em duas fases, implementado sobre `rmcp::transport::auth` (`AuthorizationManager`, PKCE, discovery, DCR, `StateStore`/`CredentialStore` nossos, `AuthClient`) — validado no Spike S3 (`docs/spikes/S3-upstream-oauth.md`).

Fase 1 (`register` falha com 401/`WWW-Authenticate` → `AuthorizationRequired`):
1. `oauth_redirect_uri` obrigatório → senão `InvalidInput` + `code = "upstream_oauth_required"` (o CLI usa este código para arrancar o listener loopback).
2. Discovery; se `client_id` vazio → DCR com body `{client_name: "trunks-{server}", redirect_uris, token_endpoint_auth_method: "none"|"client_secret_basic", grant_types: ["authorization_code","refresh_token"], response_types: ["code"], scope?}` (`scope` omitido se vazio); sem `registration_endpoint` ou 403 → erro que instrui a fornecer `oauth_client_id`/`oauth_client_secret`.
3. Gerar `state` (32 bytes hex), `code_verifier`/`challenge` (S256), `session_id` (32 bytes hex = 64 chars).
4. Transacção: apagar sessão pendente anterior do mesmo servidor; inserir nova com `expires_at = now + 10 min`.
5. Devolver `202 {authorization_required: {session_id, authorization_url, expires_at}}`.

Fase 2 (`POST /api/v0/upstream_oauth/sessions/{id}/complete {code, state}`): validar presença → carregar sessão (`NotFound`) → expirada (`InvalidInput`) → `state` diferente (`InvalidInput`) → upsert metadata em `upstream_oauth_tokens` sem tocar nos tokens → troca do code (grava tokens via `CredentialStore`) → se `force`, deregistar existente → registar com credenciais guardadas → apagar sessão pendente → `201 {server}`.

`CredentialStore` em DB: `get` devolve `None` se não há linha ou `access_token` vazio; `save` preserva `client_id/secret/redirect_uri/scopes`.

Implementação (Fase 5), `trunks-core::oauth` + `RegistryService`:
- Port `OAuthStore` (`credentials`, `save_credentials`, `clear_tokens`, `delete_oauth`, `pending`, `save_pending`, `delete_pending`, `purge_expired_pending`) implementado por `trunks_db::Store`; migração `0004_oauth_issuer` acrescenta `upstream_oauth_tokens.issuer`.
- `UpstreamConn::connect_http` detecta o 401 **tipado** (`AuthRequiredError` na cadeia de erros do `initialize`) → `Error::UpstreamAuthRequired{www_authenticate}`; nunca por substring. Se o servidor tem tokens guardados, o `reqwest::Client` (já com os headers configurados) é embrulhado em `AuthClient` (`initialize_from_store` + refresh transparente); os tokens guardados ganham a um `bearer_token` estático.
- `RegistryService::register` devolve `RegisterOutcome::{Registered, AuthorizationRequired}`. No 401: sem `oauth_redirect_uri` → `UpstreamOAuthRequired` (400 + `code`); com ele → `oauth::bootstrap` (discovery a partir do challenge, DCR com `client_name = "trunks-{server}"` só sem `oauth_client_id`, URL com PKCE S256 + `state`, registo do cliente persistido já em `upstream_oauth_tokens` sem tokens, sessão pendente com o `RegisterServerInput` completo e `force`; `session_id` = 32 bytes hex; TTL 10 min). O `StateStore` do `rmcp` é em memória e **capturado** — a persistência é explícita na sessão pendente.
- `complete_oauth(session_id, code, state)`: `NotFound` → expirada (apaga a sessão) → `state` diferente (sem pedido ao token endpoint) → `oauth::exchange` (novo `AuthorizationManager` com o `StateStore` pré-carregado da sessão; troca falhada mantém a sessão para retry) → apaga a sessão → `register(input, force)` normal (a ligação já usa o bearer). `oauth_*` só com `streamable_http`.
- `deregister` apaga credenciais e sessões pendentes; o arranque purga sessões expiradas.
- Rota: `POST /api/v0/upstream_oauth/sessions/{id}/complete {code, state}` → `201 {server}` (admin). `POST /servers` → `202 {authorization_required: {session_id, authorization_url, expires_at}}`.
- Em modo stateless cada chamada faz discovery + `initialize` com o bearer (um round-trip extra ao AS); cache de metadata fica para o roadmap.

### 4.9 Identidades e tokens

- `generate_token()` → 32 bytes CSPRNG, base64url sem padding (43 chars). `validate_custom_token(s)`: `len ≥ 8` && sem whitespace Unicode.
- `IdentityService` (users): `create_admin()` (só via `/init`), `create_user(name, token?)` role sempre `user`, `update_user_token` (rotação, custom ou gerado; o token antigo deixa de valer no pedido seguinte), `delete_user` (o **último** admin → `InvalidInput`; idempotente; apaga na mesma transacção sessões, memberships, grants e clients do user), `authenticate_user`.
- `IdentityService` (MCP clients): `create_client(name, desc, allow_list, token?)` (lista vazia = sem acesso), `update_client(desc?, allow_list?, token?)` (só os campos dados; `COALESCE` na DB), `delete_client` idempotente, `authenticate_mcp_client`. `allow_list` malformada → sem acesso; entradas válidas: `"*"` ou nomes de servidor válidos (não precisam de existir).
- Nomes de identidades: não vazios, sem whitespace, ≤ 100 chars. Duplicados → 400 `… already exists` (pré-verificação por nome; a constraint única da DB é mapeada para 400 em caso de corrida ou de token repetido).
- **Tokens só são serializados duas vezes**: na criação e na rotação. `GET /users` e `GET /clients` nunca os devolvem (decisão própria; o legado devolvia em plaintext).
- v1: comparação exact-match em plaintext (compat com DB existente). D8: coluna `access_token_hash`, migração que hasheia e limpa `access_token`.

### 4.9a Modelo de acesso (D14) — `trunks-core::access`

- Tabelas (`0005_access`): `teams(name UNIQUE, description)`, `team_members(team_id, user_id, role maintainer|member, source manual|oidc — `0006`)`, `access_grants(subject_kind user|team, subject_id, target_kind server|tool_group, target_name, permission use)` com alvos **por nome** (sobrevivem a `register --force`; `*` = todos os servidores), `mcp_clients.owner_kind/owner_id` (NULL = sem dono).
- `AccessSet {all, servers, tool_groups}` é resolvido **uma vez na autenticação** e vai no `Principal`/`UserPrincipal`. User: união dos grants próprios e das suas teams; admin: `all`; development: `all`. Client com dono: conjunto do dono ∩ `allow_list` (vazia ou `*` = tudo do dono; lista não vazia = só esses servidores, e perde os grupos); sem dono: `allow_list` como sempre (fail-closed).
- Regras: pode usar a tool `T` se `T.server` está concedido **ou** `T` pertence às effective tools de um grupo concedido; prompts e resources exigem o servidor; um grupo concedido não alarga acesso a prompts/resources. Um servidor é **visível** se concedido ou se alguma das suas tools o é por via de grupo. Entidades negadas respondem **404** (não 403) para não revelar existência.
- Papéis de team: `maintainer` gere membros e os clients da team; `member` só herda. Admin global gere tudo. Nomes de team seguem as regras de nomes de identidades.
- Transição: um `user` migrado sem grants **não vê nada** até um admin conceder (nota de migração).

### 4.9b Login de humanos (D16, D17) — `trunks-core::{oidc, login}`

- **Sessões.** `LoginService` abre sessões de browser: valor = `generate_token()`, entregue só no cookie `trunks_session` (`HttpOnly; SameSite=Lax; Path=/; Max-Age=<ttl>`, `Secure` quando o URL público é `https` ou, sem URL público, `X-Forwarded-Proto: https`); a DB guarda apenas o SHA-256. TTL `TRUNKS_SESSION_TTL_HOURS`; expiradas são purgadas no arranque e ignoradas na leitura. Duas entradas: OIDC (abaixo) e `POST /auth/token` (troca um access token de user por sessão — login por token da web app, D16).
- **OIDC** (`OidcClient`): Authorization Code + PKCE `S256`, `state` e `nonce` de 32 bytes CSPRNG. `begin` devolve o URL de autorização; `state`, `nonce`, `code_verifier` e `return_to` ficam em `oidc_login_states` (uso único, 10 min) — nada em memória, por isso funciona com várias réplicas. `return_to` só aceita caminhos relativos (começa por `/`, sem `//` inicial, sem `\` e sem `://`); o resto vira `/`.
- **Discovery e chaves.** `GET <issuer>/.well-known/openid-configuration` (o `issuer` do documento tem de ser **igual** ao configurado) + JWKS, em cache 1 h. Chaves que o build não sabe usar são ignoradas uma a uma. Um token com `kid` desconhecido ou assinatura que não verifica força **um** refresh do JWKS (rotação de chaves), no máximo de 30 em 30 s; falhas de claims não provocam refresh. Redirects HTTP nunca são seguidos; timeout 15 s.
- **Troca do code.** `POST token_endpoint` (`grant_type=authorization_code`, `code`, `redirect_uri`, `code_verifier`); client confidencial autentica-se com HTTP Basic, ou `client_secret_post` quando o IdP só anuncia esse; client público envia só `client_id`. Dos erros do IdP só `error`/`error_description` são mostrados.
- **Verificação do ID token** (`jsonwebtoken`): algoritmo ∈ {`RS*`, `PS*`, `ES256`, `ES384`, `EdDSA`} — **simétricos (`HS*`) e `none` nunca**; assinatura contra o JWKS; `iss` = issuer configurado; `aud` contém o `client_id`; `azp`, se presente, = `client_id`; `exp` (60 s de tolerância); `nonce` = o do pedido; `sub` não vazio. `exp`, `iss`, `aud` e `sub` são obrigatórios. Só depois disto se lêem os restantes claims.
- **Identidade**: `(issuer, sub)`; username = `preferred_username`, senão `email`, senão `sub` (tem de cumprir as regras de nomes; senão `sub`). Grupos: claim `TRUNKS_OIDC_GROUPS_CLAIM` do ID token (array de strings, ou string separada por vírgulas/espaços); se ausente, o mesmo claim do `userinfo` (só se o `sub` coincidir). Ausente em ambos ⇒ **nada é sincronizado**.
- **Provisionamento JIT** (D17): procura o user por `(issuer, sub)`; se não existir, cria-o (role `user`, ou `admin` se o grupo de admin estiver nos claims) com um access token gerado que nunca é mostrado. **Nunca liga a um user local pelo nome**: se o username já existe sem par OIDC → 403 (evita que o naming do IdP tome conta de uma conta local, ex. o `admin` de bootstrap).
- **Sincronização em cada login** (só com claim de grupos presente): com `TRUNKS_OIDC_ADMIN_GROUP` definido, o role segue o claim nos dois sentidos, excepto que o **último admin nunca é despromovido**; com `TRUNKS_OIDC_SYNC_TEAMS`, cada grupo com o prefixo `TRUNKS_OIDC_TEAM_PREFIX` (retirado; nome resultante tem de ser válido) torna-se team — criada se não existir — e as memberships com `team_members.source = 'oidc'` são substituídas pelo conjunto actual (role `member`); memberships manuais (`source = 'manual'`) nunca são tocadas. Os **grants continuam a ser geridos no Trunks**.
- OIDC é só para humanos na web: os endpoints MCP **nunca** aceitam cookies (v2.0 no roadmap trata dos MCP clients).

### 4.10 Tool groups

```rust
pub struct ToolGroupSpec { name: GroupName, description: String, included_tools: Vec<String>, included_servers: Vec<String>, excluded_tools: Vec<String> }
```
- `create`: validar nome → resolver contra o **catálogo actual** → `len == 0` → `InvalidInput`; tool incluída inexistente/desactivada → `InvalidInput "…does not exist or is disabled"` → insert → rebuild catálogo.
- `update`: diff old/new (`added`/`removed` por campo) → validar adicionadas → **update completo da linha incluindo listas vazias** → rebuild. Devolve `{name, old, new}`.
- `delete`: idempotente (não dá 404); o handler HTTP larga o `StreamableHttpService` cacheado do grupo.
- Implementação (Fase 4): `trunks-core::groups::{validate, diff, GroupService}`. `validate` também exige que os `included_servers` existam, que as exclusões sejam nomes canónicos válidos, e normaliza as listas (sem duplicados, ordenadas). A definição é guardada como escrita (nomes); um grupo cujos inputs desaparecem (tool desactivada, servidor removido) **degrada para vazio** no catálogo sem falhar e recupera quando voltam.
- Shapes: `ToolGroup {name, description, included_tools[], included_servers[], excluded_tools[]}` (body de `POST`/`PUT`, itens de `GET /tool-groups`, ficheiro `-c`); `POST /tool-groups` → `201 ToolGroupWithEndpoints` (`ToolGroup` + `streamable_http_endpoint: "/v0/groups/{n}/mcp"`); `GET /tool-groups/{n}` idem; `PUT /tool-groups/{n}` → `200 {name, old, new}` (o `name` do body é ignorado); `DELETE` → 204; `GET /tool-groups/{n}/effective-tools` → `[Tool]`. Erros: `GROUP_NO_TOOLS`, `tool '{t}' does not exist or is disabled`, `server '{s}' does not exist`, `tool group '{n}' already exists`.
- Endpoints: `GET /v0/groups/{g}/mcp` montado dinamicamente — o router tem um handler em `/v0/groups/{name}/mcp` que resolve o grupo no catálogo (404 `{"error":"tool group not found: {name}"}`) e despacha para um `StreamableHttpService<GatewayHandler{Group}>` cacheado em `DashMap<GroupName, _>` (invalidado no delete). `/v0/groups/{g}/sse|message`: **[BREAKING]** não existem (D4).

## 5. API HTTP (`trunks-api`)

Contrato `/api/v0` (estável dentro de 1.x; shapes detalhados no doc interno de migração):

| Grupo | Middleware | Rotas |
|---|---|---|
| Público | — | `GET /health` → `{"status":"ok"}`; `GET /metadata` → `{"version"}`; `POST /init {mode}` → dev `{"status"}` / enterprise `{"status","admin_access_token"}`; já inicializado → 400; `mode` diferente daquele com que o processo arrancou → 400 `this server was started in {x} mode and cannot be initialized in {y} mode`, sem nada gravado; `GET /metrics` (se activo) |
| Auth (browser, D16) | — (só actuam em enterprise; dev → 403 `ENTERPRISE_ONLY`) | `GET /auth/config`, `GET /auth/login?return_to=`, `GET /auth/callback`, `POST /auth/token`, `POST /auth/logout` |
| Dashboard static | `require_initialized` (a web app autentica-se contra a API; D15) | `GET /`, `/index.html`, `/assets/*` |
| MCP | `require_initialized`, `mcp_client_auth` | `ANY /mcp`, `ANY /v0/groups/{name}/mcp` |
| REST user | `require_initialized`, `user_auth` | `GET /api/v0/servers`, `/tools?server=`, `/tool?name=`, `POST /tools/invoke`, `GET /resources?server=`, `POST /resources/get`, `/resources/read`, `GET /prompts?server=`, `/prompt?name=`, `POST /prompts/render`, `GET /users/whoami` (+enterprise) |
| REST admin | + `require_admin` | `POST /servers?force=`, `POST /upstream_oauth/sessions/{id}/complete`, `DELETE /servers/{name}`, `POST /servers/{name}/enable\|disable`, `GET /server_configs`, `POST /tools/enable\|disable?entity=`, `POST /prompts/enable\|disable?entity=`, `GET/POST /clients`, `PUT/DELETE /clients/{name}` (+enterprise), `GET/POST /users`, `PUT/DELETE /users/{username}` (+enterprise), `GET/POST /tool-groups`, `GET/PUT/DELETE /tool-groups/{name}`, `GET /tool-groups/{name}/effective-tools` |
| Dashboard API | `require_initialized`, `user_auth` (dev: aberto; enterprise: bearer de user ou cookie de sessão) | `GET /api/dashboard/{overview,servers,servers/{n},tools?server=&enabled=,prompts?server=,resources?server=,tool-groups,tool-groups/{n},diagnostics,oauth/session/{id}}`; `GET /api/dashboard/oauth/callback` (HTML, **público**) |

Dashboard (D15 — só leitura agregada e sanitizada; as mutações usam `/api/v0`). Tipos em `trunks_core::dashboard`:
- `overview` → `{mode, version, uptime_seconds, counts{servers, servers_enabled, tools, tools_enabled, prompts, resources, tool_groups, pending_oauth}, sessions{stateful_active}, metrics_enabled}`.
- `servers` → `[{name, transport, enabled, description, session_mode, config_summary{url?, header_keys[], command? (≤80 chars), args[], env_keys[]}, counts{tools, tools_enabled, prompts, resources}, oauth{status: none|pending|authorized, session_id?, expires_at?}, last_error?, created_at?, updated_at?}]`; `servers/{n}` → o mesmo + `tools[]`, `prompts[]`, `resources[]` (shapes de `/api/v0`).
- `tools|prompts|resources` → listas de `/api/v0` com filtros `server` (e `enabled` em tools).
- `tool-groups[/{n}]` → `ToolGroup + {streamable_http_endpoint, effective_tools[], degraded}`.
- `diagnostics` → `{database{backend, url_redacted, migrations_latest}, bind{host, port}, allowed_hosts[], upstream_init_timeout_sec, session_idle_timeout_sec, metrics_enabled, servers_unreachable[{name, error, at}], pending_oauth[{session_id, server_name, expires_at}]}`. `servers_unreachable` vem das notas de saúde do `SessionManager` (último erro de ligação/transporte por upstream; um erro de tool **não** conta); sem probing activo.
- `oauth/session/{id}` → `{session_id, server_name?, status: pending|expired|completed|unknown, expires_at?}` — `completed` é conhecido pelo processo que fez a troca (cache em memória, 1 h).
- `oauth/callback?code=&state=` (ou `?error=&error_description=`) → página HTML; o backend procura a sessão pelo `state`, completa a troca e regista o servidor; a página faz `postMessage({source:'trunks-oauth'})` ao `opener`. Erros renderizam a página com a razão (nunca 500). A web app usa `oauth_redirect_uri = <origin>/api/dashboard/oauth/callback` no `POST /api/v0/servers` e faz polling de `oauth/session/{id}`.
- Invariante testada: nenhum valor de `env`, header, `bearer_token`, token OAuth ou `client_id` aparece em resposta alguma de `/api/dashboard`.

Auth de browser (`trunks-api::routes::auth`):
- `GET /auth/config` → `{mode: "development"|"enterprise"|null, login_required, oidc}` — o que a web app precisa antes de haver login (`oidc` = `GET /auth/login` disponível).
- `GET /auth/login?return_to=` → `302` para o IdP; 404 `OIDC_NOT_CONFIGURED` se não houver issuer configurado.
- `GET /auth/callback?code=&state=` → `302` para `return_to` + `Set-Cookie: trunks_session=…`. Falhas respondem com uma **página HTML** ("Sign-in failed" + razão) e sem cookie: 400 (o IdP devolveu `error`, ou faltam `code`/`state`), 401 (`state` desconhecido/expirado/repetido, troca do code recusada, ID token inválido), 403 (username já existe como user local), 502 (IdP inalcançável ou discovery inválido).
- `POST /auth/token {access_token}` → `200 {username, role}` + cookie; token inválido → 401. Tokens de MCP client não servem.
- `POST /auth/logout` → `204`, apaga a sessão na DB e expira o cookie (idempotente).

Shapes do registry (admin):
- `POST /servers?force=` body `RegisterServerInput {name, transport, description, url?, bearer_token?, headers{}, command?, args[], env{}, session_mode, oauth_redirect_uri?, oauth_client_id?, oauth_client_secret?, oauth_scopes[]}` → `201 {"server": McpServer}` ou `202 {"authorization_required": {session_id, authorization_url, expires_at}}` (Fase 5). `force` é booleano estrito (`true|false|1|0|t|f`, sem distinção de maiúsculas; outro valor → 400 `INVALID_FORCE_QUERY`). Nome duplicado sem `force` → 400 `server '{n}' already exists; use force to replace it`. Falha de ligação ao upstream → 500 com a razão (nada persistido).
- `DELETE /servers/{name}` → 204; 404 se não existir.
- `POST /servers/{name}/enable|disable` e `POST /{tools|prompts|resources}/enable|disable?entity=` → `200 EnableDisableResult {name?, tools_affected[], prompts_affected[], resources_affected[]}` (listas vazias omitidas). `entity` em falta → 400 `MISSING_ENTITY_QUERY`; entidade desconhecida → 404; para resources, uma string que não é servidor nem URI `trunks://` → 400.
- `GET /server_configs` → `[RegisterServerInput]` **com segredos**: é o formato de `trunks export` e de `trunks register -c`.

Shapes de identidades (enterprise + admin; em development → 403 `ENTERPRISE_ONLY`):
- `GET /users` → `[User {username, role}]`. `POST /users {username, access_token?}` → `201 UserWithToken {username, role, access_token}`. `PUT /users/{username} {access_token?}` → `200 UserWithToken` (token novo; 404 se não existir). `DELETE /users/{username}` → 204 (idempotente; o último admin → 400 `ADMIN_CANNOT_BE_DELETED`).
- `GET /clients` → `[McpClient {name, description, allow_list[], owner?{kind, name}}]` — admin vê todos; um user vê os seus e os das suas teams. `POST /clients {name, description, allow_list[], access_token?, owner?}` → `201 McpClientWithToken {…, owner?, access_token}`; admin escolhe qualquer dono ou nenhum; um user fica como dono salvo se nomear uma team que mantém (senão 403). `PUT /clients/{name} {description?, allow_list?, access_token?, rotate_token}` → `200` (só admin, dono ou maintainer da team dona; senão 403 `NOT_OWNER`). `DELETE /clients/{name}` → 204 (idempotente para admin).
- `GET /users/whoami` → `WhoAmI {username, role, teams[{name, role}], access{all, servers[], tool_groups[]}}`.
- Teams (`require_enterprise`): `GET /teams` → `[Team {name, description}]`; `POST /teams` (admin) → `201 TeamDetail {…, members[{username, role}]}`; `GET /teams/{n}` → `TeamDetail`; `PUT /teams/{n} {description}` (admin); `DELETE /teams/{n}` → 204 idempotente (admin; apaga memberships e grants da team e desassocia os clients que possuía); `POST /teams/{n}/members {username, role?}` (admin ou maintainer; upsert) → `TeamDetail`; `DELETE /teams/{n}/members/{u}` → 204.
- Grants (admin): `GET /grants?target_kind=&target=` → `[GrantSpec {subject_kind, subject, target_kind, target}]`; `POST /grants GrantSpec` → 201 (200 se já existia); `DELETE /grants GrantSpec` → 204; atalhos `GET /servers/{n}/access`, `GET /tool-groups/{n}/access`. Erros: `INVALID_SUBJECT_KIND`, `INVALID_TARGET_KIND`, `INVALID_TEAM_ROLE`, 404 para sujeito inexistente, 400 para nome de alvo inválido.
- Leituras de `/api/v0` (`servers`, `tools`, `tool`, `prompts`, `prompt`, `resources`, `tool-groups[/{n}[/effective-tools]]`) e invocações (`tools/invoke`, `prompts/render`, `resources/{get,read}`) são filtradas pelo `AccessSet` do caller; mutações de grupos continuam admin-only.
- Erros de validação: `USERNAME_REQUIRED`, `NAME_REQUIRED`, `INVALID_TOKEN_FORMAT`, `invalid allow list entry '{e}': …`.

Erros: `{"error": string, "code"?: string}`. Mapeamento `RegistryError → StatusCode`: `NotFound→404`, `InvalidInput→400` (+`code:"upstream_oauth_required"` quando aplicável), `Unauthorized→401`, `Forbidden→403`, `UpstreamTimeout→504`, resto `→500` com a mensagem. Fora do domínio: corpo acima do limite → 413 `"request body too large"`; prazo do pedido REST excedido → 504 `"request timed out"`. Mensagens que testes/CLI dependem: `"server is not initialized"` (403), `"missing access token"`, `"invalid access token: …"`, `"user is not authorized to perform this action"`, `"this request is only allowed in enterprise mode"`, `"missing MCP client access token"`, `"invalid MCP client token"`, `"Server is already initialized"`, `"name is required"`, `"missing 'name' query parameter"`, `"missing 'entity' query parameter"`, `"invalid 'force' query parameter: expected true or false"`, `"url is required for the streamable_http transport"`, `"command is required for the stdio transport"`, `"the 'sse' transport is not supported; register the server as streamable_http"`, `"tool group not found: {name}"`.

Middleware:
- `require_initialized`: lê `ServerConfig` de um `watch::Receiver<Option<ServerMode>>` em memória (actualizado por `/init`), não da DB por pedido. 403 se não inicializado. Insere `ServerMode` nas extensions.
- `user_auth`: dev → `Principal::DevOpen`; enterprise → `Authorization: Bearer` → lookup → 401; sem bearer, cookie `trunks_session` → sessão válida → o mesmo `UserPrincipal` (o bearer ganha sempre). Com cookie, métodos não seguros (`POST/PUT/PATCH/DELETE`) exigem `Origin` igual ao URL público ou ao `Host` do pedido; ausente ou diferente → 403 `CROSS_ORIGIN_REJECTED`. Pedidos com bearer não têm esta verificação.
- `mcp_client_auth`: dev → `Principal::DevOpen`; enterprise → bearer → lookup em `mcp_clients` (tokens de user e cookies de sessão são **rejeitados**) → 401; insere `Principal::McpClient`.
- `require_admin`: dev → passa; enterprise → role ≠ admin → 403.
- `require_enterprise`: 403 com a mensagem acima.
- `trace`: access log — uma linha `INFO` por resposta com `method`, `path` (**sem query string**: transporta `code`/`state` de OAuth), `status`, `latency` e `principal` ∈ `dev|user|admin|mcp_client` (nunca o token).
- Prazo REST: `TRUNKS_REQUEST_TIMEOUT_SEC` + 5 s de folga (para um upstream que atinja o mesmo limite ser reportado como tal), leitura do corpo incluída → 504. **Não** se aplica aos endpoints MCP, que são streams; aí o limite é por pedido ao upstream (§4.7).
- Host/Origin: `StreamableHttpServerConfig.allowed_hosts` por defeito é loopback-only (protecção DNS rebinding). Configurável via `TRUNKS_ALLOWED_HOSTS` (lista separada por vírgulas; `*` desliga).
- Body limit: 4 MiB (`trunks_api::BODY_LIMIT`) em REST (`DefaultBodyLimit`) e em MCP (`max_request_body_bytes` do rmcp, fixado explicitamente); acima → 413.
- Clientes lentos (`trunks_api::serve`, o accept loop próprio sobre `hyper-util`): o cabeçalho completo de um pedido tem de chegar em 30 s — contados desde a ligação, incluindo a fase em que ainda não se sabe se é HTTP/1 ou HTTP/2 —, senão a ligação é fechada (o mesmo prazo fecha ligações keep-alive paradas); o corpo de um pedido MCP não pode ficar mais de 30 s sem dados (em REST o corpo é coberto pelo prazo do pedido); ligações HTTP/2 são verificadas com ping de 30 em 30 s. Todas as ligações aceites têm `TCP_NODELAY`: as respostas MCP são um cabeçalho seguido de um evento pequeno, e com o algoritmo de Nagle o evento esperava pelo ACK (atrasado) do cabeçalho — cerca de 40 ms a mais em cada resposta pequena para clientes que reutilizam a ligação.

Web app: `trunks-dashboard` embebe o bundle de `web/app` (Lit + `line://ui`) com `rust-embed`; build via `cargo xtask build-dashboard` (`npm ci && npm run build`, copia para `crates/trunks-dashboard/dist`). Rotas `/`, `/index.html`, `/assets/*`; sem SPA fallback em v1. `summarize_server_config` nunca emite `bearer_token`, valores de `env`, header `authorization` (case-insensitive), userinfo/query/fragment de URLs; `command` truncado a 80 chars.

## 6. SDK (`trunks-client`)

`Client::new(base_url, token: Option<SecretString>, reqwest::Client)`, um método por endpoint, `ApiError { status, message, code }` via `snafu`. Todos os métodos `async` recebem `&self`, timeouts configuráveis, **não expõe tipos internos de DB** (`ListPrompts` devolve `trunks_types::Prompt`, não a struct de linha).

## 7. CLI (`trunks`)

Árvore de comandos (inventário completo no doc interno de migração; exit codes 0/1). Pontos de implementação:

- `clap` derive com `#[command(display_order)]` para reproduzir os grupos "basic"/"advanced" do help.
- Flag persistente `--registry` (default `http://127.0.0.1:8080`); precedência flag explícita > `registry_url` do ficheiro > default; TIP impresso quando a flag foi usada e o ficheiro não tem `registry_url`.
- Ficheiro de config `~/.config/trunks/config.toml` (`registry_url`, `access_token`; XDG, `0600`).
- `register`: `--name`, `--transport stdio|streamable-http` (inferido de `--url` quando omitido), `--description`, `--url`, `--bearer-token`, `--header NAME=VALUE`*, `--command`, `--arg ARG`*, `--env NAME=VALUE`*, `--session-mode stateless|stateful`, `--force`, `-c/--conf FILE` (JSON `RegisterServerInput`; flags sobrepõem-se ao ficheiro). `${VAR}` é expandido em todas as strings do ficheiro; variável em falta = erro. Saída: `Server <n> registered successfully!` seguida das tools/prompts/resources, ou `This server does not provide any tools, prompts or resources.`
- `deregister <n>` → `Server <n> deregistered successfully!`. `enable|disable server|tool|prompt|resource <entity>` imprime as entidades afectadas. `get prompt <n> [--arg k=v]*`, `get resource <uri> [--read]`. `export [-d DIR]` (default `.trunks`; o directório tem de estar vazio ou não existir) escreve `servers/<n>.json` **com credenciais** e avisa.
- `create user [name] [--access-token T] [-c FILE]` → `User <n> created.` / `Access token: <t>` / `Log in with: trunks login <t>`. `create mcp-client [name] [--allow s1,s2|*]* [--description] [--access-token] [-c FILE]` → `MCP client <n> created.` / `Access token:` / `Allowed servers:` + aviso quando a lista contém `*` ou está vazia. `update user <n> [--access-token]` roda o token; `update mcp-client <n> [--allow …] [--description] [--access-token T | --rotate-token]`. `delete user|mcp-client <n>`. `list users|mcp-clients` (tabelas sem tokens). Ficheiros `-c` são os bodies JSON dos endpoints, com `${VAR}` expandido.
- `create team <n> [--description]`, `delete team <n>`, `list teams` (com membros), `add-team-member <team> <user> [--role member|maintainer]`, `remove-team-member <team> <user>`, `grant|revoke --user X|--team X --server Y|--group Y` (`--server '*'`), `list grants [--server|--group]`; `create mcp-client … --owner user:x|team:y`; `list mcp-clients` mostra o dono.
- `create group -c FILE` → `Tool group <n> created.` + definição, endpoint MCP e effective tools; `get group <n>`; `list groups`; `list tools --group <g>` (effective tools); `invoke <tool> --group <g>` recusa tools fora do grupo; `update group <n> -c FILE` imprime o diff por campo (`tools added/removed`, `servers …`, `exclusions …`, `description`) ou `No changes detected for tool group <n>.`; `delete group <n>`. `export` escreve também `groups/<n>.json`.
- `register` com fluxo OAuth: primeira tentativa sem redirect; ao receber 400 `upstream_oauth_required` arranca um listener loopback `127.0.0.1:0` em `/oauth/callback`, repete o pedido com esse redirect → 202 → imprime a URL de autorização e tenta abrir o browser (`open`/`xdg-open`/`rundll32`, best-effort) → espera o callback até `expires_at` → `complete` → saída normal de registo. Flags: `--oauth-redirect-uri`, `--oauth-client-id`, `--oauth-client-secret`, `--oauth-scope`*. Com `--oauth-redirect-uri` próprio o CLI imprime a URL e o `session_id` e termina com erro "registration pending"; completa-se com `trunks oauth-complete <session_id> --code … --state …`.
- `invoke`: grava `image_*/audio_*/resource_*` no CWD com a mesma tabela de extensões por MIME.
- Valores que podem começar por `-` são aceites como valores (`allow_hyphen_values`): o token de `login`, `--access-token`, `--bearer-token`, `--oauth-client-secret`, `--oidc-client-secret`, `--code`/`--state` e `--arg` (`--arg -y`). Os tokens gerados são base64url: um em cada 64 começa por `-`, e o próprio CLI manda correr `trunks login <token>`.
- Saída: texto plano. `--output json` em v1.x.

## 8. `start` — arranque e shutdown

1. `dotenvy::dotenv().ok()`.
2. Resolver `ServerMode` (`--mode` > `TRUNKS_MODE` > `development`); `TRUNKS_METRICS_ENABLED` (default on em enterprise).
3. `tracing` init (`TRUNKS_LOG`, fallback `RUST_LOG`, default `info`; JSON em enterprise).
4. DB: `--database-url` > `TRUNKS_DATABASE_URL` (ou `TRUNKS_DATABASE_URL_FILE`) > `sqlite://trunks.db`. SQLite: `?mode=rwc`, `journal_mode=WAL`, `busy_timeout=5000`. **Migrações** `sqlx::migrate!` com um directório por dialecto (`migrations/sqlite`, `migrations/postgres`): baseline idempotente sobre o schema legacy + `normalize_legacy` (detalhe no doc interno de migração).
5. Verificar modo: se inicializado noutro modo → erro `"server is already initialized in X mode, cannot start in Y mode"`. Se não inicializado e dev → `init(dev)` silencioso; enterprise → imprimir lembrete `init-server`.
6. Validação: se existir `mcp_servers.transport = 'sse'` **enabled** → erro de arranque com instruções (D4), ou warn + skip se `TRUNKS_IGNORE_SSE_SERVERS=1`.
7. Construir `Catalog` (falha de parse de `input_schema` → **erro de arranque**; `annotations` inválidas → warn + vazio).
8. `SessionManager`, `GatewayState`, métricas, router.
9. Bind `--host`/`TRUNKS_BIND_HOST` (vazio = todas as interfaces) + `--port`/`TRUNKS_PORT`/8080. Banner + `"Trunks HTTP server listening on {addr}"`.
10. `SIGINT`/`SIGTERM` → `trunks_api::serve` deixa de aceitar ligações e dá 10 s aos pedidos em curso (streams MCP ainda abertos ao fim desse tempo são cortados, com um `warn`) → `sessions.shutdown().await` → `"server stopped"`, exit 0.

## 9. Configuração — tabela completa

Precedência: flag > env var > `.env` (via `dotenvy`) > default. Todas as env vars têm prefixo `TRUNKS_`.

| Flag / env | Semântica | Default |
|---|---|---|
| `--mode` / `TRUNKS_MODE` | `development` \| `enterprise`; case-insensitive | `development` |
| `--host` / `TRUNKS_BIND_HOST` | bind host; vazio = todas as interfaces | todas |
| `--port` / `TRUNKS_PORT` | bind port | 8080 |
| `--database-url` / `TRUNKS_DATABASE_URL` | `sqlite://<path>` ou `postgres://user:pass@host:port/db` | `sqlite://trunks.db` |
| `--database-url-file` / `TRUNKS_DATABASE_URL_FILE` | caminho de um ficheiro com o URL da base de dados (para secrets montados como ficheiro); quando definido **prevalece** sobre o URL; espaços e mudanças de linha à volta são ignorados; ficheiro ilegível ou vazio é erro de arranque (a mensagem indica o ficheiro, nunca o conteúdo) | unset |
| `TRUNKS_METRICS_ENABLED` | `true/false` — liga `/metrics` | on em enterprise, off em dev |
| `TRUNKS_UPSTREAM_INIT_TIMEOUT_SEC` | inteiro ≥1; timeout do `initialize` upstream | 30 |
| `TRUNKS_SESSION_IDLE_TIMEOUT_SEC` | ≥0; 0 = sem expiração | 0 |
| `--request-timeout-sec` / `TRUNKS_REQUEST_TIMEOUT_SEC` | ≥0; limite de um pedido REST e de um pedido a um upstream; 0 = sem limite | 300 |
| `TRUNKS_ALLOWED_HOSTS` | lista de `Host` aceites nos endpoints MCP; `*` desliga | loopback |
| `TRUNKS_IGNORE_SSE_SERVERS` | `1` → ignora servidores `sse` legacy em vez de falhar o arranque | unset |
| `TRUNKS_LOG` (fallback `RUST_LOG`) | filtro `tracing` | `info` |
| `--public-url` / `TRUNKS_PUBLIC_URL` | URL base público (ex. `https://trunks.example.com`); decide o `Secure` do cookie e a origem aceite; **obrigatório com OIDC** (o IdP redirecciona para `<url>/auth/callback`) | unset |
| `TRUNKS_SESSION_TTL_HOURS` | duração das sessões de browser (≥1) | 12 |
| `TRUNKS_OIDC_ISSUER` | issuer OIDC; defini-lo liga o "Sign in with SSO" (só enterprise; em development é erro de arranque) | unset |
| `TRUNKS_OIDC_CLIENT_ID` | client id registado no IdP; obrigatório com issuer | unset |
| `TRUNKS_OIDC_CLIENT_SECRET` | segredo do client (omitir para client público + PKCE) | unset |
| `TRUNKS_OIDC_SCOPES` | scopes pedidos além de `openid` (lista separada por vírgulas) | `profile,email` |
| `TRUNKS_OIDC_GROUPS_CLAIM` | claim com os grupos/roles do utilizador | `groups` |
| `TRUNKS_OIDC_ADMIN_GROUP` | valor do claim que torna o user admin; sem ele o login nunca altera roles | unset |
| `TRUNKS_OIDC_SYNC_TEAMS` | `true` → sincroniza memberships de teams a partir do claim | `false` |
| `TRUNKS_OIDC_TEAM_PREFIX` | só grupos com este prefixo viram teams (o prefixo é retirado) | vazio |
| `TRUNKS_REGISTRY` (CLI) / `--registry` | URL do servidor para os comandos de gestão | `http://127.0.0.1:8080` |

## 10. Observabilidade

Métricas (`prometheus-client`), labels `mcp_server_name`, `tool_name`, `outcome ∈ {success,error}`, valores truncados a 64 chars, vazio → `unknown`:

- `trunks_tool_calls_total` (counter) — também usado para prompts (`tool_name` recebe o nome do prompt).
- `trunks_tool_call_latency_seconds` (histogram, buckets `0.005 … 30`).
- `trunks_build_info{version}`.
- `trunks_upstream_sessions_active{mcp_server_name,mode}` (gauge): sessões de upstream abertas — `mode="stateful"` são as mantidas entre chamadas (0 ou 1 por servidor), `mode="stateless"` as das chamadas em curso. Desce quando a sessão termina, seja como for (fecho, erro, timeout, pedido abandonado).
- `trunks_upstream_connect_errors_total{mcp_server_name}` (counter): tentativas falhadas de abrir uma sessão com um upstream ao servir uma chamada (processo que não arranca, `initialize` falhado ou expirado, autorização em falta). Erros durante o registo de um servidor não contam.

Logs: `tracing` com `target` por subsistema (`trunks::registry`, `trunks::gateway`, `trunks::sessions`, `upstream.stderr`). Nunca logar tokens, `Authorization`, `env` de stdio, `client_secret`. Access log via `tower_http::trace` ao nível `info`. Em `debug`/`trace` as dependências (rmcp, sqlx) registam argumentos e resultados de tools e o texto das queries — nunca credenciais (verificado por `log_hygiene`), mas pode ser conteúdo sensível do utilizador: não usar esses níveis em produção sem necessidade. Quando o filtro activa `debug` ou `trace`, o arranque emite um `warn` a dizê-lo.

## 11. Segurança

- Todos os segredos em `SecretString`; `Debug` redigido; serialização só em `GET /server_configs` (admin) e na resposta de criação de user/client.
- Headers downstream nunca chegam ao upstream (o `UpstreamConn` é construído sem acesso ao pedido).
- Fail-closed por tipo: `Principal` ausente ⇒ vazio/negado.
- Inicialização: `POST /init` é público por natureza (ainda não há identidades) e só aceita o modo com que o processo arrancou — um servidor enterprise por inicializar não pode ser transformado em development (sem autenticação) por quem lá chegar primeiro. Quem inicializa fica com o token de administrador: `trunks init-server` corre-se antes de expor o servidor.
- Sessões de browser: cookie `HttpOnly; SameSite=Lax` (+`Secure` em https), só o SHA-256 em DB, verificação de `Origin` em mutações autenticadas por cookie, nunca aceites nos endpoints MCP. ID tokens só com algoritmos assimétricos; `state` de uso único; `client_secret`, `code_verifier`, ID/access tokens do IdP em `SecretString` e nunca em logs.
- DNS rebinding: `allowed_hosts` loopback por defeito.
- Processos stdio: process group próprio (Unix) ou job object (Windows), mortos em árvore no fim da sessão; sem herdar stdin; stderr por pipe, registado com os valores de `env` redigidos, 4 KiB por linha e caracteres de controlo escapados (§4.7) — nunca escrito directamente no stderr do Trunks.
- Limites: corpo ≤ 4 MiB, prazo por pedido (`TRUNKS_REQUEST_TIMEOUT_SEC`). Clientes lentos: 30 s para o cabeçalho, 30 s de pausa máxima no corpo MCP (§5).
- `cargo deny` (advisories, licenças permissivas), `cargo audit` no CI.

## 12. Testes

| Nível | Onde | Cobertura |
|---|---|---|
| Fuzz | `fuzz/` (cargo-fuzz, nightly; workspace próprio) + `trunks-types/tests/names_random.rs` | invariantes dos parsers de nomes e URIs (`trunks-types/tests/invariants`, partilhadas): nunca entram em pânico, parse ∘ print = identidade, qualquer nome/URI não vazio sobrevive ao round-trip. O teste com seed corre em todo o CI; o workflow `fuzz` corre 60 s por target nos PRs que tocam nos parsers e 1 h por target a pedido |
| Processo | `trunks/tests/server_process.rs` | o binário real: o stderr de um upstream stdio que imprime um segredo aparece no output do servidor redigido e sem caracteres de controlo; `SIGTERM` termina com exit 0 |
| Árvore de processos | `trunks-core/tests/gateway.rs` | um upstream stdio que lança um processo próprio que nunca termina: fechar ou largar a ligação mata-o (o descendente segura um socket do teste, que fecha quando ele morre) |
| Desempenho | `crates/trunks/benches/gateway.rs` (`cargo bench -p trunks --bench gateway`) | latência ponta-a-ponta contra o binário optimizado: `tools/list` com 500 tools e `tools/call` por sessões stateless/stateful, stdio e HTTP, ao lado da chamada directa ao upstream. Resultados e leitura em `07-desempenho.md`. Não corre no CI (ruído); `clippy --all-targets` garante que compila. Guarda de regressão no CI: `server_process.rs` exige que a mais rápida de 20 chamadas pequenas, numa ligação reutilizada, fique abaixo de 30 ms |
| Accept loop | `trunks-api/tests/serve.rs` | cabeçalho que nunca acaba, ligação muda e prefácio HTTP/2 incompleto são desligados; shutdown deixa acabar pedidos em curso e não espera para lá do prazo |
| Higiene de logs | `trunks-api/tests/log_hygiene.rs` | sessão enterprise completa com todos os níveis de log ligados (incl. `TRACE` de dependências): nenhum token, bearer, header, `env`, cookie de sessão, `code` ou `state` aparece no output |
| Unit | cada crate | parsing de nomes/URIs (tabela de casos §4.1), resolução de grupos, filtragem por allow-list, classificação de erros, token gen/validate, `summarize_server_config` |
| Integration (core) | `trunks-core/tests` com stores em memória + upstream `rmcp` em processo | registo/deregisto/enable cascata, stateless vs stateful, invalidação, OAuth com `wiremock` como authorization server |
| API | `trunks-api/tests` com `axum::Router` + SQLite `:memory:` | cada rota, códigos, shapes, middleware; login OIDC contra um IdP falso (`trunks_testing::oidc::FakeIdp`: discovery, JWKS, token, userinfo) incl. rotação de chaves, `aud`/assinatura/expiração/`nonce`/algoritmo simétrico rejeitados, `state` de uso único, CSRF por `Origin` |
| E2E / conformidade | `crates/trunks/tests/conformance` (target `conformance`): o binário real (`trunks start`) numa base de dados vazia, conduzido de fora pelo CLI, pela REST API e por um cliente MCP, com o `fake-upstream` (stdio e HTTP) | matriz **{development, enterprise} × {sqlite, postgres}**, uma célula por teste. *Development*: registo, listagens, invocação por CLI/REST/MCP, enable/disable, tool groups e o seu endpoint MCP, persistência após reinício, deregisto. *Enterprise*: inicialização, 401/403, users, teams e grants (nada sem grant; equipa; grant directo; revoke), MCP clients (allow-list; cliente com dono nunca vê mais do que o dono), sessão de browser (cookie, origem, nunca em MCP), persistência após reinício (tokens, grants, sessões), logout e cascatas de eliminação. As células PostgreSQL precisam de `TRUNKS_TEST_PG_URL` (servidor onde possam criar bases de dados) e são saltadas sem ela — excepto com `TRUNKS_TEST_REQUIRE_PG`, como no job `test-postgres` do CI, onde falham. OAuth de upstream e OIDC estão cobertos nos testes de API (`trunks-api/tests`), não nesta matriz; a web app entra com a 6c. **Sem harness diferencial**: não há binário legacy disponível para comparar; a referência é esta especificação |
| Snapshot | `insta` | saída do CLI (`list`, `usage`, `get group`) |
| CLI | `trunks/src/cli_parsing.rs`, `trunks/src/cli_reference.rs` | valores começados por `-` (tokens, segredos, argumentos de um comando stdio) são valores e as flags à volta continuam a ser flags; `docs/guide/reference/cli.md` é igual ao `--help` de todos os comandos (regenerar: `TRUNKS_UPDATE_DOCS=1 cargo test -p trunks cli_reference`), sem valores de variáveis de ambiente |

## 13. Build e release

- `cargo-dist` (D9) para `x86_64/aarch64 × linux-gnu/linux-musl/darwin/windows`; archives `trunks-<target>.tar.xz` (zip em Windows), `sha256.sum`, instaladores `shell`/`powershell`. Sem Homebrew (D9).
- Docker: um `Dockerfile` multi-stage com dois targets sobre o mesmo stage de build (`rust:1.97-bookworm`, compilador o de `rust-toolchain.toml`): `distroless` (por defeito; `gcr.io/distroless/cc-debian12:nonroot`) e `stdio` (`ghcr.io/astral-sh/uv:python3.13-bookworm-slim` + Node.js 24 — o Node 20 previsto chegou ao fim de vida —, utilizador `trunks`, uid 10001). Nenhuma corre como root; dados em `/data`; `TRUNKS_DATABASE_URL=sqlite:///data/trunks.db` por defeito. Imagens `ghcr.io/websublime/trunks`, tags `vX.Y.Z`, `latest`, `vX.Y.Z-stdio`, `latest-stdio` (`latest*` só em releases finais), `linux/amd64` e `linux/arm64`.
- Workflow `docker`: em PRs que tocam no `Dockerfile` (e a pedido) constrói os dois targets nas duas arquitecturas, em runners nativos, e faz um smoke test (versão, utilizador não-root, `/health`, API em development, paragem limpa com `SIGTERM`, `node`/`npx`/`uvx`/`python3` na `stdio`) sem publicar; em tags `vX.Y.Z*` faz o mesmo, publica por arquitectura e cria os manifestos multi-arquitectura.
- Versão reportada por `trunks version`, `/metadata` e `trunks_build_info`: `v<versão do pacote>` num build do commit com a tag `v<versão>`; `v<versão>+<commit>` (7 caracteres; `+<commit>.dirty` com alterações por commitar) em qualquer outro build feito a partir de um checkout git; só `v<versão>` quando não há checkout (arquivo de código, build das imagens). Calculado por `crates/trunks/build.rs` com o `git` do sistema, sem dependências novas.
- `docker-compose.yaml` (development, SQLite, imagem `-stdio`, porta em `127.0.0.1`) e `docker-compose.prod.yaml` (enterprise + PostgreSQL 16, variáveis obrigatórias a partir de `.env`; modelo em `.env.example`).
- `CHANGELOG.md` (Keep a Changelog). Documentação de utilizador em `docs/guide/` (inglês, Markdown no repositório, sem gerador de site): quickstart, conceitos, upstreams, tool groups, modo enterprise, ligação de clientes, operações e referências (CLI, configuração, API HTTP). A referência do CLI (`docs/guide/reference/cli.md`) é **gerada** a partir das definições `clap` e um teste falha quando deixa de coincidir com o `--help` (§12).

## 14. Riscos técnicos e como se desriscam

| Risco | Mitigação |
|---|---|
| ~~`rmcp` não propaga `http::request::Parts`/extensions para o `ServerHandler`~~ | **Resolvido** — Spike S1 (`docs/spikes/S1-rmcp-principal.md`): propaga por pedido em ambos os modos de sessão; cliente `rmcp` e `mcp-remote` verificados. |
| ~~`rmcp` `auth` não cobre DCR ou não deixa controlar `state`~~ | **Resolvido** — Spike S3: cobre DCR, PKCE, `state` via `StateStore` nosso, refresh e `AuthClient` (`docs/spikes/S3-upstream-oauth.md`). |
| ~~Camada de DB sobre DBs legacy~~ | **Resolvido** — `trunks-db` adopta as fixtures legacy nos dois backends (migrações `IF NOT EXISTS` + `normalize_legacy`, testes em CI com Postgres 16). |
| Protocolo: clientes antigos (2025-xx) vs `rmcp` 3.8 default 2026-07-28 | `legacy_session_mode = true` no `StreamableHttpServerConfig` para servir clientes anteriores com sessão; testar com `mcp-remote`, Cursor, Claude Code. |
| SSE (D4) | Decisão explícita antes da fase 3. |
| Regressão silenciosa de contrato | Suite de conformidade escrita **antes** de implementar cada área (TDD por contrato). |
