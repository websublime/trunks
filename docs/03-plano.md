# Trunks — Plano de Execução

> Versão 2.0 (2026-11-04). Depende de `01-produto.md` (decisões) e `02-especificacao.md` (arquitectura). A estratégia de compatibilidade de dados (D3) vive num doc interno de migração.

---

## 1. Pressupostos

- Um engenheiro (Miguel) a tempo parcial, com apoio de agentes de código. Estimativas em **semanas-pessoa de trabalho efectivo**, não em calendário; multiplicar pelo factor de disponibilidade real.
- Rust stable, `rmcp` 3.8 pinado. Se um spike falhar, o plano B correspondente da spec §14 substitui o item sem alterar as fases.
- Decisões D1 (nome: Trunks), D4 (sem SSE), D9 (`cargo-dist`), D10 (`snafu`) e D11 (`sqlx` sem ORM, `06-analise-db.md`) estão fechadas.
- A web app é construída com `line://ui` (D2). Roadmap pós-v1.0 em `05-roadmap.md`.

## 2. Princípios de sequenciação

1. **Desriscar primeiro**: os três spikes atacam as únicas incógnitas técnicas reais (`rmcp` server com principal por pedido, lib de DB escolhida em D11 sobre fixtures legacy, OAuth no `rmcp`).
2. **Verticais finas, não camadas**: cada fase entrega um fluxo end-to-end utilizável (`start` → `register` → `/mcp` funciona) em vez de "toda a DB" depois "toda a API".
3. **Teste de contrato antes do código**: no início de cada fase escrevem-se os testes de conformidade dessa área; a fase fecha quando passam.
4. **Oráculo diferencial** contra a implementação legacy (doc interno) corre em CI desde a Fase 2 até v1.0, para garantir D3.

## 3. Fases

### Fase 0 — Spikes e fundações (1,5 sem.)

Objectivo: provar as três hipóteses técnicas e deixar o workspace a compilar com CI verde.

| Item | Entregável | Critério de saída |
|---|---|---|
| S1 `rmcp` server | **Feito** — `docs/spikes/S1-rmcp-principal.md`, código em `spikes/s1-rmcp-principal/`. 21 checks verdes: principal por pedido em ambos os modos de sessão, cliente `rmcp` e `mcp-remote`. Falta só smoke com Claude Code/Cursor reais (Fase 7). | Sem plano B necessário. |
| D11 análise DB | **Feito** — `06-analise-db.md`: 4 protótipos contra as fixtures SQLite e Postgres; decisão `sqlx` nativo com `$N`. | Decisão registada em `01-produto.md`. |
| S2 DB | **Feito** — `trunks-db` real: `Db` enum, `connect`, migrações `0001_baseline` (IF NOT EXISTS) / `0002_normalize_legacy` / `0003_entity_uniqueness` por dialecto, structs das 10 tabelas, repos de leitura + `ConfigStore`. Testes contra as fixtures SQLite e Postgres (CI com serviço `postgres:16`). `trunks start` já liga, migra e valida o modo. | Zero erros de mapeamento; segunda migração é no-op; mismatch de modo recusado. |
| S3 OAuth | **Feito** — `docs/spikes/S3-upstream-oauth.md`, código em `spikes/s3-upstream-oauth/`. 22 checks com `wiremock`: 401→discovery→DCR→URL (PKCE/state/resource)→restart→troca→uso via `AuthClient`→refresh→refresh rejeitado; cliente pré-registado; AS sem DCR. `rmcp::transport::auth` chega; sem `oauth2` directo. | Decisão: `rmcp auth`. |
| Workspace | Cargo workspace com os 7 crates vazios, lints (`clippy -D warnings`, `rustfmt`, `cargo-deny`), `snafu` como base de erros em cada crate, CI GitHub Actions (lint, test, build linux/mac/win), `cargo dist init` com `dist-workspace.toml` (targets, instaladores), `xtask build-dashboard`, `web/dashboard` copiado e a buildar. | `cargo test` e CI verdes; `trunks --version` imprime versão; `dist plan` lista os artefactos. |

### Fase 1 — Núcleo: arranque, DB, modos, registry read-only (2 sem.) — **Feita**

Objectivo: `trunks start` arranca sobre uma DB legacy existente (normalizada por migração) e serve a parte de leitura da REST API.

- `trunks-types`: enums, newtypes (`ServerName`, `CanonicalName`, `ResourceUri`, `GroupName`) com as tabelas de casos de validação; tipos REST.
- `trunks-db`: structs de linha, repositórios com SQL explícito (lib de D11), `connect()` (`TRUNKS_DATABASE_URL`), migrações baseline + `tool_uniqueness` + `normalize_legacy` (URIs `trunks://`, modo `enterprise`).
- `trunks-core`: `ServerMode` service (`init`, verificação de modo no arranque), `Catalog` construído da DB, `metrics` (noop + prometheus).
- `trunks-api`: router, middleware `require_initialized`/`user_auth`/`require_admin`/`require_enterprise`, `/health`, `/metadata`, `/init`, `/metrics`, rotas GET de `/api/v0/{servers,tools,tool,prompts,prompt,resources}`, erros.
- `trunks`: `start` (config completa, banner, shutdown gracioso), `version`, `list servers|tools|prompts|resources`, `usage`, `--registry`, ficheiro de config.
- Testes: unit das tabelas de nomes; API tests das rotas GET; conformidade: arrancar sobre as fixtures e comparar `list *` com a implementação legacy (harness diferencial v0).

**Saída:** leitura completa. Um operador aponta Trunks a uma DB existente e vê todos os servidores, tools, prompts e resources.

Entregue: `trunks-types::api` (D13), `trunks-core::{domain,catalog,identity,mode,metrics}` com ports `async_trait`, `trunks-db::Store` (ports sobre `Db`), `trunks-api` (middleware `require_initialized`/`user_auth`/`require_admin`/`require_enterprise`, `/health` `/metadata` `/init` `/metrics`, `GET /api/v0/{servers,tools,tool,prompts,prompt,resources,users/whoami}`), CLI (`start` com catálogo e validação SSE, `list`, `usage`, `init-server`, `login`, config `~/.config/trunks/config.toml` 0600). Testes: domínio (catálogo/grupos), `trunks-db` nas duas DBs, API em dev e enterprise sobre as fixtures.

### Fase 2 — Gateway MCP e registry read-write (3 sem.) — **Feita** (2a + 2b)

Objectivo: fluxo principal completo em modo development. Dividida em dois PRs: **2a** upstream + sessões + `/mcp` + invocação REST sobre o catálogo existente; **2b** registry read-write + CLI.

- `trunks-core::upstream`: `UpstreamConn::connect` para `stdio` (TokioChildProcess, env, stderr→tracing, process group) e `streamable_http` (headers, bearer; sem OAuth ainda); timeout de `initialize` e mensagens de erro accionáveis.
- `trunks-core::sessions`: `SessionManager` stateless/stateful, sweeper de idle, invalidação tipada, shutdown.
- `trunks-core::registry`: `register` (validar → connect → descobrir tools/prompts/resources → transacção), `deregister`, `set_server_enabled`, `set_{tools,prompts,resources}_enabled`; rebuild do catálogo.
- `trunks-core::gateway`: `GatewayHandler{Scope::Global}` com `list_*`/`call_tool`/`get_prompt`/`read_resource`, reescrita de nomes/URIs, strip de headers, métricas.
- `trunks-api`: `ANY /mcp`; `POST /servers` (sem OAuth: devolve erro accionável se o upstream pedir auth), `DELETE /servers/{n}`, `enable|disable` de servidor/tools/prompts, `POST /tools/invoke`, `/prompts/render`, `/resources/{get,read}`, `GET /server_configs`.
- `trunks`: `register` (flags e `-c` com `${VAR}`; sem OAuth), `deregister`, `enable|disable`, `invoke` (incl. gravação de ficheiros), `get prompt|resource`, `export`.
- Testes: strip de headers, filtro em modo dev, session manager (stateless/stateful/idle/invalidação), registo/deregisto/enable, handlers de tool/prompt/resource; e2e modo dev; harness diferencial; smoke manual com Claude Code e Cursor.

**Saída:** Trunks utilizável pelo developer individual (modo dev, sem OAuth, sem grupos).

Entregue em 2a: `trunks-core::upstream` (`UpstreamConn` stdio/streamable_http construído só da configuração — nenhum header downstream chega ao upstream; timeout de `initialize` com mensagem accionável; `sse` recusado), `trunks-core::sessions` (`SessionManager` stateless/stateful com lock por servidor, sweeper de idle, invalidação em erro de transporte, shutdown), `trunks-core::gateway` (`GatewayHandler` global e por grupo, principal fail-closed a partir de `Parts`, reescrita de nomes/URIs, `Invoker` partilhado com a REST, métricas), `trunks-api` (`ANY /mcp`, `ANY /v0/groups/{g}/mcp` com cache de handlers, `POST /api/v0/tools/invoke`, `/prompts/render`, `/resources/{get,read}`), CLI `invoke` (texto/JSON; gravação de ficheiros binários fica para 2b), crate `trunks-testing` (upstream falso stdio/HTTP). Testes: 6 de gateway no core, 2 e2e (dev + enterprise com allow-lists e grupos).

Entregue em 2b: `trunks-core::registry` (`RegistryService`: validar → connect → descobrir → transacção; `deregister`; flags de servidor e de entidades com a semântica do §4.4; invalidação de sessões; rebuild do catálogo), port `RegistryStore` de escrita e `trunks-db::repos::registry` (transacções, `RETURNING`, testado nas duas DBs), `trunks-api::routes::admin` (`POST/DELETE /servers`, `enable|disable`, `GET /server_configs`) atrás de `require_admin`, SDK (`register_server`, `deregister_server`, `set_*_enabled`, `server_configs`, `render_prompt`, `read_resource`), CLI `register` (flags e `-c` com `${VAR}`), `deregister`, `enable|disable`, `get prompt|resource`, `export`, e gravação de `image_*/audio_*/resource_*` no `invoke`. Testes: 3 e2e de registry (dev + gate admin em enterprise), escrita na DB, `envsubst`. Smoke manual do CLI contra o servidor. Em falta nesta fase: harness diferencial e smoke com Claude Code/Cursor (fica para a validação de M2).

### Fase 3 — Enterprise: identidades e controlo de acesso (1,5 sem.) — **Feita**

- `trunks-core::identity`: users (admin/user), mcp clients, geração/validação de tokens, allow-lists.
- Middleware `mcp_client_auth`; `Principal::McpClient`; filtragem em `list_tools` e autorização em `call_*` (fail-closed); rejeição de tokens de user no `/mcp`.
- `/init` em enterprise (cria admin, devolve token uma vez); `/api/v0/{clients,users}` CRUD; `/users/whoami`.
- CLI: `init-server`, `login`, `create|delete|update mcp-client|user`, `list mcp-clients|users`.
- Testes: filtro em enterprise, clients, users, middleware; e2e modo enterprise.

Entregue: port `IdentityStore` completo e `trunks-db::repos::identity` (CRUD; violação de unicidade mapeada para 400), `IdentityService` com users/clients (validação de nomes, allow-lists e tokens; rotação; admin indelével), `trunks-api::routes::identities` (`/users*`, `/clients*` atrás de `require_enterprise` + `require_admin`), SDK, CLI `create|update|delete user|mcp-client` (flags ou `-c` com `${VAR}`) e `list users|mcp-clients`. Auth, principals, allow-lists no gateway, `/init`, `whoami`, `init-server` e `login` já existiam das fases 1–2. Testes: 3 e2e (enterprise-only, users + semântica de tokens, clients + allow-lists a aplicar em `/mcp` sem restart, rejeição de tokens de user), unit de validação. Smoke manual do CLI em enterprise.

### Fase 4 — Tool groups (1 sem.) — **Feita**

- `trunks-core::groups`: resolução, validação, CRUD transaccional, diff, `GroupView` no catálogo, startup degradado.
- `GatewayHandler{Scope::Group}`; rota `/v0/groups/{name}/mcp` com cache de services; endpoints na resposta (sem campos SSE).
- `/api/v0/tool-groups*`; CLI `create|get|list|update|delete group`, `list tools --group`, `invoke --group`.
- Testes: resolução e CRUD de grupos, API, e2e de grupos; verificar que grupo não alarga allow-list.

Entregue: `trunks-core::groups` (`validate` contra o catálogo, `diff`, `GroupService` CRUD com update de substituição total e delete idempotente), port `RegistryStore::{create,update,delete}_group` + `trunks-db::repos::groups`, `trunks-api::routes::groups` (`/tool-groups*` atrás de `require_admin`; delete larga o service cacheado), SDK, CLI (`create|get|update|delete group`, `list groups`, `list tools --group`, `invoke --group`, `export` com `groups/`). O scope de grupo no gateway e a rota `/v0/groups/{g}/mcp` já existiam da Fase 2a. Testes: 2 e2e (CRUD + validação + degradação/recuperação + drop do cache; grupo nunca alarga a allow-list de um client e exige token de client), unit de `diff`. Smoke manual do CLI.

### Fase 5 — OAuth upstream (1,5 sem.) — **Feita**

- `trunks-core::oauth`: bootstrap (discovery, DCR condicional, PKCE, `state`, sessão pendente com TTL, transacção), `complete`, `CredentialStore` em DB com refresh; integração em `UpstreamConn::connect` (usa tokens guardados quando existem); limpeza no `deregister`.
- API: `202 authorization_required`, `POST /upstream_oauth/sessions/{id}/complete`, `code: upstream_oauth_required`.
- CLI `register`: listener loopback, abrir browser, esperar callback, `complete`; erro claro quando o redirect é fornecido pelo utilizador.
- Testes: OAuth com `wiremock`, e2e de registo OAuth, `force=true` com OAuth.

Entregue: `trunks-core::oauth` (`DbCredentialStore`/`CapturedStateStore` sobre `rmcp::transport::auth`, `bootstrap`, `exchange`, `auth_client`), port `OAuthStore` + `trunks-db::repos::oauth` (escritas transaccionais) + migração `0004_oauth_issuer`, erro tipado `UpstreamAuthRequired` detectado na cadeia de erros do `initialize`, `RegisterOutcome` e `complete_oauth` no `RegistryService`, limpeza no `deregister`, purga no arranque, rota `…/complete`, `202` no `POST /servers`, SDK `complete_oauth`, CLI (`register` com listener loopback e browser, `oauth-complete`, flags `--oauth-*`), fakes de AS/upstream em `trunks-testing::oauth`. Testes: 2 e2e na API (DCR + restart-safe via DB, erros de sessão/state/code, bearer nas chamadas do gateway, export sem tokens, deregister limpa; cliente pré-registado sem DCR, refresh transparente persistido) e 1 e2e do binário (`register` → callback loopback → `invoke`). `force=true` com OAuth passa pelo `register` normal (coberto pelos testes de registry).

### Fase 6 — Dashboard, modelo de acesso e web app em `line://ui` (5–6 sem.)

Decisões D14–D17 (`01-produto.md`). Dividida em quatro PRs:

- **6a — Dashboard API lean + gating.** `trunks-core::dashboard` (shaping sanitizado, puro), `/api/dashboard/{overview,servers,servers/{n},tools,prompts,resources,tool-groups,tool-groups/{n},diagnostics,oauth/session/{id}}` + `GET /api/dashboard/oauth/callback` (HTML; o backend completa a troca). Em enterprise todas as rotas de dashboard e estáticos passam por `user_auth` (bearer); em development ficam abertas. Mutações reutilizam `/api/v0`. Testes: nunca vaza segredos; 401 em enterprise sem token; callback completa o OAuth.
  Entregue: `trunks-core::dashboard` (tipos + shaping puro, unit test de sanitização), `RuntimeInfo` no `AppState`, notas de saúde por upstream no `SessionManager`, `RegistryService::{complete_oauth_by_state, dashboard_extras, oauth_session_status}`, rotas `trunks-api::routes::dashboard` (reads atrás de `user_auth`, callback público), SDK `Client::token`. Testes: reads sanitizados + 401 em enterprise + degradação de grupo + diagnósticos; callback do browser completa a troca e o polling vê `completed`; saúde do `SessionManager`.
- **6b — Teams, grants e clients com dono.** Migração `0005_access` (`teams`, `team_members`, `access_grants`, `mcp_clients.owner_kind/owner_id`), `trunks-core::access` (resolução pura do conjunto efectivo; tabela de casos), `Principal`/`UserPrincipal` com conjunto efectivo, filtragem em todas as listagens e invocações, `/api/v0/teams*`, `/api/v0/{servers,tool-groups}/{n}/access`, clients com `owner`, CLI (`create|delete team`, `add|remove team-member`, `grant|revoke`, `list teams|grants`). Testes: tabela de resolução, e2e de visibilidade e de self-service, grupo nunca alarga acesso.
  Entregue: migração `0005_access`, `trunks-core::access` (`AccessSet`, resolução e `narrowed_by` com tabela de casos), port `AccessStore` + `trunks-db::repos::access`, `IdentityService` (teams, membros, grants, dono de clients, `access_of_user/client`), `Principal`/`UserPrincipal` com `AccessSet` e `may_use_tool`, gateway a usar acesso por tool (grupos concedidos), filtragem em todas as leituras/invocações de `/api/v0`, rotas `teams*`/`grants`/`{servers,tool-groups}/{n}/access`, clients com dono e self-service, `whoami` estendido, SDK e CLI. Testes: 2 e2e novos (grants → visibilidade/invocação incl. grupo e wildcard; teams/membros/papéis/clients com dono/narrowing/revogação viva/delete de team) + unit de `AccessSet`; fixture legacy: user sem grants vê nada.
- **6c — Web app** (adiada: espera por uma base maior de componentes da `line://ui`; a Fase 7 avança entretanto). Lit + `line://ui` + Vite/TS em `web/app`; login por token em enterprise; ecrãs de 5.5; `trunks-dashboard` (rust-embed) + `cargo xtask build-dashboard`; smoke com Playwright contra o binário.
- **6d — OIDC para humanos.** `GET /auth/login|callback|logout`, validação do ID token (JWKS, `iss`, `aud`, `nonce`), tabela `sessions` + cookie, JIT provisioning, mapa claims → role/teams (D17), botão "Sign in with SSO" na web app. Testes com IdP falso em wiremock.
  Entregue: migração `0006_oidc` (`users.oidc_issuer/oidc_subject`, `team_members.source`, `sessions`, `oidc_login_states`), port `SessionStore` + `trunks-db::repos::sessions`, `trunks-core::oidc` (discovery + JWKS em cache com refresh por rotação, PKCE, troca do code, verificação do ID token com `jsonwebtoken`, grupos do token ou do userinfo) e `trunks-core::login` (`LoginService`: sessões, JIT por `(issuer, sub)`, sync de role/teams, guardas do último admin), rotas `/auth/{config,login,callback,token,logout}`, `user_auth` com bearer **ou** cookie + verificação de `Origin`, flags/env `TRUNKS_PUBLIC_URL`, `TRUNKS_SESSION_TTL_HOURS`, `TRUNKS_OIDC_*`, `trunks_testing::oidc::FakeIdp`. `delete_user` passa a apagar em cascata e só protege o último admin. Testes: 5 e2e (round-trip + cookie, sync de role/teams nos dois sentidos e via userinfo, rotação de chaves, tokens/estados inválidos, CSRF e login por token) + unit (PKCE, grupos, `return_to`, prefixos). O botão "Sign in with SSO" fica para a 6c (a web app lê `GET /auth/config`).

### Fase 7 — Conformidade final, hardening e release v1.0 (2 sem.)

Dividida em PRs: **7a** hardening → **7b** conformidade → **7c** performance → **7d** release → **7e** documentação. Dependem da 6c: smoke Playwright, a parte "web app" da conformidade e a tag v1.0.

- Harness diferencial completo em CI; suite de conformidade a 100 %; matriz {dev, enterprise} × {sqlite, postgres}.
- Smoke com os 4 clientes reais (Claude Code, Claude Desktop via `mcp-remote`, Cursor, Copilot) em ambos os modos.
- Hardening: `allowed_hosts`, body limits, timeouts por pedido, fuzz de `CanonicalName::parse`/`ResourceUri::parse` (`cargo-fuzz`, 1 h cada), `cargo deny`, revisão de logs por fuga de segredos.
  Entregue (7a): limite de 4 MiB em REST e MCP (413), `TRUNKS_REQUEST_TIMEOUT_SEC` (prazo REST → 504; limite por pedido a upstreams → `UpstreamTimeout`, com reabertura da sessão stateful), access log `INFO` sem query string e com o tipo de principal, invariantes dos parsers partilhadas entre um teste com seed (CI normal) e os targets `cargo-fuzz` (`fuzz/` + workflow `fuzz`), teste de higiene de logs com todos os níveis ligados, `cargo xtask ci` constrói o `fake-upstream` antes dos testes. O fuzz encontrou um caso: resource com `uri` vazio gerava um URI público não parseável — passa a ser ignorado na descoberta. Seguimento (pontos em aberto da 7a): accept loop próprio (`trunks_api::serve`) com prazo de 30 s para o cabeçalho — incluindo a fase de detecção HTTP/1 vs HTTP/2, onde o hyper espera sem limite —, pausa máxima de 30 s no corpo MCP, ping HTTP/2 e shutdown com 10 s de tolerância; stderr dos upstreams stdio capturado e registado com redacção dos valores de `env` (antes era herdado: ia directo para o stderr do Trunks, com os segredos que o upstream imprimisse); aviso no arranque quando o log está em `debug`/`trace`; upstreams stdio corridos como árvore de processos (process group / job object) e mortos por inteiro no fim da sessão — a spec já o dizia, mas só o filho directo era morto. Corrida de fuzz de 1 h por target feita (workflow `fuzz` manual, run 42792966752): ~74,8 M e ~75,6 M execuções, sem falhas.
  Entregue (7b): suite de conformidade caixa-preta contra o binário (`crates/trunks/tests/conformance`), matriz {development, enterprise} × {sqlite, postgres}, com reinício do servidor a meio de cada cenário; as células PostgreSQL correm obrigatoriamente no job `test-postgres`. Passou à primeira nos dois backends — não apareceu nenhuma divergência de dialecto. O harness diferencial contra a implementação legacy **não foi feito**: sem o binário legacy não há oráculo; a conformidade é medida contra a especificação.
- Performance: benchmark `tools/list` com 500 tools e `call_tool` stateless stdio; publicar números.
  Entregue (7c): `cargo bench -p trunks --bench gateway` (binário real, build optimizado, cliente MCP sobre Streamable HTTP) e os números em `docs/07-desempenho.md`. O benchmark encontrou um problema real: sem `TCP_NODELAY` nas ligações aceites, cada resposta MCP pequena demorava ~44 ms para clientes que reutilizam a ligação (uma chamada com sessão stateful passava de ~0,5 ms para ~44 ms). Corrigido no accept loop, com teste de regressão ao nível do processo.
- Release: `cargo-dist` (release workflow gerado, tag → GitHub Release com binários, instaladores, checksums), Dockerfiles (`distroless/cc` e `-stdio`) num workflow próprio, `docker-compose*.yaml`, `CHANGELOG` com §7 da migração, guia operacional §6.
  Entregue (7d): `Dockerfile` com os targets `distroless` e `stdio`, workflow `docker` (build + smoke test em amd64 e arm64 nos PRs; publicação multi-arquitectura em `ghcr.io/websublime/trunks` nas tags), `docker-compose.yaml`, `docker-compose.prod.yaml` e `.env.example`, `CHANGELOG.md`, guia operacional (`docs/08-operacao.md`). O release de binários por `cargo-dist` já existia. O changelog e o guia descrevem o Trunks em si; o guia de migração de instalações anteriores fica no documento interno. A publicação propriamente dita só acontece com a primeira tag `vX.Y.Z` — esse caminho não foi exercitado. Ao escrever o guia ficaram à vista três pontos da especificação por implementar, entretanto implementados: `TRUNKS_DATABASE_URL_FILE`, as métricas `trunks_upstream_sessions_active` e `trunks_upstream_connect_errors_total`, e a versão com o commit de origem.
- Documentação: README, site de docs (quickstart, guias, referência CLI/API/config).

**Saída:** v1.0.

### Fase 8 — Pós-v1.0

Movido para `05-roadmap.md` (v1.x operabilidade, v2.0 OIDC, v2.x organizações e equipas, v3 governança). Resumo:

1. Tokens hasheados (`access_token_hash`, migração in-place, flag de compat para leitura dupla durante a transição).
2. `trunks apply -d <dir>` — reconciliação declarativa (inverso de `export`).
3. Prompts e resources em tool groups. (A filtragem de prompts/resources na listagem em enterprise foi antecipada para a v1: listagens e erros MCP deixaram de revelar o que está fora do alcance do cliente, como na REST API.)
4. `--output json` no CLI.
5. OTLP export; audit log estruturado por chamada (quem, que tool, outcome) — base para roadmap #4.
6. Users não-admin poderem registar servidores/grupos próprios.
7. Health checks activos de sessões stateful e reconnect com backoff.
8. OAuth downstream (OIDC) — só depois de v1 estável.

## 4. Cronograma agregado

| Fase | Esforço (sem.) | Acumulado | Marco |
|---|---|---|---|
| 0 Spikes | 1,5 | 1,5 | M0: hipóteses provadas, CI verde, D1/D4 fechadas |
| 1 Núcleo | 2 | 3,5 | **M1 atingido**: leitura completa sobre DB existente |
| 2 Gateway | 3 | 6,5 | M2: **utilizável em dev mode** (dogfooding começa aqui) |
| 3 Enterprise | 1,5 | 8 | M3: auth + ACL |
| 4 Grupos | 1 | 9 | M4 |
| 5 OAuth | 1,5 | 10,5 | M5 |
| 6 Dashboard API + web app v0 | 3 | 13,5 | M6: feature-complete |
| 7 Release | 2 | 15,5 | **v1.0** |

≈ 15–16 semanas-pessoa. Com disponibilidade de ~40 %, aponta para ~9 meses de calendário; com 100 %, ~4 meses. O risco de derrapagem concentra-se nas fases 2 e 5.

Dependências entre fases: 0 → 1 → 2 → {3, 4, 5, 6 em qualquer ordem, mas 3 antes de 4 para testar "grupo não alarga ACL"} → 7.

## 5. Definição de "done" por fase

Uma fase só fecha quando **todos** se verificam:

1. Os testes de contrato da área existem (tabelas de casos) e passam.
2. Os e2e/scripts da área passam contra o binário em {sqlite, postgres} e nos modos aplicáveis.
3. Harness diferencial sem diffs não justificados nas rotas da área.
4. `cargo clippy -D warnings`, `cargo fmt --check`, `cargo deny check` limpos; zero `unwrap`/`expect` em código de request path (lint `clippy::unwrap_used` em `trunks-core`/`trunks-api`).
5. Nenhum segredo em logs (teste que grava logs ao nível `trace` durante os e2e e faz `grep` dos tokens usados).
6. Secção correspondente de `02-especificacao.md` actualizada se a implementação divergiu.

## 6. Estrutura de trabalho e ferramentas

- **Repositório**: `websublime/trunks` (ou nome final de D1), `main` protegido, PRs por item de fase, squash.
- **Issues**: um épico por fase; itens da tabela como issues; labels `parity`, `breaking`, `improvement`.
- **CI** (GitHub Actions): `lint`, `test` (matriz OS × DB; Postgres via service container), `conformance` (precisa de `npx`; Node 20 no runner), `differential` (usa a implementação legacy como oráculo, ver doc interno), `build` (cargo-dist dry-run).
- **Fixtures**: `tests/fixtures/legacy.{sqlite,sql}` geradas por script (3 servidores: stdio, http, http+headers; 2 grupos, 2 users, 2 clients) — commitadas, regeneráveis (script no doc interno).
- **Dogfooding**: a partir de M2, Trunks passa a ser o gateway do próprio ambiente de desenvolvimento (Claude Code + Cursor) — melhor detector de regressões de protocolo.

## 7. Riscos de plano

| Risco | Sinal de alerta | Resposta |
|---|---|---|
| `rmcp` quebra API numa minor | `cargo update` falha a compilar | Pin exacto; `UpstreamConn`/`GatewayHandler` isolam o SDK; actualizar só em fases de release |
| Fase 2 derrapa (stdio lifecycle, protocolo com clientes reais) | >4 sem. | Cortar `stateful` para Fase 3; priorizar `stateless` + clientes reais |
| OAuth consome mais que 1,5 sem. | DCR/refresh com ASs reais (GitHub, Atlassian) a falhar | Mecânica validada no S3 contra um AS fake; o risco restante é a variação entre IdPs reais → smoke com 2 IdPs reais na Fase 5 |
| Fidelidade de shapes insuficiente | Diffs recorrentes no harness | Aumentar cobertura do harness antes de continuar; é mais barato que corrigir depois |
| Disponibilidade intermitente | Fases a arrastar sem marcos | Marcos pequenos e demonstráveis; M2 é o ponto a partir do qual já há valor mesmo que o resto pare |

## 8. Primeiras acções (próxima sessão de trabalho)

1. ~~Análise D11~~ feita (`06-analise-db.md`).
2. ~~Repositório, workspace, `snafu`, `cargo dist`, CI~~ feito.
3. ~~Fixtures legacy~~ feitas (sintéticas; regenerar com binário real quando possível).
4. ~~Spike S1~~ feito — sem plano B.
5. ~~Spike S3~~ feito. **Fase 0 concluída.**
6. ~~Fase 1~~ feita. ~~Fase 2~~ feita (M2). ~~Fase 3~~ feita. ~~Fase 4~~ feita. ~~Fase 5~~ feita — Fase 6 em curso: ~~6a~~, ~~6b~~, ~~6d~~ feitas; a 6c (web app) fica adiada até a `line://ui` ter mais componentes. Fase 7 em curso: ~~7a~~, ~~7b~~, ~~7c~~, ~~7d~~.
