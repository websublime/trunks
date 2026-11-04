# Trunks — Documento de Migração (MCPJungle Go → Trunks Rust)

> Versão 1.2 (2026-11-04). Base: `mcpjungle/mcpjungle@49bf232`.
> Dois públicos: (1) quem **implementa** o rewrite — mapeamento pacote-a-pacote e contratos a preservar; (2) quem **opera** um MCPJungle e vai trocar para Trunks — §6.

---

## 1. Princípios da migração

1. **Contrato antes de código.** Para cada área, porta-se primeiro o teste (Go → Rust/conformidade) e só depois a implementação. O original tem 17.6k linhas de testes; são a especificação executável.
2. **Compatibilidade de wire e de dados é não-negociável na v1** (REST, CLI, DB). Melhorias internas são livres.
3. **Dívida técnica conhecida resolve-se de raiz**, mas cada correcção que altera comportamento observável é listada em §7 e assinalada no changelog.
4. **Não se porta linha a linha.** Onde a arquitectura Rust é diferente (um handler em vez de quatro proxies, catálogo em memória), porta-se o comportamento, não a estrutura. A web app segue o mesmo princípio: mesmos ecrãs e mesma API, código novo.

## 2. Mapeamento de pacotes

| Go (`github.com/mcpjungle/mcpjungle`) | LOC aprox. | Rust (crate::módulo) | Notas de porte |
|---|---|---|---|
| `main.go` | 20 | `trunks/src/main.rs` | exit 1 em erro; `ErrSilent` → `ExitCode` sem reimprimir |
| `cmd/root.go`, `cmd/*.go` | ~3.5k | `trunks/src/commands/*` | clap derive; ver §4 |
| `cmd/config/` | 100 | `trunks/src/config.rs` | YAML `~/.trunks.conf` + fallback `~/.mcpjungle.conf`; `0600` |
| `cmd/start.go` | 500 | `trunks/src/commands/start.rs` + `trunks-api::serve` | precedência de config (§5 da spec) |
| `client/` | ~1.2k | `trunks-client` | 1:1; sem tipos de DB na API pública |
| `internal/api/server.go`, `server_config.go` | 400 | `trunks-api/src/router.rs` | axum `Router::nest` por grupo de middleware |
| `internal/api/middleware.go` | 200 | `trunks-api/src/middleware/{initialized,user_auth,client_auth,admin,enterprise}.rs` | `ServerConfig` via `watch` channel, não DB por pedido |
| `internal/api/mcp_servers.go`, `mcp_tools.go`, `mcp_prompts.go`, `mcp_resources.go`, `tool_groups.go`, `mcp_clients.go`, `users.go` | ~1.5k | `trunks-api/src/handlers/*.rs` | shapes em `trunks-types` |
| `internal/api/dashboard*.go` | 700 | `trunks-api/src/dashboard/*.rs` | inclui página HTML do callback OAuth e cache de resultados com retenção 10 min |
| `internal/api/errors.go`, `pkg/apierrors` | 100 | `trunks-types::ApiError`, `trunks-core::Error` (snafu, `impl IntoResponse`) | `errors.Is` chain → `enum` com variantes e context selectors; `code` só para OAuth |
| `internal/service/mcp/mcp.go`, `server.go`, `tool.go`, `prompt.go`, `resource.go` | ~2.5k | `trunks-core::registry` | transaccional; cria `CatalogSnapshot` |
| `internal/service/mcp/proxy.go`, `proxy_filter.go` | 500 | `trunks-core::gateway` (`GatewayHandler`) | filtro + autorização dentro do handler |
| `internal/service/mcp/session_manager.go`, `session_result.go` | 400 | `trunks-core::sessions` | lock por servidor; classificação de erro tipada |
| `internal/service/mcp/util.go` | 400 | `trunks-types::names`, `trunks-core::upstream` | `validateServerName`, `splitServerToolName`, `buildResourceURI`, `isLoopbackURL`, `createXxxConn` |
| `internal/service/mcp/upstream_oauth.go` | 700 | `trunks-core::oauth` | sobre `rmcp::transport::auth` ou `oauth2` |
| `internal/service/mcpclient/`, `user/`, `config/` | 400 | `trunks-core::identity`, `trunks-core::server_mode` | |
| `internal/service/toolgroup/` | 600 | `trunks-core::groups` | resolução pura + CRUD; sem callbacks |
| `internal/service/dashboard/` | 400 | `trunks-core::dashboard` | shaping sanitizado |
| `internal/model/*` | 500 | `trunks-db::rows::*` | structs planos + SQL explícito (D11); mesmos nomes de tabela/coluna |
| `internal/db/`, `internal/migrations/` | 150 | `trunks-db::{connect, migration}` | baseline idempotente; ver §5 |
| `internal/configresolver/` | 150 | `trunks/src/envsubst.rs` | só `${VAR}`; recursivo em `serde_json::Value`; var em falta = erro |
| `internal/telemetry/` | 300 | `trunks-core::metrics` | `prometheus-client`; `Noop` quando desligado |
| `internal/dashboardui/` | 20 | `trunks-dashboard` | `rust-embed` |
| `pkg/types/*` | 800 | `trunks-types` | serde com os mesmos `json:"..."` |
| `pkg/logger/` (zap) | 100 | — | **não portar**: não é usado em runtime no original |
| `pkg/util/DiffTools` | 50 | `trunks-core::groups::diff` | |
| `pkg/version/` | 50 | `trunks-types::version` | `NormalizeVersion` prefixa `v` |
| `pkg/testhelpers/`, `internal/e2e/`, `scripts/*.sh` | ~3k | `tests/conformance` | ver §8 |
| `web/dashboard/` | 2.2k TS | `web/app/` | **não copiar** — reescrita em Lit + `line://ui` contra a mesma API `/api/dashboard` (D2) |

## 3. Contrato REST — shapes a preservar

Tipos `serde` em `trunks-types` com os nomes JSON exactos. Lista dos que têm armadilhas:

| Tipo | Campos JSON | Armadilha |
|---|---|---|
| `McpServer` (GET /servers) | `name, transport, enabled, description, url?, command?, args?, env?, session_mode` | |
| `RegisterServerInput` | `name, transport, description, url, bearer_token, headers{}, command, args[], env{}, session_mode, oauth_redirect_uri, oauth_client_id, oauth_client_secret, oauth_scopes[]` | `GET /server_configs` devolve isto **com segredos** (admin). |
| `RegisterServerResult` | `server?` **ou** `authorization_required?{session_id, authorization_url, expires_at}` | 201 vs 202 |
| `Tool` (GET /tools, /tool) | `ID, CreatedAt, UpdatedAt, DeletedAt, name, enabled, description, input_schema, annotations` | O original vaza `gorm.Model` com **PascalCase**. Manter: `#[serde(rename="ID")]` etc. `name` é canónico. |
| `Prompt`, `Resource` | idem + `arguments` / `uri, mime_type, annotations, meta` | `original_uri` e `server_id` **não** aparecem |
| `ToolInvokeResult` | `_meta?, isError?, content[], structuredContent?` | camelCase aqui |
| `PromptResult` | `description, messages[{role, content}], meta?` | |
| `ResourceReadResult` | `contents[]` | URIs reescritas para `mcpj://` |
| `EnableDisableServerResult` | `name, tools_affected[], prompts_affected[]` | resources afectados **não** são devolvidos |
| `McpClient` (POST/GET /clients) | `ID…, name, description, access_token, allow_list[]` | POST devolve o token; GET também (paridade — plaintext) |
| `CreateOrUpdateUserResponse` | `username, role, access_token` | `User` (GET) é só `username, role` |
| `CreateToolGroupResponse` | `streamable_http_endpoint, sse_endpoint, sse_message_endpoint` | **[D4]** manter os campos SSE com valor vazio ou omitir? Recomendação: omitir (`skip_serializing_if`) — o CLI antigo imprime strings vazias sem falhar. |
| `GetToolGroupResponse` | ToolGroup flattened + endpoints | |
| `UpdateToolGroupResponse` | `name, old, new` | |
| `APIErrorResponse` | `error, code?` | |
| Dashboard `*Response` | ver `pkg/types/dashboard.go` | `config_summary` só `header_keys`, `env_keys`, `command` truncado |

Códigos: 200/201/202/204 por rota conforme spec §5; `DELETE /tool-groups/{n}` → 204 mesmo se não existir; `DELETE /clients/{n}` idem; `DELETE /servers/{n}` → 404 se não existir.

Query params: `?force=` parseado como bool estrito (`strconv.ParseBool` → `"true"|"1"|"t"|"T"|"TRUE"|"True"`…; inválido → 400). `?entity=` aceita nome de servidor ou nome canónico.

## 4. Contrato CLI — inventário

Preservar nomes, flags, defaults, exit codes e as strings de saída que o `scripts/test-mcpjungle.sh` faz `grep`. Tabela resumida (detalhe no original `cmd/*.go`):

| Comando | Flags | Endpoint | Saída-chave |
|---|---|---|---|
| `start` | `--port`, `--host`, `--sqlite-db-path`, `--enterprise`, `--prod` | — | banner + `HTTP server listening on` |
| `register` | `--name`, `--url`, `--description`, `--bearer-token`, `--force`, `-c/--conf` | `POST /servers[?force]`, OAuth loopback | `Server <n> registered successfully!`; lista tools/prompts/resources; `This server does not provide any tools, prompts or resources.` |
| `deregister <n>` | — | `DELETE /servers/{n}` | |
| `list tools\|prompts\|resources\|servers\|mcp-clients\|users\|groups` | `--server`, `--group` (tools) | vários | `N. name  [ENABLED\|DISABLED]` |
| `usage <tool>` | — | `GET /tool` | schema legível; `(required\|optional)` |
| `invoke <tool>` | `--input` (default `{}`), `--group` | `POST /tools/invoke` | `Response from tool:` / `The tool returned an error:`; ficheiros `image_<ts>.<ext>` etc. |
| `get group\|prompt\|resource` | `--arg k=v`, `--read` | | |
| `enable\|disable tool\|prompt\|server <n>` (+ legado `enable <n>`) | — | `POST /tools/enable?entity=` … | |
| `create mcp-client [n]` | `--allow`, `--description`, `--access-token`, `-c` | `POST /clients` | `Access token: …`; warning com `*` |
| `create user [n]` | `--access-token`, `-c` | `POST /users` | `mcpjungle login <token>` → **`trunks login <token>`** |
| `create group -c` | `-c` obrigatório | `POST /tool-groups` | endpoints |
| `delete mcp-client\|user\|group <n>` | — | | |
| `update group -c`, `update mcp-client <n> --access-token`, `update user <n> --access-token` | | `PUT …` | diff `added/removed` ou `No changes detected` |
| `init-server` | — | `POST /init {"mode":"production"}` | guarda token |
| `login <token>` | — | `GET /users/whoami` | `You are now logged in as <u>` |
| `export` | `-d/--dir` (default `.mcpjungle`) | `GET /tool-groups`, `GET /server_configs` | `groups/<n>.json`, `servers/<n>.json`; dir tem de estar vazio |
| `version` | — | `GET /metadata` (5 s) | `CLI Version:`, `Server Version:` |
| `--registry` (global), `--version/-v` | | | |

Ficheiros JSON de configuração (`${VAR}` expandido no CLI): servidor (`RegisterServerInput`), grupo, mcp-client (`name, description, allowed_servers[], access_token, access_token_ref{env,file}`), user (`name, access_token, access_token_ref`). Nota: no ficheiro de user o campo é `name`, não `username`.

## 5. Base de dados — compatibilidade e migrações

### 5.1 Estado de partida
O original corre `AutoMigrate` do GORM em cada arranque; não há tabela de versões. Um Trunks que arranca sobre uma DB existente encontra as 10 tabelas com o schema de §4.2 da spec, criadas por GORM, com estas particularidades:

- `id` é `bigserial`/`INTEGER PRIMARY KEY AUTOINCREMENT`; timestamps `timestamptz`/`datetime`; `deleted_at` nullable com índice `idx_<tabela>_deleted_at`.
- JSON: Postgres `jsonb`; SQLite `JSON`/`text` (GORM com glebarez mapeia `jsonb` → `JSON` que no SQLite é afinidade NUMERIC/TEXT).
- Índices únicos com nomes GORM (`idx_mcp_servers_name`, `uni_users_username`, …). Não dependemos dos nomes.
- Booleans no SQLite são `numeric` 0/1.

### 5.2 Estratégia
Migrações versionadas (ferramenta conforme D11: `sqlx::migrate!`, `refinery`, ou runner próprio) com tabela de versões `_trunks_migrations`:

- **`m20261101_000001_baseline`**: para cada tabela, `CREATE TABLE IF NOT EXISTS` com o schema GORM exacto; para cada índice, `CREATE [UNIQUE] INDEX IF NOT EXISTS`. Numa DB vazia cria tudo; numa DB MCPJungle não faz nada. Testada contra dumps reais (SQLite e Postgres) gerados pelo binário Go — ficheiros `tests/fixtures/mcpjungle-0.4.x.{sqlite,sql}`.
- **`m20261101_000002_tool_uniqueness`**: `CREATE UNIQUE INDEX IF NOT EXISTS ux_tools_server_name ON tools(server_id, name) WHERE deleted_at IS NULL` (idem prompts; resources por `uri`). Antes de criar, dedup defensivo: manter o `id` mais alto por chave. **Aditiva e reversível.**
- Migrações futuras (D8: `access_token_hash`) são `ALTER TABLE ADD COLUMN` + backfill, nunca destrutivas na v1.x.

Regras: Trunks **nunca** faz `DROP`/`ALTER … TYPE` numa coluna existente na série 1.x; a DB continua a poder ser aberta pelo binário Go (rollback operacional possível enquanto o Go `AutoMigrate` ignorar colunas extra — ele ignora).

### 5.3 Leitura de dados existentes — casos a testar
- `mcp_servers.config` com `headers` contendo `Authorization` explícito.
- `session_mode` NULL/vazio em linhas antigas → tratar como `stateless`.
- `tool_groups.*` JSON `null` vs `[]`.
- `mcp_clients.allow_list` malformado → sem acesso (não erro).
- `server_configs.mode = 'production'` em DBs antigas (o hook `BeforeSave` só normaliza em escrita) → ler como enterprise.
- Linhas com `deleted_at` não nulo (soft-deleted por versões antigas) → ignorar.
- `transport = 'sse'` → D4.

## 6. Guia operacional: trocar MCPJungle por Trunks

### 6.1 Pré-requisitos
- MCPJungle ≥ 0.4.x (schema de §5.1). Versões anteriores: actualizar primeiro o Go para 0.4.x (que migra) e só depois trocar.
- Backup da DB (`pg_dump` ou cópia do `mcpjungle.db`).
- `mcpjungle list servers` para verificar se há `Transport: sse`. Se houver, re-registar como `streamable_http` quando o upstream o suportar, ou aceitar `TRUNKS_IGNORE_SSE_SERVERS=1` (ficam desactivados).

### 6.2 Passos
1. Parar o `mcpjungle start`.
2. Trocar binário/imagem: `ghcr.io/<org>/trunks:latest[-stdio]`. Mesmas env vars (`DATABASE_URL`, `SERVER_MODE`, `OTEL_ENABLED`, `PORT`, …).
3. `trunks start`. No arranque: aplica a baseline (no-op), valida o modo (tem de coincidir com o inicializado), reconstrói o catálogo e imprime um resumo `N servers, N tools, N prompts, N resources; N groups`.
4. Clientes (Cursor, `mcp-remote`, Copilot): **sem alterações** — mesmos URLs e tokens.
5. CLI: `brew install …/trunks`; copiar `~/.mcpjungle.conf` → `~/.trunks.conf` (ou deixar o fallback funcionar).
6. Dashboards Prometheus: ligar `TRUNKS_METRICS_LEGACY_NAMES=1` até renomear queries.
7. Smoke test: `trunks list servers`, `trunks invoke <server>__<tool> --input '{}'`, pedido `tools/list` ao `/mcp` com um cliente real.

### 6.3 Rollback
Parar Trunks, repor o binário Go, arrancar. A DB é compatível (só índices aditivos e a tabela de versões de migrações, que o GORM ignora).

### 6.4 Via `export`/`import` (alternativa sem DB partilhada)
`mcpjungle export -d ./snapshot` gera `servers/*.json` e `groups/*.json`; `trunks register -c servers/<n>.json` e `trunks create group -c groups/<n>.json` recriam. Users e MCP clients **não** são exportados pelo original — têm de ser recriados (novos tokens). Útil para mudar de SQLite para Postgres ao mesmo tempo. D8 propõe `trunks apply -d ./snapshot` para automatizar.

## 7. Diferenças de comportamento (changelog de migração)

### 7.1 Breaking (requerem acção)
| # | Diferença | Acção |
|---|---|---|
| B1 | Endpoints `/sse`, `/message`, `/v0/groups/{g}/sse|message` **removidos** (D4, fechada). | Clientes SSE → Streamable HTTP (todos os clientes actuais suportam). |
| B2 | Upstreams `transport: sse` não são ligados; arranque falha a menos que `TRUNKS_IGNORE_SSE_SERVERS=1`. | Re-registar como `streamable_http`. |
| B3 | Nomes de métricas `trunks_*` (legacy opcional via flag). | Flag ou actualizar queries. |
| B4 | `Host` header validado por defeito (loopback) no `/mcp` — igual ao mcp-go **mas** agora configurável. Deployments atrás de reverse proxy que hoje funcionam por o mcp-go aceitar `Host` arbitrário? Verificar: mcp-go 0.58 **também** valida loopback por defeito; comportamento igual. | `MCP_ALLOWED_HOSTS` se necessário. |

### 7.2 Correcções visíveis (melhoram, não quebram)
| # | Original | Trunks |
|---|---|---|
| C1 | Tools de upstream `sse` só visíveis em `/sse`; stdio/http só em `/mcp`. | Tudo em `/mcp` (irrelevante com B1/B2). |
| C2 | `PUT /tool-groups/{n}` com lista vazia ou `description: ""` não persistia (GORM `Updates` ignora zero-values). | Persiste exactamente o enviado. |
| C3 | Registo/deregisto não transaccional — falha a meio deixava órfãos. | Transaccional. |
| C4 | Connect de sessão stateful segurava lock global. | Lock por servidor. |
| C5 | Erros com substring `"timeout"`/`"transport"` invalidavam a sessão stateful (incl. erros de negócio do upstream). | Só erros de transporte tipados. |
| C6 | Panic se `"mode"` não estiver no contexto. | Impossível por tipo. |
| C7 | `update user` sem argumento fazia panic. | Erro de uso. |
| C8 | `~/.mcpjungle.conf` criado com permissões default. | `0600`. |
| C9 | Tools sem UNIQUE(server_id,name). | Índice único. |
| C10 | SSE upstream `initialize` sem timeout e protocolo fixo `2024-11-05`. | N/A (B2). |

### 7.3 Preservado deliberadamente (embora discutível)
- Tokens em plaintext na DB e devolvidos em `GET /clients` (D8 muda).
- Prompts/resources não filtrados na listagem em enterprise (D8 muda).
- Grupos só com tools.
- `DELETE /tool-groups/{n}` e `DELETE /clients/{n}` devolvem 204 para inexistentes.
- `GET /tools` devolve campos `ID/CreatedAt/UpdatedAt/DeletedAt` em PascalCase.
- `RecordPromptCall` reutiliza as métricas de tools.
- Stateless stdio = um processo por chamada.
- Dashboard e `/api/dashboard` sem autenticação (dev-only).

## 8. Estratégia de testes de paridade

### 8.1 Porte dos testes Go
| Origem | Destino | Método |
|---|---|---|
| `internal/service/mcp/*_test.go` (tabelas de nomes, URIs, loopback, filtro, headers, sessões, OAuth) | unit tests em `trunks-types`/`trunks-core` | transcrever as **tabelas de casos** (inputs/expected), não a estrutura |
| `internal/api/*_test.go` | `trunks-api/tests` | `axum::Router` + `tower::ServiceExt::oneshot`; SQLite `:memory:` |
| `internal/e2e/*_test.go` | `tests/conformance/*.rs` | black-box contra binário; `server-everything` via `npx` como upstream; skip se `npx` ausente |
| `scripts/test-mcpjungle.sh`, `scripts/test-api-error-responses.sh` | `tests/conformance/cli.rs` | `assert_cmd`; **ou** correr os scripts originais com `MCPJUNGLE_BIN=./target/release/trunks` — zero porte, máxima fidelidade. Fazer ambos: scripts como gate, Rust como regressão. |

### 8.2 Oráculo diferencial
Um harness `tests/differential/` arranca **os dois binários** (Go e Rust) sobre cópias da mesma DB fixture, dispara a mesma sequência de pedidos REST e MCP, e compara respostas JSON (ignorando `CreatedAt/UpdatedAt`, latências). Cobre os shapes que a documentação não descreve e que os testes Go não assertam. Corre em CI enquanto o Go for a referência; retira-se quando Trunks divergir intencionalmente.

### 8.3 Matriz
{dev, enterprise} × {sqlite, postgres} × {upstream stdio, upstream streamable_http, upstream com OAuth (wiremock)}. Clientes reais em smoke manual: Claude Code, Cursor, `mcp-remote`, Copilot.

## 9. Dependências — tabela de equivalência

| Go | Rust | Comentário |
|---|---|---|
| `mark3labs/mcp-go` 0.58 | `rmcp` 3.8 | Diferença de modelo: registry de handlers vs `ServerHandler` trait. Sem SSE. OAuth client incluído (`auth`). |
| `gin-gonic/gin` | `axum` 0.8 + `tower-http` | binding/validation → `serde` + validação manual nos handlers |
| `gorm.io/gorm` + `datatypes` + drivers | **D11 em análise** — sem ORM (`sqlx` / `sqlx`+`sea-query` / drivers nativos) | SQL explícito; filtrar `deleted_at IS NULL` nos repositórios |
| `glebarez/sqlite` (pure Go) | SQLite `bundled` (libsqlite3 compilado no crate) | sem dependência do sistema; build cross simples |
| `spf13/cobra` | `clap` 4 | help groups via `display_order`/`next_help_heading` |
| `joho/godotenv` | `dotenvy` | |
| `gopkg.in/yaml.v3` | `serde_yaml` (ou `serde_yml`) | só para o ficheiro do CLI |
| `prometheus/client_golang` + OTel SDK + exporter | `prometheus-client` | OTel não é necessário; sem OTLP no original |
| `go.uber.org/zap` | `tracing` | zap não era usado |
| `spf13/afero` | — | só para testes; usar `tempfile` |
| `stretchr/testify` | `assert`/`pretty_assertions`, `insta` | |
| `crypto/rand`, `encoding/base64`, `encoding/hex` | `rand`, `base64`, `hex`, `sha2` | |
| `net/http` client | `reqwest` (rustls) | |
| `//go:embed` | `rust-embed` | |
| goreleaser | `cargo-dist` (D9) + GH Actions para Docker | Homebrew tap, instaladores e checksums geridos pelo `dist`; imagens Docker multi-arch num workflow à parte |
| golangci-lint | `clippy` + `rustfmt` + `cargo-deny` | |

## 10. Riscos de migração e mitigações

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| Diferença subtil de shape JSON não coberta por testes Go | Alta | Médio (CLI/dashboard antigos quebram) | Oráculo diferencial (§8.2) |
| Clientes MCP antigos vs protocolo 2026-07-28 default do `rmcp` | Média | Alto | `legacy_session_mode=true`; testar com os 4 clientes reais antes da v1 |
| DB Postgres com tipos ligeiramente diferentes do esperado (ex. `jsonb` vs `json`, `timestamp` vs `timestamptz`) | Média | Alto | Fixtures reais geradas pelo binário Go em CI; baseline só `IF NOT EXISTS` |
| Utilizadores com upstreams SSE | Baixa–média | Médio | B1/B2 + mensagem de erro accionável + docs |
| `rmcp` evolui rápido (3.x em 2026 teve várias minor) | Alta | Baixo–médio | pin exacto no `Cargo.lock`; `cargo deny` para advisories; abstrair `UpstreamConn` para isolar a API do SDK |
| Sub-estimar o OAuth (DCR, refresh, estado pendente) | Média | Médio | Spike S3 cedo; `wiremock` como AS |
