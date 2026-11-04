# Trunks — Especificação Técnica

> Arquitectura alvo em Rust para a reescrita do MCPJungle. Versão 1.1 (2026-11-03).
> Pressupõe as decisões D1–D11 de `01-produto.md`. D11 (lib de acesso a dados) está em análise; a spec descreve a camada de dados de forma agnóstica à lib.
> Contrato externo (REST, CLI, DB) é **compatível** com MCPJungle 0.4.x salvo onde marcado **[BREAKING]**.

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

Diferenças estruturais face ao original (todas internas, sem efeito no contrato):

| Original (Go/mcp-go) | Trunks (Rust/rmcp) |
|---|---|
| 2 proxies globais + 2 por grupo, cada um um `MCPServer` com registo próprio de tools e handler por tool | **1 `GatewayHandler`** parametrizado por `Scope::Global \| Scope::Group(name)`; `list_tools`/`call_tool` consultam o `Catalog` e filtram |
| Callbacks de adição/remoção de tools para sincronizar grupos | Grupos são **vistas** sobre o catálogo; nada a sincronizar |
| `GetMcpServer` à DB em cada chamada | `Catalog` em memória (`ArcSwap<CatalogSnapshot>`), reconstruído após cada mutação do registry |
| Principal via `ctx.Value("mode")`/`"client"` sem verificação | `Principal` tipado injectado por middleware; o handler **não compila** sem ele |
| `sync.RWMutex` global no `SessionManager` segurado durante o connect | `DashMap<ServerName, Arc<Mutex<Slot>>>` — lock por servidor |
| Novo `StreamableHTTPServer` por pedido nos grupos | Um `StreamableHttpService` por scope, criado on-demand e cacheado |

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
│   └── trunks-dashboard/       # rust-embed do bundle React (web/dashboard/dist)
├── web/dashboard/             # React/TS inalterado (copiado do original)
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
| MCP | `rmcp` | 3.8 | features: `server`, `client`, `transport-streamable-http-server`, `transport-streamable-http-client-reqwest`, `transport-child-process`, `auth`, `schemars` |
| HTTP client | `reqwest` | 0.12 | `rustls-tls`, `json`, `stream` |
| DB | **em análise (D11)** — sem ORM | — | candidatos: `sqlx` (SQL manual, `Any` ou dois backends), `sqlx` + `sea-query` (query builder, não ORM), `rusqlite` + `tokio-postgres`. Requisitos: SQLite bundled (sem libsqlite3 do sistema) e Postgres via rustls; JSON como `serde_json::Value`; migrações versionadas idempotentes |
| CLI | `clap` | 4 | derive, `env` |
| Serialização | `serde`, `serde_json` | — | `serde_json::Value` para JSON opaco (schemas, configs) |
| Config | `dotenvy` | — | precedência implementada à mão (ver §9) |
| Erros | `snafu` | 0.8 | context selectors por crate; `ensure!`; `Whatever` só no binário; `Report` para saída humana no CLI |
| Logging | `tracing`, `tracing-subscriber` | — | `fmt` (dev) / `json` (enterprise); `tower-http::trace` para access log |
| Métricas | `prometheus-client` | 0.23 | registry próprio; endpoint `/metrics` em text format. OTel SDK **não** é necessário para o que o original expõe. |
| Secrets | `secrecy` | — | `SecretString` para tokens/bearers; `Debug` redigido |
| Embed | `rust-embed` | — | dashboard |
| Random/crypto | `rand`, `base64`, `sha2`, `hex` | — | tokens, PKCE, URIs |
| Concorrência | `arc-swap`, `dashmap`, `tokio-util` (`CancellationToken`) | — | |
| Testes | `tokio::test`, `wiremock`, `assert_cmd`, `insta` | — | upstream fake MCP via `rmcp` server em processo |

Toolchain: Rust stable ≥ 1.85 (edition 2024). `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo deny check` (licenças + advisories) obrigatórios no CI.

## 4. Domínio (`trunks-core`)

### 4.1 Newtypes e enums (em `trunks-types`)

```rust
pub struct ServerName(String);        // validado: ^[a-zA-Z0-9_-]+$, sem "__", não termina em "_"
pub struct CanonicalName { server: ServerName, local: String }   // Display = "{server}__{local}"
impl CanonicalName { pub fn parse(s: &str) -> Result<Self>  /* split no PRIMEIRO "__" */ }
pub struct ResourceUri { server: ServerName, original: String }  // Display = "mcpj://res/{server}/{b64url_nopad(original)}"
pub struct GroupName(String);         // ^[a-zA-Z0-9][a-zA-Z0-9_-]*$

#[serde(rename_all = "snake_case")] pub enum Transport { Stdio, StreamableHttp, Sse }
#[serde(rename_all = "snake_case")] pub enum SessionMode { #[default] Stateless, Stateful }
#[serde(rename_all = "lowercase")]   pub enum Role { Admin, User }
#[serde(rename_all = "lowercase")]   pub enum ServerMode { Development, Enterprise }
// "production" aceite na desserialização e normalizado para Enterprise
```

`Sse` mantém-se no enum para desserializar DBs existentes; com D4=(a) o arranque falha com erro explícito se existir um servidor `sse` activo (ver `03-migracao.md`).

### 4.2 Modelo de dados

Idêntico ao schema GORM (nomes de tabelas e colunas), para D3. Todas as tabelas têm `id BIGSERIAL/INTEGER PK`, `created_at`, `updated_at`, `deleted_at NULL` (soft-delete herdado; Trunks faz hard-delete como o original faz com `Unscoped()`, mas respeita `deleted_at IS NULL` nas leituras).

| Tabela | Colunas relevantes | Notas |
|---|---|---|
| `mcp_servers` | `name UNIQUE`, `transport varchar(30)`, `enabled bool default true`, `description`, `config JSON NOT NULL`, `session_mode varchar(20) default 'stateless'` | `config` é `StreamableHttpConfig{url, bearer_token?, headers?}` \| `StdioConfig{command, args?, env?}` \| `SseConfig{url, bearer_token?}` |
| `tools` | `name` (bare), `enabled`, `description`, `input_schema JSON`, `annotations JSON`, `server_id FK` | sem UNIQUE(server_id,name) no original; Trunks **adiciona** índice único (migração aditiva, segura) |
| `prompts` | `name`, `enabled`, `description`, `arguments JSON`, `server_id` | idem |
| `resources` | `uri` (público mcpj), `original_uri`, `name`, `enabled`, `description`, `mime_type`, `annotations JSON`, `meta JSON`, `server_id` | idem, UNIQUE(uri) |
| `server_configs` | `mode varchar(12)`, `initialized bool` | 0 ou 1 linha |
| `users` | `username UNIQUE`, `role`, `access_token UNIQUE` | plaintext na v1 (compat); hash em D8 |
| `mcp_clients` | `name UNIQUE`, `description`, `access_token UNIQUE`, `allow_list JSON NOT NULL` | `["*"]` ou lista de nomes |
| `tool_groups` | `name UNIQUE`, `description`, `included_tools JSON`, `included_servers JSON`, `excluded_tools JSON` | |
| `upstream_oauth_pending_sessions` | `session_id UNIQUE` (64 hex), `server_name IDX`, `transport`, `server_input JSON`, `force`, `redirect_uri`, `client_id`, `client_secret`, `scopes JSON`, `state`, `code_verifier`, `expires_at IDX`, `initiated_by` | TTL 10 min |
| `upstream_oauth_tokens` | `server_name UNIQUE`, `transport`, `client_id`, `client_secret`, `redirect_uri`, `scopes JSON`, `access_token`, `token_type`, `refresh_token`, `scope`, `expires_at` | |

JSON: Postgres `jsonb`; SQLite `TEXT`. Lido/escrito como `serde_json::Value` em ambos (em Postgres com cast `::jsonb` no SQL se a lib escolhida não tipar JSON — ver D11).

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
pub async fn set_tools_enabled(&self, entity: &str, enabled) -> Result<Vec<CanonicalName>>;   // entity = server | canonical
pub async fn set_prompts_enabled(...); pub async fn set_resources_enabled(...);
```

Invariantes a preservar (do original, verificadas por testes):

1. `register`: validar nome e URL → **ligar e `initialize` ao upstream** (timeout `MCP_SERVER_INIT_REQ_TIMEOUT_SEC`) → só depois persistir. Falha de connect = nada persistido.
2. Descoberta: `tools/list` obrigatório (falha → falha o registo); `prompts/list` e `resources/list` só se o upstream anunciar a capability e são best-effort (warn, não falham).
3. **Nova vs original:** registo e deregisto correm numa **transacção** (servidor + entidades filhas + tokens/sessões OAuth). O original não era transaccional — melhoria sem efeito no contrato.
4. `force=true`: deregista o existente antes de registar; persistido na sessão OAuth pendente para repetir no `complete`.
5. Enable/disable por entidade devolve só os nomes que mudaram de estado no nível servidor, e `[entity]` sempre no nível individual (mesmo se já estava nesse estado).
6. Resource enable/disable aceita nome de servidor **ou** URI `mcpj://`; outra string → `InvalidInput`.
7. Headers `Authorization` explícitos ganham sobre `bearer_token` (log `warn!` com o texto exacto do original).

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

**Principal** — extraído de `ctx.extensions` (rmcp propaga `http::request::Parts`; o middleware axum insere `Principal` em `Parts.extensions` — a verificar no Spike S1 do plano):

```rust
pub enum Principal { DevOpen, McpClient(Arc<McpClientRecord>), /* nunca User aqui */ }
impl Principal { fn may_access(&self, server: &ServerName) -> bool }
```

Ausência de `Principal` em modo enterprise → `list_*` devolve vazio e `call_*` devolve erro `-32603 "not authorized"` — **fail-closed**, equivalente ao `ProxyToolFilter` do original. Em modo dev, `Principal::DevOpen`.

Regras de filtragem (paridade exacta):
- `list_tools`: filtra por allow-list. Tool cujo nome não tem `__` é negada.
- `list_prompts`, `list_resources`: **não filtradas** em enterprise (comportamento do original — assinalado como melhoria D8, mas a v1 preserva porque os e2e do original o asseguram).
- `call_tool`/`get_prompt`/`read_resource`: verificam allow-list **antes** de criar qualquer sessão upstream.
- `Scope::Group` restringe `list_tools`/`call_tool` às effective tools; `list_prompts`/`list_resources` em scope de grupo devolvem **vazio** (grupos só expõem tools — paridade).

Encaminhamento de uma chamada:
1. `CanonicalName::parse` → `(server, local)`; não encontrado no catálogo → `-32602` (equivalente a tool desconhecida).
2. Autorização.
3. `start = Instant::now()`; `session = sessions.acquire(server).await?`.
4. Reescrever `params.name = local` (ou `params.uri = original`); **descartar todos os headers/meta do pedido downstream** — o `UpstreamConn` só conhece as credenciais configuradas.
5. `session.call_tool(params).await`; em erro, `session.mark_unhealthy_if_connection_error(&err)`.
6. Resources: reescrever `contents[*].uri` original → `mcpj://` na resposta.
7. `metrics.record(server, local, outcome, elapsed)`; `session.release()` (fecha se stateless).

### 4.6 Sessões upstream

```rust
pub struct SessionManager {
    slots: DashMap<ServerName, Arc<tokio::sync::Mutex<Option<Arc<UpstreamConn>>>>>,
    idle_timeout: Option<Duration>,    // None = nunca expira (SESSION_IDLE_TIMEOUT_SEC ∈ {0,-1})
    init_timeout: Duration,            // MCP_SERVER_INIT_REQ_TIMEOUT_SEC, default 30s
    cancel: CancellationToken,
}
pub struct UpstreamConn { peer: rmcp::service::RunningService<RoleClient, ()>, last_used: AtomicU64, kind: ConnKind }
```

- `acquire(server)`: `Stateless` → nova `UpstreamConn` (dropped no `release`). `Stateful` → lock **do slot desse servidor**, reutiliza ou cria. O connect de um servidor lento não bloqueia os outros (correcção face ao original).
- Sweeper: `tokio::time::interval(60s)` fecha slots com `now - last_used > idle_timeout`.
- `invalidate(server)` no deregister; `shutdown()` fecha tudo (chamado antes do drain HTTP).
- Classificação de erro de ligação: `rmcp::ServiceError::Transport(_)`, `TransportClosed`, `Timeout`, erros `std::io` com kinds `ConnectionRefused/Reset/Aborted/BrokenPipe/UnexpectedEof/TimedOut` → invalidar. **Não** se faz substring matching em mensagens (o original invalidava com qualquer erro contendo "timeout" ou "transport").
- Sem retry automático da chamada falhada (paridade).

### 4.7 Ligações a upstreams (`UpstreamConn::connect`)

| Transporte | Implementação |
|---|---|
| `streamable_http` | `rmcp::transport::StreamableHttpClientTransport` com `reqwest::Client` por servidor: headers configurados + `Authorization: Bearer` (se não houver `Authorization` explícito). Se existir `upstream_oauth_tokens` para o servidor → wrapper `AuthClient` do módulo `rmcp::transport::auth` com `CredentialStore` persistente em DB (refresh automático). `initialize` com `tokio::time::timeout(init_timeout)`; erro mapeado para mensagem que menciona `MCP_SERVER_INIT_REQ_TIMEOUT_SEC`; `ConnectionRefused` + URL loopback → hint `host.docker.internal`. |
| `stdio` | `rmcp::transport::TokioChildProcess` sobre `tokio::process::Command`: `env` configurado **sobre** o ambiente herdado do processo (comportamento do `exec.Command` com `cmd.Env = append(os.Environ(), ...)` — confirmar no spike o que mcp-go faz; documentar a escolha), `kill_on_drop(true)`, `process_group(0)` em Unix para matar descendentes. Stderr capturado por task que faz `tracing::warn!(target="upstream.stderr", server=%name, "{chunk}")`. `clientInfo = {name: "trunks upstream client", version}`. |
| `sse` | **[BREAKING, D4=(a)]** não implementado. `connect` devolve `UnsupportedTransport`. |

### 4.8 OAuth upstream

Paridade com o fluxo de duas fases do original, implementado sobre `rmcp::transport::auth` (`AuthorizationManager`, PKCE, discovery de metadata, Dynamic Client Registration) — confirmar no Spike S3 que `rmcp` expõe DCR e a construção da authorization URL com `state` nosso; caso contrário, implementar os ~200 LOC em cima de `oauth2` crate.

Fase 1 (`register` falha com 401/`WWW-Authenticate` → `AuthorizationRequired`):
1. `oauth_redirect_uri` obrigatório → senão `InvalidInput` + `code = "upstream_oauth_required"` (o CLI usa este código para arrancar o listener loopback).
2. Discovery; se `client_id` vazio → DCR com body `{client_name: "trunks-{server}", redirect_uris, token_endpoint_auth_method: "none"|"client_secret_basic", grant_types: ["authorization_code","refresh_token"], response_types: ["code"], scope?}` (`scope` omitido se vazio); sem `registration_endpoint` ou 403 → erro que instrui a fornecer `oauth_client_id`/`oauth_client_secret`.
3. Gerar `state` (32 bytes hex), `code_verifier`/`challenge` (S256), `session_id` (32 bytes hex = 64 chars).
4. Transacção: apagar sessão pendente anterior do mesmo servidor; inserir nova com `expires_at = now + 10 min`.
5. Devolver `202 {authorization_required: {session_id, authorization_url, expires_at}}`.

Fase 2 (`POST /api/v0/upstream_oauth/sessions/{id}/complete {code, state}`): validar presença → carregar sessão (`NotFound`) → expirada (`InvalidInput`) → `state` diferente (`InvalidInput`) → upsert metadata em `upstream_oauth_tokens` sem tocar nos tokens → troca do code (grava tokens via `CredentialStore`) → se `force`, deregistar existente → registar com credenciais guardadas → apagar sessão pendente → `201 {server}`.

`CredentialStore` em DB: `get` devolve `None` se não há linha ou `access_token` vazio; `save` preserva `client_id/secret/redirect_uri/scopes`.

### 4.9 Identidades e tokens

- `generate_token()` → 32 bytes CSPRNG, base64url sem padding (43 chars). `validate_custom_token(s)`: `len ≥ 8` && sem whitespace Unicode.
- `UserService`: `create_admin()` (só via `/init`), `create_user(name, token?)` role sempre `user`, `update_token`, `delete` (admin → `InvalidInput`), `by_token`.
- `McpClientService`: `create(name, desc, token?, allow_list?)` (nil → `[]`), `update_token`, `delete` idempotente, `by_token`. `allow_list` malformada → sem acesso.
- v1: comparação exact-match em plaintext (compat com DB existente). D8: coluna `access_token_hash`, migração que hasheia e limpa `access_token`.

### 4.10 Tool groups

```rust
pub struct ToolGroupSpec { name: GroupName, description: String, included_tools: Vec<String>, included_servers: Vec<String>, excluded_tools: Vec<String> }
```
- `create`: validar nome → resolver contra o **catálogo actual** → `len == 0` → `InvalidInput`; tool incluída inexistente/desactivada → `InvalidInput "…does not exist or is disabled"` → insert → rebuild catálogo.
- `update`: diff old/new (`added`/`removed` por campo) → validar adicionadas → **update completo da linha incluindo listas vazias** (corrige o bug GORM `Updates` do original) → rebuild. Devolve `{name, old, new}`.
- `delete`: idempotente (paridade: não dá 404).
- Endpoints: `GET /v0/groups/{g}/mcp` montado dinamicamente — o router tem um handler em `/v0/groups/{name}/mcp` que resolve o grupo no catálogo (404 `{"error":"tool group not found: {name}"}`) e despacha para um `StreamableHttpService<GatewayHandler{Group}>` cacheado em `DashMap<GroupName, _>` (invalidado no delete). `/v0/groups/{g}/sse|message`: **[BREAKING]** não existem (D4).

## 5. API HTTP (`trunks-api`)

Compatibilidade total com o original em paths, métodos, códigos, shapes JSON e mensagens de erro. Resumo (detalhe completo com shapes em `03-migracao.md` §3):

| Grupo | Middleware | Rotas |
|---|---|---|
| Público | — | `GET /health` → `{"status":"ok"}`; `GET /metadata` → `{"version"}`; `POST /init {mode}` → dev `{"status"}` / enterprise `{"status","admin_access_token"}`; já inicializado → 400; `GET /metrics` (se activo) |
| Dashboard static | `require_initialized`, `require_dev_mode` (404) | `GET /`, `/index.html`, `/assets/*` |
| MCP | `require_initialized`, `mcp_client_auth` | `ANY /mcp`, `ANY /v0/groups/{name}/mcp` |
| REST user | `require_initialized`, `user_auth` | `GET /api/v0/servers`, `/tools?server=`, `/tool?name=`, `POST /tools/invoke`, `GET /resources?server=`, `POST /resources/get`, `/resources/read`, `GET /prompts?server=`, `/prompt?name=`, `POST /prompts/render`, `GET /users/whoami` (+enterprise) |
| REST admin | + `require_admin` | `POST /servers?force=`, `POST /upstream_oauth/sessions/{id}/complete`, `DELETE /servers/{name}`, `POST /servers/{name}/enable\|disable`, `GET /server_configs`, `POST /tools/enable\|disable?entity=`, `POST /prompts/enable\|disable?entity=`, `GET/POST /clients`, `PUT/DELETE /clients/{name}` (+enterprise), `GET/POST /users`, `PUT/DELETE /users/{username}` (+enterprise), `GET/POST /tool-groups`, `GET/PUT/DELETE /tool-groups/{name}`, `GET /tool-groups/{name}/effective-tools` |
| Dashboard API | `require_initialized`, `require_dev_mode` | `/api/dashboard/{overview,servers,tools,tool-groups,prompts,resources,diagnostics}`, `POST /servers`, `DELETE /servers/{n}`, `PATCH /{servers\|tools\|prompts}/{n}/enabled`, `POST/DELETE /tool-groups[/{n}]`, `GET /tool-groups/{n}`, `GET /oauth/callback` (HTML), `GET /oauth/session/{id}` |

Erros: `{"error": string, "code"?: string}`. Mapeamento `RegistryError → StatusCode`: `NotFound→404`, `InvalidInput→400` (+`code:"upstream_oauth_required"` quando aplicável), `Unauthorized→401`, `Forbidden→403`, resto `→500` com a mensagem. Mensagens que testes/CLI dependem: `"server is not initialized"` (403), `"missing access token"`, `"invalid access token: …"`, `"user is not authorized to perform this action"`, `"this request is only allowed in enterprise mode"`, `"missing MCP client access token"`, `"invalid MCP client token"`, `"Server is already initialized"`, `"name is required"`, `"missing 'name' query parameter"`, `"tool group not found: {name}"`.

Middleware:
- `require_initialized`: lê `ServerConfig` de um `watch::Receiver<Option<ServerMode>>` em memória (actualizado por `/init`), não da DB por pedido. 403 se não inicializado. Insere `ServerMode` nas extensions.
- `user_auth`: dev → `Principal::DevOpen`; enterprise → `Authorization: Bearer` → lookup → 401; insere `UserPrincipal`.
- `mcp_client_auth`: dev → `Principal::DevOpen`; enterprise → bearer → lookup em `mcp_clients` (tokens de user são **rejeitados**) → 401; insere `Principal::McpClient`.
- `require_admin`: dev → passa; enterprise → role ≠ admin → 403.
- `require_enterprise`: 403 com a mensagem acima.
- `trace`: access log com método, path, status, latência, `principal.kind` (nunca o token).
- Host/Origin: `StreamableHttpServerConfig.allowed_hosts` por defeito é loopback-only (protecção DNS rebinding — comportamento igual ao mcp-go). Configurável via `MCP_ALLOWED_HOSTS` (lista separada por vírgulas; `*` desliga). **Novo** face ao original, que não permitia configurar.
- Body limit: 4 MiB nos endpoints REST; `max_request_body_bytes` do rmcp no MCP.

Dashboard: `trunks-dashboard` embebe `web/dashboard/dist` com `rust-embed` (`#[folder = "../../web/dashboard/dist"]`); build via `xtask build-dashboard` (`npm ci && npm run build`). Sem SPA fallback (paridade). `summarize_server_config` nunca emite `bearer_token`, valores de `env`, header `authorization` (case-insensitive), userinfo/query/fragment de URLs; `command` truncado a 80 chars.

## 6. SDK (`trunks-client`)

Porte directo de `client/` do original: `Client::new(base_url, token: Option<SecretString>, reqwest::Client)`, um método por endpoint, `ApiError { status, message, code }` via `snafu`. Melhorias sem quebra: todos os métodos `async` recebem `&self`, timeouts configuráveis, **não expõe tipos internos de DB** (`ListPrompts` devolve `trunks_types::Prompt`, não a struct de linha da DB — o SDK Go vazava `model.Prompt`). JSON wire idêntico.

## 7. CLI (`trunks`)

Árvore idêntica ao `mcpjungle` (nomes, flags, mensagens, exit codes 0/1). Ver inventário completo em `03-migracao.md` §4. Pontos de implementação:

- `clap` derive com `#[command(display_order)]` para reproduzir os grupos "basic"/"advanced" do help.
- Flag persistente `--registry` (default `http://127.0.0.1:8080`); precedência flag explícita > `registry_url` do ficheiro > default; TIP impresso quando a flag foi usada e o ficheiro não tem `registry_url`.
- Ficheiro de config `~/.trunks.conf` (YAML `registry_url`, `access_token`), escrito com `0600`; leitura com fallback a `~/.mcpjungle.conf`.
- `register` com fluxo OAuth: listener loopback `127.0.0.1:0` em `/oauth/callback`, abre browser (`open`/`xdg-open`/`rundll32`), espera até `expires_at`, chama `complete`.
- `invoke`: grava `image_*/audio_*/resource_*` no CWD com a mesma tabela de extensões por MIME.
- `init-server` envia `{"mode":"production"}` (compat com servidores antigos) — Trunks aceita ambos.
- `update user <name>` passa a validar `ExactArgs(1)` (bug latente do original).
- Saída: texto plano como o original. `--output json` é melhoria futura, não v1.

## 8. `start` — arranque e shutdown

1. `dotenvy::dotenv().ok()`.
2. Resolver `ServerMode` (flags `--enterprise`/`--prod` > `SERVER_MODE` > dev); `OTEL_ENABLED`/`TRUNKS_METRICS_ENABLED` (default on em enterprise).
3. `tracing` init (`RUST_LOG`, default `info`; JSON em enterprise).
4. DB: `DATABASE_URL` > `POSTGRES_*` (+`_FILE`) > SQLite em `--sqlite-db-path` > `SQLITE_DB_PATH` > `./mcpjungle.db` se existir > `./mcp.db` se existir (warn) > `./trunks.db`. SQLite: `?mode=rwc`, `journal_mode=WAL`, `busy_timeout=5000`. **Migrações versionadas** com a ferramenta da lib escolhida em D11 (ver `03-migracao.md` §5 para a baseline idempotente sobre schema GORM).
5. Verificar modo: se inicializado noutro modo → erro `"server is already initialized in X mode, cannot start in Y mode"`. Se não inicializado e dev → `init(dev)` silencioso; enterprise → imprimir lembrete `init-server`.
6. Validação de compat: se existir `mcp_servers.transport = 'sse'` **enabled** → erro de arranque com instruções (D4), ou warn + skip se `TRUNKS_IGNORE_SSE_SERVERS=1`.
7. Construir `Catalog` (falha de parse de `input_schema` → **erro de arranque**, paridade; `annotations` inválidas → warn + vazio).
8. `SessionManager`, `GatewayState`, métricas, router.
9. Bind `--host`/`MCPJUNGLE_BIND_HOST` (vazio = todas as interfaces) + `--port`/`PORT`/8080. Banner + `"Trunks HTTP server listening on {addr}"`.
10. `SIGINT`/`SIGTERM` → `cancel.cancel()` → `sessions.shutdown().await` → `axum::serve(...).with_graceful_shutdown` com timeout 10 s.

## 9. Configuração — tabela completa

| Fonte | Semântica | Default |
|---|---|---|
| `--enterprise` / `--prod` (deprecado) / `SERVER_MODE` | `development`\|`enterprise`\|`production`→enterprise; case-insensitive | `development` |
| `OTEL_ENABLED` | `true/1/false/0` — liga `/metrics` | on em enterprise, off em dev |
| `DATABASE_URL` | DSN Postgres (ou `sqlite://…`) | — |
| `POSTGRES_HOST`, `POSTGRES_PORT`, `POSTGRES_USER`, `POSTGRES_PASSWORD`, `POSTGRES_DB` (+ `*_FILE` para os 3 últimos; var plana ganha) | monta DSN `postgres://u:p@h:port/db` (URL-escaped) | 5432, postgres, "", postgres |
| `--sqlite-db-path` / `SQLITE_DB_PATH` | caminho SQLite | ver §8.4 |
| `--host` / `MCPJUNGLE_BIND_HOST` | bind host; flag explicitamente vazia força todas as interfaces | todas |
| `--port` / `PORT` | bind port | 8080 |
| `MCP_SERVER_INIT_REQ_TIMEOUT_SEC` | inteiro ≥1; timeout do `initialize` upstream | 30 |
| `SESSION_IDLE_TIMEOUT_SEC` | ≥0 ou -1; 0 e -1 = sem expiração | -1 |
| `MCP_ALLOWED_HOSTS` **(novo)** | lista de `Host` aceites no `/mcp`; `*` desliga | loopback |
| `RUST_LOG` **(novo)** | filtro `tracing` | `info` |
| `TRUNKS_IGNORE_SSE_SERVERS` **(novo)** | `1` → ignora servidores `sse` em vez de falhar o arranque | unset |
| `TRUNKS_METRICS_LEGACY_NAMES` **(novo)** | `1` → expõe também `mcpjungle_*` | unset |

## 10. Observabilidade

Métricas (`prometheus-client`), labels `mcp_server_name`, `tool_name`, `outcome ∈ {success,error}`, valores truncados a 64 chars, vazio → `unknown`:

- `trunks_tool_calls_total` (counter) — também usado para prompts (paridade: `tool_name` recebe o nome do prompt).
- `trunks_tool_call_latency_seconds` (histogram, buckets `0.005 … 30` iguais ao original).
- **Novos:** `trunks_upstream_sessions_active{mcp_server_name,mode}` (gauge), `trunks_upstream_connect_errors_total{mcp_server_name}`, `trunks_build_info{version}`.
- Com `TRUNKS_METRICS_LEGACY_NAMES=1`, duplicar as duas primeiras como `mcpjungle_tool_calls_ratio_total` / `mcpjungle_tool_call_latency_seconds`.

Logs: `tracing` com `target` por subsistema (`trunks::registry`, `trunks::gateway`, `trunks::sessions`, `upstream.stderr`). Nunca logar tokens, `Authorization`, `env` de stdio, `client_secret`. Access log via `tower_http::trace` ao nível `info`.

## 11. Segurança

- Todos os segredos em `SecretString`; `Debug` redigido; serialização só em `GET /server_configs` (admin) e na resposta de criação de user/client (paridade).
- Headers downstream nunca chegam ao upstream (o `UpstreamConn` é construído sem acesso ao pedido).
- Fail-closed por tipo: `Principal` ausente ⇒ vazio/negado.
- DNS rebinding: `allowed_hosts` loopback por defeito.
- Processos stdio: process group próprio, `kill_on_drop`, stderr limitado a 4 KiB por chunk, sem herdar stdin.
- `cargo deny` (advisories, licenças permissivas), `cargo audit` no CI.

## 12. Testes

| Nível | Onde | Cobertura |
|---|---|---|
| Unit | cada crate | parsing de nomes/URIs (tabela de casos do original §4.1), resolução de grupos, filtragem por allow-list, classificação de erros, token gen/validate, `summarize_server_config` |
| Integration (core) | `trunks-core/tests` com stores em memória + upstream `rmcp` em processo | registo/deregisto/enable cascata, stateless vs stateful, invalidação, OAuth com `wiremock` como authorization server |
| API | `trunks-api/tests` com `axum::Router` + SQLite `:memory:` | cada rota, códigos, shapes, middleware (porta dos `*_test.go` de `internal/api`) |
| E2E / conformidade | `tests/conformance` com `assert_cmd` contra o binário, `@modelcontextprotocol/server-everything` via `npx` | porta de `internal/e2e` + `scripts/test-mcpjungle.sh` + `scripts/test-api-error-responses.sh`; matriz {dev, enterprise} × {sqlite, postgres (testcontainers)} |
| Snapshot | `insta` | saída do CLI (`list`, `usage`, `get group`) |

## 13. Build e release

- `cargo-dist` (D9) para `x86_64/aarch64 × linux-gnu/linux-musl/darwin/windows`; archives `trunks-<target>.tar.xz` (zip em Windows), `sha256.sum`, instaladores `shell`/`powershell`, Homebrew tap (`dist` gere a formula).
- Docker: `Dockerfile` multi-stage (`rust:1.xx-bookworm` → `gcr.io/distroless/cc`, ou musl → `distroless/static`); `stdio.Dockerfile` sobre `ghcr.io/astral-sh/uv:debian` + Node 20 (idêntico). Tags `vX`, `latest`, `vX-stdio`, `latest-stdio`; multi-arch.
- Versão via `env!("CARGO_PKG_VERSION")` + `git describe` injectado por `build.rs` (`vergen`); `/metadata` devolve `v`-prefixed.
- `docker-compose.yaml` e `docker-compose.prod.yaml` mantêm-se com a imagem trocada.

## 14. Riscos técnicos e como se desriscam

| Risco | Mitigação |
|---|---|
| `rmcp` não propaga `http::request::Parts`/extensions para o `ServerHandler` | Spike S1. Plano B: `StreamableHttpService` por principal (cache por token hash) — custa memória mas funciona. |
| `rmcp` `auth` não cobre DCR ou não deixa controlar `state` | Spike S3. Plano B: `oauth2` crate directo (~200 LOC). |
| Lib de DB (D11): JSON em SQLite (`TEXT`) vs `jsonb` Postgres sobre DB criada por GORM; placeholders `?` vs `$1`; `RETURNING` | Análise D11 + Spike S2: abrir DB real do MCPJungle (sqlite e pg) com a lib candidata; baseline migration só `CREATE TABLE IF NOT EXISTS`. |
| Protocolo: clientes antigos (2025-xx) vs `rmcp` 3.8 default 2026-07-28 | `legacy_session_mode = true` no `StreamableHttpServerConfig` para servir clientes anteriores com sessão; testar com `mcp-remote`, Cursor, Claude Code. |
| SSE (D4) | Decisão explícita antes da fase 3. |
| Perda de paridade silenciosa | Suite de conformidade portada **antes** de implementar cada área (TDD por contrato). |
