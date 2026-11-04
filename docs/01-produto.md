# Trunks — Documento de Produto

> Reescrita em Rust do [MCPJungle](https://github.com/mcpjungle/mcpjungle) (Go, v0.4.x, licença MPL-2.0).
> Nome: **Trunks**. Versão deste documento: 1.1 (2026-11-03).
> Base analisada: commit `49bf232` de 2026-11-03.

---

## 1. O que é

Trunks é um **MCP gateway self-hosted**: um único processo que se coloca entre os clientes de IA (Claude, Cursor, Copilot, Codex, agentes próprios) e um conjunto arbitrário de servidores MCP (*upstreams*). Em vez de configurar cada servidor MCP em cada cliente, regista-se cada servidor uma vez no gateway e todos os clientes ligam-se a **um só endpoint MCP**.

O produto é simultaneamente:

1. **Registry** — catálogo persistente dos servidores MCP registados e das suas tools, prompts e resources.
2. **Gateway/proxy** — endpoint MCP (Streamable HTTP) que agrega tudo o que está registado e encaminha cada chamada para o upstream correcto.
3. **Control plane** — REST API + CLI + dashboard web para gerir o registry, identidades, controlo de acesso e observabilidade.

Trunks mantém **paridade funcional** com o MCPJungle (mesma REST API, mesmo CLI, mesma base de dados, mesmos conceitos) e corrige um conjunto definido de fragilidades estruturais do original. Não é um produto novo; é a mesma proposta de valor com uma implementação mais robusta e mais barata de operar.

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
- **Controlo de acesso** por cliente (allow-lists) em modo enterprise.
- **Observabilidade** central (métricas Prometheus por tool/servidor).

## 3. Para quem

| Persona | Cenário | O que usa |
|---|---|---|
| **Developer individual** | Tem 5–15 servidores MCP (filesystem, github, context7, …) e usa 2–3 clientes de IA. Quer um setup limpo e reproduzível. | Modo *development*, SQLite, CLI + dashboard, `docker compose up`. |
| **Platform/AI engineer de uma equipa** | Opera MCP como infraestrutura partilhada. Precisa de autenticação, allow-lists por cliente, métricas, Postgres. | Modo *enterprise*, Postgres, CLI, `/metrics`. |
| **Autor de agentes** | Constrói agentes que precisam de um conjunto fixo e pequeno de tools. | Tool groups com endpoint próprio. |
| **Admin/ops** | Faz deploy, rotação de tokens, GitOps dos registos. | `export`, ficheiros JSON declarativos, imagens Docker, env vars. |

## 4. Conceitos centrais

### 4.1 Upstream MCP server
Qualquer servidor MCP registado no gateway. Três transportes:

- **`streamable_http`** — remoto, via HTTP. Auth por bearer token estático, headers customizados ou OAuth 2.0 (beta).
- **`stdio`** — processo local (`command`, `args`, `env`) lançado pelo gateway.
- **`sse`** — transporte legacy; deprecado no spec MCP. *Ver decisão em §8.*

### 4.2 Nomes canónicos
Para evitar colisões entre servidores, o gateway expõe:

- Tools e prompts como `<server>__<nome>` (separador duplo underscore; split no **primeiro** `__`).
- Resources como `mcpj://res/<server>/<base64url-sem-padding(uri-original)>`.

Nomes de servidor obedecem a `^[a-zA-Z0-9_-]+$`, não podem conter `__` nem terminar em `_`.

### 4.3 Modos de operação
Fixados na primeira inicialização; não se alteram depois.

| Capacidade | `development` | `enterprise` |
|---|---|---|
| Autenticação no gateway (`/mcp`) | Desligada | Bearer token de **MCP client** obrigatório |
| Autenticação na REST API | Desligada | Bearer token de **user**; mutações só para `admin` |
| Controlo de acesso | Tudo acessível | Allow-list de servidores por MCP client |
| Dashboard web + `/api/dashboard` | Disponível | **404** (não existe) |
| Métricas OTel/Prometheus | Off por defeito | On por defeito |
| Inicialização | Automática no arranque | Manual (`init-server` → devolve token de admin uma única vez) |

`production` é aceite como alias deprecado de `enterprise`.

### 4.4 Identidades (só enterprise)
- **Users** (humanos): `admin` ou `user`. Acedem à REST API/CLI. `user` só lê e invoca; `admin` muta.
- **MCP clients** (máquinas): um token + allow-list de servidores (`"*"` = todos). Acedem ao `/mcp` e aos endpoints de grupos. Tokens de user **não** servem no `/mcp` e vice-versa.

### 4.5 Tool groups
Conjunto curado de tools exposto num endpoint MCP próprio: `/v0/groups/<nome>/mcp`. Definido por `included_tools` ∪ (todas as tools de `included_servers`) − `excluded_tools`; a exclusão ganha sempre. Um grupo **nunca** alarga permissões: a allow-list do cliente aplica-se igualmente. Se uma tool incluída é desactivada ou o servidor removido, desaparece do grupo automaticamente. Hoje grupos expõem **só tools** (não prompts/resources) — limitação conhecida do original.

### 4.6 Sessões upstream
Por servidor registado, `session_mode`:
- **`stateless`** (defeito): nova ligação por chamada; para stdio significa um processo novo por chamada.
- **`stateful`**: ligação persistente reutilizada; fecha no shutdown, no deregister ou após `SESSION_IDLE_TIMEOUT_SEC` de inactividade.

As sessões stateful são **por servidor**, não por cliente downstream.

## 5. Funcionalidades (inventário de paridade)

### 5.1 Gestão do registry
- Registar servidor (`streamable_http` via flags ou JSON; `stdio` só via JSON) com `--force` para substituir.
- Deregistar, activar/desactivar servidor (cascata para tools/prompts/resources).
- Activar/desactivar tools e prompts individualmente ou por servidor.
- Listar/consultar servidores, tools, prompts, resources (filtro por servidor ou grupo).
- Invocar tool, renderizar prompt com argumentos, ler resource — pela REST API/CLI.
- `usage <tool>` mostra o input schema de forma legível.
- `export` grava a configuração de servidores e grupos como ficheiros JSON (GitOps).
- Placeholders `${VAR}` em qualquer string dos ficheiros JSON de configuração.

### 5.2 Gateway MCP
- Endpoint principal `/mcp` (Streamable HTTP) com tools, prompts e resources agregados.
- Endpoints por tool group.
- Headers do pedido downstream **nunca** são encaminhados ao upstream; as credenciais configuradas no registo (bearer/headers/OAuth) são aplicadas pelo gateway.
- Reescrita bidireccional de nomes e URIs (canónico ↔ original).
- Métricas por chamada: `tool_calls_total` e `tool_call_latency_seconds` com labels `mcp_server_name`, `tool_name`, `outcome`.

### 5.3 Autenticação de upstreams
- Bearer token estático; headers customizados (um header `Authorization` explícito ganha sobre `bearer_token`).
- OAuth 2.0 Authorization Code + PKCE, com Dynamic Client Registration quando o upstream o suporta, ou `client_id`/`client_secret` fornecidos. Fluxo em duas fases (bootstrap → autorização no browser → complete), sessão pendente com TTL de 10 min. Tokens são **gateway-scoped** (um grant partilhado por todos os clientes downstream). Refresh automático.

### 5.4 Governança (enterprise)
- `init-server`, `login`, `whoami`.
- CRUD de users e MCP clients; tokens gerados (32 bytes, base64url) ou fornecidos (≥8 chars, sem whitespace); token lido de env ou ficheiro em configs JSON.
- Allow-lists aplicadas na listagem de tools e em **toda** invocação (tools, prompts, resources), nos endpoints global e de grupo.

### 5.5 Dashboard (development)
UI React embebida no binário: overview, servidores (registar incl. OAuth via popup+polling, apagar, toggle), tools/prompts (toggle), tool groups (criar/apagar), resources, diagnósticos. API própria em `/api/dashboard/*`, só leitura de sumários **sanitizados** (nunca expõe tokens, valores de env, secrets OAuth).

### 5.6 Operação
- Binário único: `start` corre o servidor; o resto é CLI cliente da REST API.
- SQLite (defeito, ficheiro local) ou PostgreSQL (`DATABASE_URL` ou `POSTGRES_*`).
- Configuração por flags > env vars > defaults; `.env` carregado automaticamente.
- Imagens Docker: `latest` (distroless) e `latest-stdio` (com Node 20 + `uv`/`uvx` para upstreams stdio).
- Binários para Linux/macOS/Windows × amd64/arm64; Homebrew tap.
- Shutdown gracioso: fecha sessões stateful, drena HTTP em 10 s.

## 6. O que fica fora de âmbito (herdado e mantido)

Limitações documentadas do original que o rewrite **não** se compromete a resolver na fase de paridade:

- OAuth/SSO/OIDC para **clientes downstream** (só tokens estáticos).
- Prompts e resources em tool groups.
- Em enterprise, só admins registam servidores e criam grupos.
- Sem OTLP export; sem audit logs detalhados.
- Sem suporte a sampling, elicitation, roots, notificações `list_changed` (não existem no original).
- Sem TLS nativo (espera-se reverse proxy).

## 7. Porquê reescrever em Rust

Argumentos que justificam o custo, por ordem de peso:

1. **Modelo de concorrência e ownership.** O original tem estado em memória espalhado por quatro tipos de proxy servers, três `RwMutex` com ordem de aquisição assimétrica, callbacks sem lock, e type assertions não verificadas em `context.Value` que fazem panic se o middleware não injectar `"mode"`/`"client"`. Em Rust, o *principal* autenticado é um tipo que o handler recebe obrigatoriamente, e um grupo é um filtro sobre um catálogo em memória em vez de um servidor MCP duplicado.
2. **Footprint e arranque.** Gateway que lança processos stdio por chamada e corre em ambientes partilhados beneficia de binário pequeno, sem GC, arranque instantâneo. É também o perfil típico de ferramenta "instala e esquece" em máquina de developer.
3. **Correcção por construção nos pontos críticos de segurança**: strip de headers, fail-closed do filtro, não vazar secrets no dashboard — modelados em tipos (`Secret<String>`, newtypes para canonical names) em vez de convenção.
4. **Dívida acumulada no original** que um rewrite resolve de raiz: tokens em plaintext, operações multi-tabela sem transacção, `GetMcpServer` à DB em cada chamada, lock global durante o connect de sessões stateful, servidor Streamable HTTP alocado por pedido nos grupos, `Updates` do GORM que ignora zero-values ao actualizar grupos.
5. **Ecossistema**: o SDK Rust oficial (`rmcp` 3.8, Out 2026) já implementa o spec MCP 2026-07-28 (sessões stateless por defeito), o que alinha com o modelo do gateway.

Contra-argumentos que devem ficar registados: o original está activo e a receber features (OAuth beta, dashboard); a reescrita arranca sempre atrás; o SDK `rmcp` abandonou o transporte SSE (D4); a equipa (Miguel) tem mais quilometragem em TypeScript do que em Rust — o plano deve incluir spikes para desriscar `rmcp` antes de comprometer.

## 8. Decisões de produto para o rewrite

| # | Decisão | Estado |
|---|---|---|
| D1 | **Nome: Trunks.** Binário `trunks`, crates `trunks-*`, env `TRUNKS_*`, config `~/.trunks.conf`. Nota: existe um crate `trunks` no crates.io (lib de load testing, sem binário); não publicamos a raiz no crates.io — distribuição é por `cargo-dist` (GitHub Releases, Homebrew, Docker) — e os crates publicáveis usam prefixo `trunks-`. | **Fechada** |
| D2 | **Dashboard React/TypeScript mantém-se** tal como está e é embebido no binário Rust. Reescrever UI em Rust não acrescenta valor. | Proposto |
| D3 | **Compatibilidade de REST API, CLI e schema de DB** com MCPJungle 0.4.x: um utilizador aponta Trunks à DB existente e os clientes/CLIs/dashboards continuam a funcionar. Isto é a estratégia de migração principal. | Proposto |
| D4 | **SSE legacy não é suportado.** `rmcp` 3.x não tem SSE e o spec deprecou-o. Trunks não expõe `/sse`/`/message` nem liga a upstreams `sse`; arranque falha com erro accionável se a DB tiver servidores `sse` activos (override `TRUNKS_IGNORE_SSE_SERVERS=1`), com documentação de migração para `streamable_http`. | **Fechada** |
| D5 | **Quirk SSE/non-SSE do original não é preservado.** Irrelevante dado D4. | Fechada |
| D6 | **Nomes de env vars mantêm-se** (`SERVER_MODE`, `DATABASE_URL`, `PORT`, …) por compatibilidade; aliases `TRUNKS_*` podem vir depois. Ficheiro de config do CLI: `~/.trunks.conf`, com fallback de leitura a `~/.mcpjungle.conf`. | Proposto |
| D7 | **Nomes de métricas**: o original expõe `mcpjungle_tool_calls_ratio_total` (sufixo `_ratio` é artefacto do exporter OTel). Trunks expõe `trunks_tool_calls_total` / `trunks_tool_call_latency_seconds` e, por flag, aliases com os nomes antigos durante a transição. | Proposto |
| D8 | **Melhorias pós-paridade** (não bloqueiam a v1): hash de tokens em repouso (SHA-256; migração in-place possível porque o lookup é exact-match), transacções nas operações multi-tabela, `apply` declarativo (inverso do `export`), prompts/resources em grupos, filtro de prompts/resources na listagem em enterprise. | Proposto |
| D9 | **Distribuição com `cargo-dist`** (binários multi-plataforma, archives, checksums, Homebrew tap, instaladores shell/PowerShell). Docker via GitHub Actions. | **Fechada** |
| D10 | **Erros com `snafu`** em todos os crates (context selectors, `Whatever` no binário, `ensure!`), sem `thiserror`/`anyhow`. | **Fechada** |
| D11 | **Acesso a dados sem ORM.** sea-orm/diesel excluídos. A lib concreta (sqlx com SQL manual vs. sqlx + `sea-query` como query builder vs. drivers nativos) é decidida numa análise dedicada antes da Fase 1. | **Em análise** |
| D12 | **Licença: MPL-2.0** para o repositório inteiro. O original é MPL-2.0 e o `web/dashboard` é copiado sem alterações, logo esses ficheiros têm de permanecer MPL-2.0 com atribuição; licenciar o resto igual evita um repo com duas licenças. Reavaliável para o código Rust (Apache/MIT) se se quiser, mantendo MPL-2.0 no dashboard. | Proposto |

## 9. Critérios de sucesso

- Suite de conformidade (os testes e2e e o `scripts/test-mcpjungle.sh` do original, adaptados) passa contra o binário Trunks em ambos os modos e ambas as DBs.
- Um deployment MCPJungle existente (Postgres) migra para Trunks **sem** passo de migração de dados, só troca de binário/imagem.
- Clientes configurados (Cursor, Claude via `mcp-remote`, Copilot) não precisam de alterar configuração.
- Binário release < 25 MB; arranque com 20 servidores registados < 500 ms (excluindo connect a upstreams); RSS em idle < 50 MB.
- Zero `unwrap`/`expect` em caminhos de request; `cargo clippy -D warnings`; `cargo deny` limpo.

## 10. Referências

- Repositório original: https://github.com/mcpjungle/mcpjungle
- Documentação: https://docs.mcpjungle.com
- SDK Rust MCP: https://docs.rs/rmcp (3.8.0, 2026-10-26)
- Spec MCP: https://modelcontextprotocol.io/specification/2026-07-28

Documentos relacionados neste projecto: `02-especificacao.md`, `03-migracao.md`, `04-plano.md`.
