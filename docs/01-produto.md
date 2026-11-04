# Trunks — Documento de Produto

> Versão 2.0 (2026-11-04). Nome: **Trunks**. Licença: MPL-2.0 (provisória, D12).
>
> Vocabulário: Trunks v1 consegue abrir e normalizar os dados de uma geração anterior de gateway, referida nestes documentos simplesmente como **legacy** (D3). É uma funcionalidade de migração, não a identidade do produto.

---

## 1. O que é

Trunks é um **MCP gateway self-hosted**: um único processo que se coloca entre os clientes de IA (Claude, Cursor, Copilot, Codex, agentes próprios) e um conjunto arbitrário de servidores MCP (*upstreams*). Em vez de configurar cada servidor MCP em cada cliente, regista-se cada servidor uma vez no gateway e todos os clientes ligam-se a **um só endpoint MCP**.

O produto é simultaneamente:

1. **Registry** — catálogo persistente dos servidores MCP registados e das suas tools, prompts e resources.
2. **Gateway/proxy** — endpoint MCP (Streamable HTTP) que agrega tudo o que está registado e encaminha cada chamada para o upstream correcto.
3. **Control plane** — REST API + CLI + web app para gerir o registry, identidades, controlo de acesso e observabilidade.

Escrito em Rust: binário único, arranque instantâneo, sem GC, footprint pequeno — o perfil certo tanto para a máquina de um developer como para infraestrutura partilhada.

## 2. Problema que resolve

Sem gateway, com N servidores MCP e M clientes:

- Cada cliente precisa das suas N configurações (comandos, URLs, tokens) — N×M configurações duplicadas.
- As credenciais dos upstreams (bearer tokens, API keys) vivem em cada máquina de cada utilizador.
- Não há vista partilhada do que existe nem controlo de quem acede a quê.
- O contexto dos modelos enche-se com dezenas de tools irrelevantes para a tarefa.
- Setups locais não são reproduzíveis.

Com gateway:

- **1 endpoint** para todos os clientes; credenciais dos upstreams ficam no gateway.
- **Discovery unificado** de tools/prompts/resources com nomes canónicos sem colisões.
- **Tool groups**: endpoints MCP dedicados que expõem só um subconjunto curado de tools.
- **Controlo de acesso** por cliente (allow-lists) em modo enterprise; por equipas e indivíduos no roadmap.
- **Observabilidade** central (métricas Prometheus por tool/servidor).

## 3. Para quem

| Persona | Cenário | O que usa |
|---|---|---|
| **Developer individual** | Tem 5–15 servidores MCP (filesystem, github, context7, …) e usa 2–3 clientes de IA. Quer um setup limpo e reproduzível. | Modo *development*, SQLite, CLI + web app, `docker compose up`. |
| **Platform/AI engineer de uma equipa** | Opera MCP como infraestrutura partilhada. Precisa de autenticação, allow-lists por cliente, métricas, Postgres. | Modo *enterprise*, Postgres, CLI, `/metrics`. |
| **Autor de agentes** | Constrói agentes que precisam de um conjunto fixo e pequeno de tools. | Tool groups com endpoint próprio. |
| **Admin/ops** | Faz deploy, rotação de tokens, GitOps dos registos. | `export`, ficheiros JSON declarativos, imagens Docker, env vars. |

## 4. Conceitos centrais

### 4.1 Upstream MCP server
Qualquer servidor MCP registado no gateway. Transportes suportados:

- **`streamable_http`** — remoto, via HTTP. Auth por bearer token estático, headers customizados ou OAuth 2.0.
- **`stdio`** — processo local (`command`, `args`, `env`) lançado pelo gateway.

O transporte SSE (deprecado no spec MCP) não é suportado (D4); registos legacy com `sse` são detectados no arranque com erro accionável.

### 4.2 Nomes canónicos
Para evitar colisões entre servidores, o gateway expõe:

- Tools e prompts como `<server>__<nome>` (separador duplo underscore; split no **primeiro** `__`).
- Resources como `trunks://res/<server>/<base64url-sem-padding(uri-original)>`.

Nomes de servidor obedecem a `^[a-zA-Z0-9_-]+$`, não podem conter `__` nem terminar em `_`.

### 4.3 Modos de operação
Fixados na primeira inicialização; não se alteram depois.

| Capacidade | `development` | `enterprise` |
|---|---|---|
| Autenticação no gateway (`/mcp`) | Desligada | Bearer token de **MCP client** obrigatório |
| Autenticação na REST API | Desligada | Bearer token de **user**; mutações só para `admin` |
| Controlo de acesso | Tudo acessível | Allow-list de servidores por MCP client |
| Web app + `/api/dashboard` | Disponível | **404** (não existe) |
| Métricas Prometheus | Off por defeito | On por defeito |
| Inicialização | Automática no arranque | Manual (`init-server` → devolve token de admin uma única vez) |

`production` é aceite como alias deprecado de `enterprise`.

### 4.4 Identidades (só enterprise)
- **Users** (humanos): `admin` ou `user`. Acedem à REST API/CLI. `user` só lê e invoca; `admin` muta.
- **MCP clients** (máquinas): um token + allow-list de servidores (`"*"` = todos). Acedem ao `/mcp` e aos endpoints de grupos. Tokens de user **não** servem no `/mcp` e vice-versa.

### 4.5 Tool groups
Conjunto curado de tools exposto num endpoint MCP próprio: `/v0/groups/<nome>/mcp`. Definido por `included_tools` ∪ (todas as tools de `included_servers`) − `excluded_tools`; a exclusão ganha sempre. Um grupo **nunca** alarga permissões: a allow-list do cliente aplica-se igualmente. Se uma tool incluída é desactivada ou o servidor removido, desaparece do grupo automaticamente. Em v1 grupos expõem **só tools**; prompts e resources em grupos estão no roadmap.

### 4.6 Sessões upstream
Por servidor registado, `session_mode`:
- **`stateless`** (defeito): nova ligação por chamada; para stdio significa um processo novo por chamada.
- **`stateful`**: ligação persistente reutilizada; fecha no shutdown, no deregister ou após `SESSION_IDLE_TIMEOUT_SEC` de inactividade.

As sessões stateful são **por servidor**, não por cliente downstream.

## 5. Funcionalidades v1

### 5.1 Gestão do registry
- Registar servidor (`streamable_http` via flags ou JSON; `stdio` via JSON) com `--force` para substituir.
- Deregistar, activar/desactivar servidor (cascata para tools/prompts/resources).
- Activar/desactivar tools e prompts individualmente ou por servidor.
- Listar/consultar servidores, tools, prompts, resources (filtro por servidor ou grupo).
- Invocar tool, renderizar prompt com argumentos, ler resource — pela REST API/CLI.
- `usage <tool>` mostra o input schema de forma legível.
- `export` grava a configuração de servidores e grupos como ficheiros JSON (GitOps).
- Placeholders `${VAR}` em qualquer string dos ficheiros JSON de configuração.

### 5.2 Gateway MCP
- Endpoint principal `/mcp` (Streamable HTTP, spec MCP 2026-07-28) com tools, prompts e resources agregados.
- Endpoints por tool group.
- Headers do pedido downstream **nunca** são encaminhados ao upstream; as credenciais configuradas no registo (bearer/headers/OAuth) são aplicadas pelo gateway.
- Reescrita bidireccional de nomes e URIs (canónico ↔ upstream).
- Métricas por chamada: `trunks_tool_calls_total` e `trunks_tool_call_latency_seconds` com labels `mcp_server_name`, `tool_name`, `outcome`.

### 5.3 Autenticação de upstreams
- Bearer token estático; headers customizados (um header `Authorization` explícito ganha sobre `bearer_token`).
- OAuth 2.0 Authorization Code + PKCE, com Dynamic Client Registration quando o upstream o suporta, ou `client_id`/`client_secret` fornecidos. Fluxo em duas fases (bootstrap → autorização no browser → complete), sessão pendente com TTL de 10 min. Em v1 os tokens são **gateway-scoped** (um grant partilhado por todos os clientes downstream); credenciais por identidade estão no roadmap. Refresh automático.

### 5.4 Governança (enterprise)
- `init-server`, `login`, `whoami`.
- CRUD de users e MCP clients; tokens gerados (32 bytes, base64url) ou fornecidos (≥8 chars, sem whitespace); token lido de env ou ficheiro em configs JSON.
- Allow-lists aplicadas na listagem de tools e em **toda** invocação (tools, prompts, resources), nos endpoints global e de grupo.

### 5.5 Web app (development)
Web app em `line://ui` embebida no binário: overview, servidores (registar incl. OAuth via popup+polling, apagar, toggle), tools/prompts (toggle), tool groups (criar/apagar), resources, diagnósticos. API própria em `/api/dashboard/*`, só leitura de sumários **sanitizados** (nunca expõe tokens, valores de env, secrets OAuth). A partir de v2.0 torna-se a consola autenticada (ver `05-roadmap.md`).

### 5.6 Operação
- Binário único: `start` corre o servidor; o resto é CLI cliente da REST API.
- SQLite (defeito, `sqlite://trunks.db`) ou PostgreSQL, via `TRUNKS_DATABASE_URL`.
- Configuração por flags > env vars > defaults; `.env` carregado automaticamente.
- Imagens Docker: `latest` (distroless) e `latest-stdio` (com Node 20 + `uv`/`uvx` para upstreams stdio).
- Binários para Linux/macOS/Windows × amd64/arm64 via `cargo-dist`; instaladores shell/PowerShell; Homebrew tap.
- Shutdown gracioso: fecha sessões stateful, drena HTTP em 10 s.

## 6. Fora de âmbito em v1

- OAuth/SSO/OIDC para **clientes downstream** (só tokens estáticos) — v2.0 no roadmap.
- Prompts e resources em tool groups — v1.x.
- Em enterprise, só admins registam servidores e criam grupos — resolvido pelas organizações em v2.x.
- Sem OTLP export; sem audit logs detalhados — v3.
- Sem suporte a sampling, elicitation, roots, notificações `list_changed`.
- Sem TLS nativo (espera-se reverse proxy).
- Sem transporte SSE.

## 7. Princípios de engenharia

1. **Fail-closed por construção.** O *principal* autenticado é um tipo que o handler recebe obrigatoriamente; a sua ausência em modo enterprise significa "nada visível, nada invocável".
2. **Segredos modelados em tipos** (`SecretString`), nunca em strings soltas; o strip de headers downstream é estrutural (a ligação upstream não tem acesso ao pedido).
3. **Estado derivado em memória, verdade na DB.** O catálogo é um snapshot imutável reconstruído após cada mutação; a DB só é tocada em mutações e no arranque.
4. **Contratos congelados por major.** `/api/v0`, o CLI e o schema são estáveis dentro de 1.x; funcionalidade nova vive em `/api/v1`.
5. **Sem ORM, sem magia.** SQL explícito em dois dialectos, migrações versionadas e idempotentes.

## 8. Decisões

| # | Decisão | Estado |
|---|---|---|
| D1 | **Nome: Trunks.** Binário `trunks`, crates `trunks-*`, env `TRUNKS_*`, config `~/.trunks.conf`. Existe um crate `trunks` no crates.io (lib de load testing, sem binário); não publicamos a raiz no crates.io — distribuição por `cargo-dist` — e os crates publicáveis usam prefixo `trunks-`. | Fechada |
| D2 | **Web app em `line://ui`** (Lit + Zag.js Web Components, `@websublime/line-*`). v1.0 entrega a web app v0 contra `/api/dashboard`; a partir de v2.0 torna-se a consola autenticada. | Fechada |
| D3 | **Compatibilidade de dados legacy**: Trunks abre uma DB de um deployment anterior e normaliza-a por migração no arranque (URIs `trunks://`, modos, índices). Só dados — nomes de env vars, ficheiros, métricas e esquemas são exclusivamente os de Trunks. Detalhe no doc interno de migração. | Proposto |
| D4 | **SSE não suportado.** `rmcp` 3.x não tem SSE e o spec deprecou-o. Arranque falha com erro accionável se a DB tiver servidores `sse` activos (override `TRUNKS_IGNORE_SSE_SERVERS=1`). | Fechada |
| D5 | Nenhuma particularidade de implementação legacy ligada a SSE é preservada. | Fechada |
| D6 | **Nomes próprios, sem aliases legacy**: todas as env vars com prefixo `TRUNKS_*` (`TRUNKS_MODE`, `TRUNKS_DATABASE_URL`, `TRUNKS_PORT`, …), config do CLI em `~/.config/trunks/config.toml`, DB por defeito `trunks.db`, URIs de resources `trunks://res/…`, modos só `development`/`enterprise`. | Fechada |
| D7 | **Métricas** `trunks_tool_calls_total` / `trunks_tool_call_latency_seconds` (+ `trunks_upstream_sessions_active`, `trunks_upstream_connect_errors_total`, `trunks_build_info`). | Fechada |
| D8 | **Melhorias pós-v1.0** geridas em `05-roadmap.md`. | Proposto |
| D9 | **Distribuição com `cargo-dist`** (binários multi-plataforma, archives, checksums, Homebrew tap, instaladores shell/PowerShell). Docker via GitHub Actions. | Fechada |
| D10 | **Erros com `snafu`** em todos os crates (context selectors, `Whatever` no binário, `ensure!`). | Fechada |
| D11 | **Acesso a dados sem ORM.** A lib concreta (sqlx com SQL manual vs. sqlx + `sea-query` vs. drivers nativos) é decidida numa análise dedicada antes da Fase 1. | Em análise |
| D12 | **Licença: MPL-2.0** (provisória). Decidir antes do primeiro release público se se alinha com o resto do `websublime` (Apache-2.0/MIT). | Em aberto |

## 9. Critérios de sucesso v1.0

- Suite de conformidade passa contra o binário em ambos os modos e ambas as DBs.
- Um deployment legacy (Postgres) migra para Trunks trocando o binário/imagem e a configuração (env vars `TRUNKS_*`); os dados são normalizados no arranque; clientes configurados (Cursor, Claude via `mcp-remote`, Copilot) não precisam de alterar configuração.
- Binário release < 25 MB; arranque com 20 servidores registados < 500 ms (excluindo connect a upstreams); RSS em idle < 50 MB.
- Zero `unwrap`/`expect` em caminhos de request; `cargo clippy -D warnings`; `cargo deny` limpo.

## 10. Referências

- Spec MCP: https://modelcontextprotocol.io/specification/2026-07-28
- SDK Rust MCP: https://docs.rs/rmcp
- `line://ui`: https://line-ui.websublime.com

Documentos relacionados: `02-especificacao.md`, `03-plano.md`, `04-arquitectura.md`, `05-roadmap.md`.
