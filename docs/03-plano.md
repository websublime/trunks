# Trunks — Plano de Execução

> Versão 2.0 (2026-11-04). Depende de `01-produto.md` (decisões) e `02-especificacao.md` (arquitectura). A estratégia de compatibilidade de dados (D3) vive num doc interno de migração.

---

## 1. Pressupostos

- Um engenheiro (Miguel) a tempo parcial, com apoio de agentes de código. Estimativas em **semanas-pessoa de trabalho efectivo**, não em calendário; multiplicar pelo factor de disponibilidade real.
- Rust stable, `rmcp` 3.8 pinado. Se um spike falhar, o plano B correspondente da spec §14 substitui o item sem alterar as fases.
- Decisões D1 (nome: Trunks), D4 (sem SSE), D9 (`cargo-dist`) e D10 (`snafu`) estão fechadas. D11 (lib de acesso a dados, sem ORM) fecha-se na Fase 0 e condiciona o Spike S2.
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
| S1 `rmcp` server | Protótipo axum + `StreamableHttpService<GatewayHandler>` que lê um `Principal` inserido por middleware em `Parts.extensions` e o usa em `list_tools`. Testar com `legacy_session_mode` on/off contra `mcp-remote` e Claude Code. | Principal chega ao handler em ambos os modos de sessão; se não chegar, plano B (service por principal) protótipado e medido. |
| D11 análise DB | Documento `05-analise-db.md`: comparar `sqlx` (SQL manual; `Any` vs dois backends), `sqlx`+`sea-query`, `rusqlite`+`tokio-postgres` contra os requisitos (dois dialectos, JSON, migrações, bundled SQLite, ergonomia sem ORM, custo de duplicação). | Decisão registada em `01-produto.md`. |
| S2 DB | Com a lib de D11: structs das 10 tabelas; abrir `tests/fixtures/legacy.sqlite` e um Postgres carregado com `tests/fixtures/legacy.sql`; ler todas as linhas; baseline migration `IF NOT EXISTS` no-op sobre ambas; medir quanto SQL é partilhado vs duplicado. | Zero erros de mapeamento; JSON lido como `serde_json::Value` nas duas DBs. |
| S3 OAuth | Com `wiremock` como authorization server: discovery → DCR → authorization URL com `state` nosso → troca de code → refresh. Via `rmcp::transport::auth`; se insuficiente, `oauth2` crate. | Fluxo completo em teste automático; decisão `rmcp auth` vs `oauth2` registada. |
| Workspace | Cargo workspace com os 7 crates vazios, lints (`clippy -D warnings`, `rustfmt`, `cargo-deny`), `snafu` como base de erros em cada crate, CI GitHub Actions (lint, test, build linux/mac/win), `cargo dist init` com `dist-workspace.toml` (targets, instaladores), `xtask build-dashboard`, `web/dashboard` copiado e a buildar. | `cargo test` e CI verdes; `trunks --version` imprime versão; `dist plan` lista os artefactos. |
| Decisões | D11 fechada e registada em `01-produto.md`. | — |

### Fase 1 — Núcleo: arranque, DB, modos, registry read-only (2 sem.)

Objectivo: `trunks start` arranca sobre uma DB legacy existente (normalizada por migração) e serve a parte de leitura da REST API.

- `trunks-types`: enums, newtypes (`ServerName`, `CanonicalName`, `ResourceUri`, `GroupName`) com as tabelas de casos de validação; tipos REST.
- `trunks-db`: structs de linha, repositórios com SQL explícito (lib de D11), `connect()` (`TRUNKS_DATABASE_URL`), migrações baseline + `tool_uniqueness` + `normalize_legacy` (URIs `trunks://`, modo `enterprise`).
- `trunks-core`: `ServerMode` service (`init`, verificação de modo no arranque), `Catalog` construído da DB, `metrics` (noop + prometheus).
- `trunks-api`: router, middleware `require_initialized`/`user_auth`/`require_admin`/`require_enterprise`, `/health`, `/metadata`, `/init`, `/metrics`, rotas GET de `/api/v0/{servers,tools,tool,prompts,prompt,resources}`, erros.
- `trunks`: `start` (config completa, banner, shutdown gracioso), `version`, `list servers|tools|prompts|resources`, `usage`, `--registry`, ficheiro de config.
- Testes: unit das tabelas de nomes; API tests das rotas GET; conformidade: arrancar sobre as fixtures e comparar `list *` com a implementação legacy (harness diferencial v0).

**Saída:** leitura completa. Um operador aponta Trunks a uma DB existente e vê todos os servidores, tools, prompts e resources.

### Fase 2 — Gateway MCP e registry read-write (3 sem.)

Objectivo: fluxo principal completo em modo development.

- `trunks-core::upstream`: `UpstreamConn::connect` para `stdio` (TokioChildProcess, env, stderr→tracing, process group) e `streamable_http` (headers, bearer; sem OAuth ainda); timeout de `initialize` e mensagens de erro accionáveis.
- `trunks-core::sessions`: `SessionManager` stateless/stateful, sweeper de idle, invalidação tipada, shutdown.
- `trunks-core::registry`: `register` (validar → connect → descobrir tools/prompts/resources → transacção), `deregister`, `set_server_enabled`, `set_{tools,prompts,resources}_enabled`; rebuild do catálogo.
- `trunks-core::gateway`: `GatewayHandler{Scope::Global}` com `list_*`/`call_tool`/`get_prompt`/`read_resource`, reescrita de nomes/URIs, strip de headers, métricas.
- `trunks-api`: `ANY /mcp`; `POST /servers` (sem OAuth: devolve erro accionável se o upstream pedir auth), `DELETE /servers/{n}`, `enable|disable` de servidor/tools/prompts, `POST /tools/invoke`, `/prompts/render`, `/resources/{get,read}`, `GET /server_configs`.
- `trunks`: `register` (flags e `-c` com `${VAR}`; sem OAuth), `deregister`, `enable|disable`, `invoke` (incl. gravação de ficheiros), `get prompt|resource`, `export`.
- Testes: strip de headers, filtro em modo dev, session manager (stateless/stateful/idle/invalidação), registo/deregisto/enable, handlers de tool/prompt/resource; e2e modo dev; harness diferencial; smoke manual com Claude Code e Cursor.

**Saída:** Trunks utilizável pelo developer individual (modo dev, sem OAuth, sem grupos).

### Fase 3 — Enterprise: identidades e controlo de acesso (1,5 sem.)

- `trunks-core::identity`: users (admin/user), mcp clients, geração/validação de tokens, allow-lists.
- Middleware `mcp_client_auth`; `Principal::McpClient`; filtragem em `list_tools` e autorização em `call_*` (fail-closed); rejeição de tokens de user no `/mcp`.
- `/init` em enterprise (cria admin, devolve token uma vez); `/api/v0/{clients,users}` CRUD; `/users/whoami`.
- CLI: `init-server`, `login`, `create|delete|update mcp-client|user`, `list mcp-clients|users`.
- Testes: filtro em enterprise, clients, users, middleware; e2e modo enterprise.

### Fase 4 — Tool groups (1 sem.)

- `trunks-core::groups`: resolução, validação, CRUD transaccional, diff, `GroupView` no catálogo, startup degradado.
- `GatewayHandler{Scope::Group}`; rota `/v0/groups/{name}/mcp` com cache de services; endpoints na resposta (sem campos SSE).
- `/api/v0/tool-groups*`; CLI `create|get|list|update|delete group`, `list tools --group`, `invoke --group`.
- Testes: resolução e CRUD de grupos, API, e2e de grupos; verificar que grupo não alarga allow-list.

### Fase 5 — OAuth upstream (1,5 sem.)

- `trunks-core::oauth`: bootstrap (discovery, DCR condicional, PKCE, `state`, sessão pendente com TTL, transacção), `complete`, `CredentialStore` em DB com refresh; integração em `UpstreamConn::connect` (usa tokens guardados quando existem); limpeza no `deregister`.
- API: `202 authorization_required`, `POST /upstream_oauth/sessions/{id}/complete`, `code: upstream_oauth_required`.
- CLI `register`: listener loopback, abrir browser, esperar callback, `complete`; erro claro quando o redirect é fornecido pelo utilizador.
- Testes: OAuth com `wiremock`, e2e de registo OAuth, `force=true` com OAuth.

### Fase 6 — Dashboard API + web app v0 em `line://ui` (3 sem.)

- `trunks-core::dashboard` shaping sanitizado; `/api/dashboard/*` completo incl. `POST /servers` com retry OAuth usando `/api/dashboard/oauth/callback`, página HTML de callback, cache de resultados 10 min, `GET /oauth/session/{id}`. (1 sem.)
- `web/app`: Lit + `line://ui` + Vite/TS; ecrãs: overview, servidores (registar incl. OAuth upstream via popup + polling, apagar, toggle), tools/prompts (toggle), tool groups (criar/apagar), resources, diagnósticos. Dark-first, `line-` components, tokens `--line-*`. (2 sem.)
- `trunks-dashboard` (rust-embed) + `cargo xtask build-dashboard`; rotas estáticas dev-only.
- Testes: e2e da dashboard API (nunca vaza segredos; 404 em enterprise; toggles reflectem em `/mcp`); smoke da web app com Playwright contra o binário.
- Q1 do roadmap: se v1.0 precisar de sair mais cedo, a web app v0 desliza para v1.1 e a fase entrega só a API.

### Fase 7 — Conformidade final, hardening e release v1.0 (2 sem.)

- Harness diferencial completo em CI; suite de conformidade a 100 %; matriz {dev, enterprise} × {sqlite, postgres}.
- Smoke com os 4 clientes reais (Claude Code, Claude Desktop via `mcp-remote`, Cursor, Copilot) em ambos os modos.
- Hardening: `allowed_hosts`, body limits, timeouts por pedido, fuzz de `CanonicalName::parse`/`ResourceUri::parse` (`cargo-fuzz`, 1 h cada), `cargo deny`, revisão de logs por fuga de segredos.
- Performance: benchmark `tools/list` com 500 tools e `call_tool` stateless stdio; publicar números.
- Release: `cargo-dist` (release workflow gerado, tag → GitHub Release com binários, instaladores, checksums), Dockerfiles (`distroless/cc` e `-stdio`) num workflow próprio, `docker-compose*.yaml`, `CHANGELOG` com §7 da migração, guia operacional §6.
- Documentação: README, site de docs (quickstart, guias, referência CLI/API/config).

**Saída:** v1.0.

### Fase 8 — Pós-v1.0

Movido para `05-roadmap.md` (v1.x operabilidade, v2.0 OIDC, v2.x organizações e equipas, v3 governança). Resumo:

1. Tokens hasheados (`access_token_hash`, migração in-place, flag de compat para leitura dupla durante a transição).
2. `trunks apply -d <dir>` — reconciliação declarativa (inverso de `export`).
3. Prompts e resources em tool groups; filtragem de prompts/resources na listagem em enterprise.
4. `--output json` no CLI.
5. OTLP export; audit log estruturado por chamada (quem, que tool, outcome) — base para roadmap #4.
6. Users não-admin poderem registar servidores/grupos próprios.
7. Health checks activos de sessões stateful e reconnect com backoff.
8. OAuth downstream (OIDC) — só depois de v1 estável.

## 4. Cronograma agregado

| Fase | Esforço (sem.) | Acumulado | Marco |
|---|---|---|---|
| 0 Spikes | 1,5 | 1,5 | M0: hipóteses provadas, CI verde, D1/D4 fechadas |
| 1 Núcleo | 2 | 3,5 | M1: leitura completa sobre DB existente |
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
| Spike S1 falha (sem principal por pedido no `rmcp`) | Fim da Fase 0 | Plano B (service por principal) — +0,5 sem. na Fase 3 |
| `rmcp` quebra API numa minor | `cargo update` falha a compilar | Pin exacto; `UpstreamConn`/`GatewayHandler` isolam o SDK; actualizar só em fases de release |
| Fase 2 derrapa (stdio lifecycle, protocolo com clientes reais) | >4 sem. | Cortar `stateful` para Fase 3; priorizar `stateless` + clientes reais |
| OAuth consome mais que 1,5 sem. | DCR/refresh com ASs reais (GitHub, Atlassian) a falhar | Aceitar v1 com `client_id` manual obrigatório; DCR em 1.1 |
| Fidelidade de shapes insuficiente | Diffs recorrentes no harness | Aumentar cobertura do harness antes de continuar; é mais barato que corrigir depois |
| Disponibilidade intermitente | Fases a arrastar sem marcos | Marcos pequenos e demonstráveis; M2 é o ponto a partir do qual já há valor mesmo que o resto pare |

## 8. Primeiras acções (próxima sessão de trabalho)

1. Análise D11 (lib de DB sem ORM) → `05-analise-db.md` → decisão.
2. Criar repositório `trunks`, workspace Cargo, `snafu`, `cargo dist init`, CI mínimo.
3. Gerar as fixtures legacy (`tests/fixtures/`).
4. Spike S1 (`rmcp` + axum + principal) — é o que mais condiciona a arquitectura.
5. Spike S2 em paralelo (lib de D11 sobre as fixtures).
