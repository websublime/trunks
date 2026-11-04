# Trunks — Documento de Arquitectura

> Versão 2.0 (2026-11-04). Complementa `02-especificacao.md` (que detalha contratos e módulos); este documento dá as **vistas** — contexto, contentores, componentes, runtime, dados, deployment — e regista as decisões arquitecturais. Diagramas em Mermaid (renderizam no GitHub e no VS Code).
> Estrutura inspirada em C4 + arc42, sem cerimónia.

---

## 1. Objectivos e restrições arquitecturais

| Objectivo | Como se reflecte na arquitectura |
|---|---|
| Contratos estáveis (REST, CLI, DB) | Camada `trunks-types` isola os shapes JSON; `trunks-db` isola o schema |
| Um só binário, um só processo | `trunks start` liga tudo; CLI é cliente HTTP do mesmo binário |
| Fail-closed por construção | Autorização é um tipo (`Principal`) que o handler recebe obrigatoriamente, não um valor opcional num contexto |
| Baixo custo operacional | Sem GC, arranque < 500 ms, catálogo em memória, DB só para mutações e arranque |
| Isolamento do SDK MCP | `rmcp` só é visível em `trunks-core::gateway` e `trunks-core::upstream`; o resto do domínio não o importa |
| Sem ORM (D11) | Repositórios com SQL explícito atrás de traits; o domínio nunca vê a lib de DB |

Restrições: Rust stable; `rmcp` 3.8 (spec MCP 2026-07-28, sem SSE); dois dialectos SQL (SQLite/Postgres); web app em `line://ui` (D2).

## 2. Vista de contexto (C4 nível 1)

```mermaid
flowchart LR
    subgraph clients["Clientes de IA"]
        claude["Claude Code / Desktop<br/>(via mcp-remote)"]
        cursor["Cursor / Copilot / Codex"]
        agent["Agentes próprios"]
    end
    subgraph humans["Operadores"]
        dev["Developer<br/>(modo development)"]
        admin["Admin / Platform eng.<br/>(modo enterprise)"]
    end

    trunks(["**Trunks**<br/>MCP gateway + registry"])

    subgraph upstreams["Upstream MCP servers"]
        http["Remotos — Streamable HTTP<br/>(context7, GitHub, Atlassian…)"]
        stdio["Locais — stdio<br/>(filesystem, time, custom)"]
    end
    db[("SQLite | PostgreSQL")]
    prom["Prometheus"]
    oauth["Authorization Servers<br/>(OAuth 2.0 dos upstreams)"]

    claude & cursor & agent -- "MCP / Streamable HTTP<br/>/mcp, /v0/groups/{g}/mcp" --> trunks
    dev -- "CLI + Dashboard web" --> trunks
    admin -- "CLI (REST /api/v0)" --> trunks
    trunks -- "MCP client" --> http
    trunks -- "spawn + stdio" --> stdio
    trunks --- db
    prom -- "scrape /metrics" --> trunks
    trunks -- "authorization code + PKCE<br/>DCR, refresh" --> oauth
```

Fronteiras de confiança: tudo o que entra por `/mcp` e `/api/v0` é **não confiável** até o middleware produzir um `Principal`; tudo o que sai para upstreams carrega **só** credenciais configuradas no gateway — nunca headers do cliente.

## 3. Vista de contentores (C4 nível 2)

```mermaid
flowchart TB
    subgraph proc["Processo `trunks start` (tokio)"]
        direction TB
        http["HTTP server (axum)"]
        rest["REST API<br/>/api/v0, /api/dashboard"]
        mcp["MCP endpoints (rmcp)<br/>/mcp, /v0/groups/{g}/mcp"]
        core["Domínio<br/>registry · catalog · gateway · sessions · oauth · identity · groups"]
        dbl["Camada de dados<br/>(SQL explícito, migrações)"]
        up["Upstream connections<br/>(rmcp client: child-process, streamable-http)"]
        spa["Web app embebida (line://ui)<br/>(rust-embed, só dev em v1.0)"]
        met["/metrics (prometheus-client)"]
        http --> rest & mcp & spa & met
        rest --> core
        mcp --> core
        core --> dbl
        core --> up
    end
    cli["CLI `trunks <cmd>`<br/>(mesmo binário, processo separado)"] -- "REST" --> http
    browser["Browser"] -- "GET / + /api/dashboard" --> http
    db[("SQLite ficheiro<br/>ou PostgreSQL")]
    dbl --> db
    up --> ups["Upstreams"]
```

| Contentor | Tecnologia | Responsabilidade | Estado |
|---|---|---|---|
| HTTP server | axum 0.8 + tower | routing, middleware (auth, init, limites, trace) | sem estado |
| REST API | handlers axum | binding JSON, mapeamento erros→HTTP, shapes compatíveis | sem estado |
| MCP endpoints | `rmcp::StreamableHttpService<GatewayHandler>` | protocolo MCP downstream | sessões downstream (legacy mode) em memória |
| Domínio | `trunks-core` | regras de negócio, autorização, catálogo | `Catalog` (ArcSwap), `SessionManager` (DashMap) |
| Camada de dados | `trunks-db` | SQL, migrações, dois dialectos | — |
| Upstream connections | `rmcp` client | ligações stdio/http, OAuth client | processos filhos, ligações HTTP |
| Web app | Lit + `line://ui` | UI dev-only em v1.0; consola autenticada a partir de v2.0 | browser |
| CLI | clap + `trunks-client` | gestão via REST | `~/.trunks.conf` |

## 4. Vista de componentes (C4 nível 3) — crates e módulos

```mermaid
flowchart TB
    subgraph types["trunks-types"]
        names["names<br/>ServerName · CanonicalName · ResourceUri · GroupName"]
        api_t["api<br/>tipos REST (serde)"]
        enums["enums<br/>Transport · SessionMode · Role · ServerMode"]
        err_t["ApiError"]
    end
    subgraph core["trunks-core"]
        registry["registry<br/>register/deregister/enable"]
        catalog["catalog<br/>CatalogSnapshot (ArcSwap)"]
        gateway["gateway<br/>GatewayHandler{Scope}"]
        sessions["sessions<br/>SessionManager"]
        upstream["upstream<br/>UpstreamConn::connect"]
        oauthm["oauth<br/>bootstrap · complete · CredentialStore"]
        identity["identity<br/>users · mcp clients · tokens · Principal"]
        groups["groups<br/>resolve · diff"]
        mode["server_mode"]
        metrics["metrics"]
        ports["ports (traits)<br/>RegistryStore · IdentityStore · OAuthStore · GroupStore · ConfigStore"]
    end
    subgraph dbc["trunks-db"]
        repos["repos (impl dos ports)"]
        migr["migrations"]
        conn["connect / Dialect"]
    end
    subgraph apic["trunks-api"]
        router["router"]
        mw["middleware"]
        handlers["handlers REST"]
        dash["dashboard API"]
        mount["mcp mount<br/>(cache de services por scope)"]
    end
    client["trunks-client (SDK)"]
    cli["trunks<br/>commands · start · config · envsubst"]
    dashui["trunks-dashboard<br/>(rust-embed)"]

    core --> types
    dbc --> core
    apic --> core
    apic --> dashui
    client --> types
    cli --> apic & client & dbc

    registry --> catalog & upstream & ports
    gateway --> catalog & sessions & identity & metrics
    sessions --> upstream
    oauthm --> upstream & ports
    groups --> catalog & ports
    identity --> ports
    repos -. implementa .-> ports
```

Regras (verificadas por `cargo-deny`/testes de arquitectura com `cargo-modules` ou simples `grep` em CI):

- `trunks-core` **não** importa axum, nem a lib de DB, nem `clap`. Importa `rmcp` apenas em `gateway` e `upstream`.
- `trunks-types` não importa nada além de `serde`/`serde_json`/`snafu`/`base64`.
- `trunks-client` não importa `trunks-core` nem `trunks-db` (é um SDK público).
- Só `trunks` conhece todos os crates — é a *composition root*.

## 5. Vistas de runtime

### 5.1 Arranque

```mermaid
sequenceDiagram
    autonumber
    participant M as trunks start
    participant C as Config
    participant D as trunks-db
    participant K as Catalog
    participant S as SessionManager
    participant H as axum

    M->>C: dotenv + flags + TRUNKS_* → Settings (mode, db, ports, timeouts)
    M->>D: connect(Settings.db)
    D->>D: run migrations (baseline IF NOT EXISTS, aditivas)
    M->>D: read server_configs
    alt não inicializado
        alt mode = development
            M->>D: init(development)
        else enterprise
            M-->>M: log "run `trunks init-server`"
        end
    else inicializado em modo diferente
        M-->>M: erro e exit 1
    end
    M->>D: load servers/tools/prompts/resources/groups (enabled)
    D-->>M: rows
    M->>M: validar: transport=sse enabled? → erro (ou skip c/ TRUNKS_IGNORE_SSE_SERVERS)
    M->>K: build CatalogSnapshot (parse schemas — erro → exit)
    M->>S: new(idle_timeout, init_timeout)
    M->>H: build router (middleware, REST, MCP mounts, dashboard, /metrics)
    H-->>M: listening on host:port
    Note over M: SIGINT/SIGTERM → cancel → S.shutdown() → H.graceful(10s)
```

### 5.2 Registo de um upstream (sem OAuth)

```mermaid
sequenceDiagram
    autonumber
    actor A as Admin (CLI)
    participant API as REST /api/v0/servers
    participant R as registry
    participant U as upstream
    participant UP as Upstream MCP
    participant DB as store
    participant K as Catalog

    A->>API: POST {name, transport, url|command, …} ?force
    API->>API: user_auth → require_admin
    API->>R: register(input, force)
    R->>R: validate ServerName, URL
    opt force && exists
        R->>R: deregister(name)
    end
    R->>U: connect(config, timeout=TRUNKS_UPSTREAM_INIT_TIMEOUT_SEC)
    U->>UP: initialize
    UP-->>U: capabilities
    R->>UP: tools/list (obrigatório)
    opt capabilities.prompts
        R->>UP: prompts/list (best-effort)
    end
    opt capabilities.resources
        R->>UP: resources/list (best-effort)
    end
    R->>U: close
    R->>DB: TX — insert server, tools, prompts, resources — COMMIT
    R->>K: rebuild snapshot
    R-->>API: ServerSummary
    API-->>A: 201 {server}
```

Nada é persistido se o `initialize` falhar. Se o upstream responder 401 com `WWW-Authenticate`, segue §5.4.

### 5.3 Chamada de tool via `/mcp` em modo enterprise

```mermaid
sequenceDiagram
    autonumber
    participant C as Cliente MCP
    participant MW as middleware<br/>require_initialized · mcp_client_auth
    participant SVC as rmcp StreamableHttpService
    participant G as GatewayHandler{Global}
    participant K as Catalog
    participant S as SessionManager
    participant UP as Upstream
    participant M as metrics

    C->>MW: POST /mcp  Authorization: Bearer <client-token><br/>tools/call {name:"github__search", args}
    MW->>MW: lookup mcp_clients by token → Principal::McpClient(allow_list)
    MW->>SVC: request + Principal em extensions
    SVC->>G: call_tool(params, ctx{Principal})
    G->>G: CanonicalName::parse → (github, search)
    G->>K: tools.get("github__search") → ToolEntry{server}
    G->>G: principal.may_access("github")? senão → erro "not authorized"
    G->>S: acquire("github")
    alt stateless
        S->>UP: connect + initialize (credenciais configuradas)
    else stateful
        S->>S: lock slot("github") → reuse ou connect
    end
    G->>UP: tools/call {name:"search", args}  — sem headers downstream
    UP-->>G: result
    G->>S: release (close se stateless — mark unhealthy se erro de transporte)
    G->>M: record(github, search, outcome, elapsed)
    G-->>SVC: CallToolResult
    SVC-->>C: 200 (JSON ou SSE stream conforme negociação)
```

`list_tools` segue o mesmo caminho até ao passo 6 e filtra o catálogo pela allow-list; sem `Principal` em enterprise devolve lista vazia.

### 5.4 OAuth upstream — bootstrap e completion

```mermaid
sequenceDiagram
    autonumber
    actor A as Admin (CLI register)
    participant API as REST
    participant O as oauth
    participant UP as Upstream MCP
    participant AS as Authorization Server
    participant DB as store
    participant B as Browser

    A->>API: POST /servers {url, oauth_redirect_uri?}
    API->>O: register → connect falha com AuthorizationRequired
    alt sem oauth_redirect_uri
        API-->>A: 400 {error, code:"upstream_oauth_required"}
        A->>A: abre listener 127.0.0.1:0/oauth/callback
        A->>API: POST /servers {…, oauth_redirect_uri}
    end
    O->>AS: discovery (.well-known)
    opt client_id vazio
        O->>AS: Dynamic Client Registration
        AS-->>O: client_id (+secret)
    end
    O->>O: state, code_verifier/challenge (S256), session_id
    O->>DB: TX — delete pending(server), insert pending{…, expires=now+10m}
    O-->>API: AuthorizationPending{session_id, url, expires_at}
    API-->>A: 202 {authorization_required}
    A->>B: abre authorization_url
    B->>AS: login + consent
    AS->>A: redirect callback?code&state
    A->>API: POST /upstream_oauth/sessions/{id}/complete {code, state}
    API->>O: complete
    O->>DB: load pending — check expiry, state
    O->>DB: upsert upstream_oauth_tokens (metadata)
    O->>AS: token exchange (code + verifier)
    AS-->>O: access + refresh tokens
    O->>DB: save tokens
    opt pending.force && exists
        O->>O: deregister(server)
    end
    O->>UP: connect com AuthClient(CredentialStore) → initialize → discover
    O->>DB: TX — insert server + entidades, delete pending
    API-->>A: 201 {server}
```

Tokens são **gateway-scoped**: uma linha em `upstream_oauth_tokens` por servidor, usada por todas as chamadas, com refresh automático pelo `AuthClient`.

### 5.5 Ciclo de vida de uma sessão stateful

```mermaid
stateDiagram-v2
    [*] --> Empty
    Empty --> Connecting: acquire() [slot lock]
    Connecting --> Ready: initialize ok
    Connecting --> Empty: initialize falha (erro ao chamador)
    Ready --> InUse: acquire()
    InUse --> Ready: release() ok · last_used = now
    InUse --> Unhealthy: release() c/ erro de transporte
    Unhealthy --> Empty: drop conn (próximo acquire reconecta)
    Ready --> Empty: idle sweeper (now − last_used > TRUNKS_SESSION_IDLE_TIMEOUT_SEC)
    Ready --> Empty: deregister(server) · shutdown
    InUse --> Empty: shutdown (cancel token)
```

Em modo stateless não há slot: `acquire` = `Connecting→Ready→InUse`, `release` = drop.

### 5.6 Pipeline de um pedido HTTP

```mermaid
flowchart LR
    req([request]) --> trace["trace<br/>(access log)"] --> limit["body limit"] --> init["require_initialized<br/>403 se não"]
    init --> route{rota}
    route -- "/api/v0/**" --> ua["user_auth<br/>dev: DevOpen · ent: bearer→User"] --> adm{"rota admin?"} -- sim --> ra["require_admin<br/>403"] --> h1["handler REST"]
    adm -- não --> h1
    route -- "/mcp, /v0/groups/{g}/mcp" --> ca["mcp_client_auth<br/>dev: DevOpen · ent: bearer→McpClient<br/>(token de user → 401)"] --> svc["StreamableHttpService<br/>→ GatewayHandler"]
    route -- "/, /assets, /api/dashboard/**" --> dm["require_dev_mode<br/>404 em enterprise"] --> h2["dashboard"]
    route -- "/health /metadata /init /metrics" --> pub["sem auth"]
```

## 6. Vista de dados

```mermaid
erDiagram
    MCP_SERVERS ||--o{ TOOLS : has
    MCP_SERVERS ||--o{ PROMPTS : has
    MCP_SERVERS ||--o{ RESOURCES : has
    MCP_SERVERS ||--o| UPSTREAM_OAUTH_TOKENS : "by server_name"
    MCP_SERVERS ||--o| UPSTREAM_OAUTH_PENDING_SESSIONS : "by server_name"

    MCP_SERVERS {
        bigint id PK
        text name UK
        varchar(30) transport "stdio | streamable_http | sse(legacy)"
        bool enabled
        text description
        json config "StreamableHttpConfig | StdioConfig"
        varchar(20) session_mode "stateless | stateful"
        timestamps created_at_updated_at_deleted_at
    }
    TOOLS {
        bigint id PK
        bigint server_id FK
        text name "bare; canónico = server__name"
        bool enabled
        text description
        json input_schema
        json annotations
    }
    PROMPTS {
        bigint id PK
        bigint server_id FK
        text name
        bool enabled
        text description
        json arguments
    }
    RESOURCES {
        bigint id PK
        bigint server_id FK
        text uri UK "trunks://res/<server>/<b64>"
        text original_uri
        text name
        bool enabled
        text mime_type
        json annotations
        json meta
    }
    TOOL_GROUPS {
        bigint id PK
        text name UK
        text description
        json included_tools
        json included_servers
        json excluded_tools
    }
    USERS {
        bigint id PK
        text username UK
        text role "admin | user"
        text access_token UK "plaintext v1; hash em D8"
    }
    MCP_CLIENTS {
        bigint id PK
        text name UK
        text description
        text access_token UK
        json allow_list "[names] | [*]"
    }
    SERVER_CONFIGS {
        bigint id PK
        varchar(12) mode "development | enterprise"
        bool initialized
    }
    UPSTREAM_OAUTH_PENDING_SESSIONS {
        bigint id PK
        text session_id UK "64 hex"
        text server_name
        json server_input
        bool force
        text state
        text code_verifier
        timestamp expires_at
    }
    UPSTREAM_OAUTH_TOKENS {
        bigint id PK
        text server_name UK
        text client_id
        text client_secret
        text access_token
        text refresh_token
        timestamp expires_at
    }
```

Notas:
- `TOOL_GROUPS` referencia tools/servidores **por nome** em JSON, não por FK — permite grupos "degradados" que sobrevivem a deregistos.
- `deleted_at` existe em todas as tabelas (herança do schema legacy). Trunks faz hard-delete e filtra `deleted_at IS NULL` nas leituras para ignorar lixo de versões antigas.
- Estado em memória derivado da DB: `CatalogSnapshot` (tudo o que é `enabled`) e `GroupView` (effective tools pré-resolvidas). Reconstruído por inteiro após cada mutação.

```mermaid
flowchart LR
    DB[("DB")] -- "load (arranque, após mutação)" --> Snap["CatalogSnapshot<br/>servers · tools · prompts · resources · groups"]
    Snap -- "ArcSwap::store" --> Cat["Catalog"]
    Cat -- "load() lock-free" --> G1["GatewayHandler Global"]
    Cat -- "load()" --> G2["GatewayHandler Group(a)"]
    Cat -- "load()" --> G3["GatewayHandler Group(b)"]
    Cat -- "load()" --> REST["handlers REST (leituras)"]
    Mut["registry · groups<br/>(mutações)"] -- "write" --> DB
    Mut -- "rebuild" --> Snap
```

## 7. Vistas de deployment

### 7.1 Local (developer, modo development)

```mermaid
flowchart TB
    subgraph host["Máquina do developer"]
        subgraph compose["docker compose"]
            t["trunks:latest-stdio<br/>:8080 · TRUNKS_MODE=development<br/>node20 + uv para upstreams stdio"]
            pg[("postgres:17")]
            t --> pg
        end
        cli["trunks CLI (installer.sh)"] --> t
        ide["Claude Code · Cursor"] -- "http://localhost:8080/mcp" --> t
        br["Browser → dashboard"] --> t
        fs["./ montado em /host:ro"] -.-> t
    end
    t -- "https" --> ext["upstreams remotos"]
```

Alternativa sem Docker: `trunks start` com SQLite em `./trunks.db`.

### 7.2 Partilhado (equipa, modo enterprise)

```mermaid
flowchart TB
    users["Developers<br/>(Cursor, Claude, agentes)"] -- "https + Bearer <mcp-client-token>" --> rp["Reverse proxy / TLS<br/>(nginx, Caddy, ingress)"]
    admins["Admins (CLI)"] -- "https + Bearer <user-token>" --> rp
    rp -- "http :8080<br/>X-Forwarded-Proto" --> t1["trunks (réplica 1)"]
    rp --> t2["trunks (réplica N)"]
    t1 & t2 --> pg[("PostgreSQL<br/>(managed)")]
    prom["Prometheus"] -- "/metrics" --> t1 & t2
    t1 & t2 -- "stdio" --> local["upstreams stdio<br/>(no mesmo container)"]
    t1 & t2 -- "https" --> remote["upstreams remotos"]
```

Limites conhecidos para N > 1 réplicas (a resolver pós-v1):
- Sessões **stateful** são por processo; com várias réplicas sem sticky sessions cada réplica tem a sua.
- O `Catalog` é reconstruído pela réplica que fez a mutação; as outras só o vêem no próximo arranque. Mitigação simples na v1.x: `LISTEN/NOTIFY` em Postgres ou um TTL de refresh configurável. Até lá, enterprise com 1 réplica, como o original.
- Sessões downstream em `legacy_session_mode` do `rmcp` são em memória; `StreamableHttpServerConfig.session_store` permite externalizar quando for preciso.

## 8. Conceitos transversais

### 8.1 Erros (`snafu`)

```mermaid
flowchart LR
    db["trunks-db::Error<br/>Sql{source} · NotFound · Dialect"] --> core["trunks-core::Error<br/>NotFound{what} · InvalidInput{reason}<br/>Unauthorized · Forbidden<br/>UpstreamOAuthRequired{…}<br/>Upstream{source} · Store{source} · Internal"]
    rmcp["rmcp::ServiceError"] --> core
    core --> api["trunks-api: impl IntoResponse<br/>NotFound→404 · InvalidInput→400 (+code)<br/>Unauthorized→401 · Forbidden→403 · resto→500"]
    core --> cli["trunks: snafu::Report<br/>mensagem humana, exit 1"]
```

- Cada crate tem o seu `enum Error` com `#[derive(Snafu)]`; conversões explícitas por `context()`, nunca `From` implícito entre camadas — preserva a classificação (`NotFound` vindo da DB não vira 500 por acidente).
- Mensagens que o CLI/testes dependem são **constantes** em `trunks-types` (ex.: `"server is not initialized"`), não strings inline.
- `Whatever` só no `main` do binário.

### 8.2 Configuração
Precedência fixa: flag explícita > env var > ficheiro `.env` (via `dotenvy`, só preenche env) > default. Resolvida uma vez em `Settings` (struct imutável) passada por `Arc`. Tabela completa em `02-especificacao.md` §9.

### 8.3 Observabilidade
- `tracing` com spans por pedido (`http.request`), por chamada upstream (`upstream.call{server,tool}`), por sessão (`upstream.session{server,mode}`). Campos nunca incluem tokens/headers/env.
- Métricas em `prometheus-client`; registry único injectado; `Noop` quando desligado.
- `/health` é liveness; **readiness** = `/health` + catálogo construído (novo campo `"catalog":"ready"` — aditivo, compatível).

### 8.4 Segurança — fronteiras

```mermaid
flowchart LR
    subgraph untrusted["Não confiável"]
        dc["cliente downstream<br/>headers, body JSON-RPC"]
        rest["pedido REST"]
    end
    subgraph gw["Trunks"]
        auth["middleware → Principal"]
        hand["GatewayHandler<br/>só vê Principal + params"]
        cfg["credenciais configuradas<br/>SecretString"]
        conn["UpstreamConn<br/>construída SEM acesso ao pedido"]
    end
    up["upstream"]
    dc --> auth --> hand --> conn --> up
    cfg --> conn
    rest --> auth
```

Propriedade central: `UpstreamConn::connect(config)` não recebe o pedido downstream — o strip de headers é estrutural, não um `request.Header = nil` que alguém pode esquecer.

## 9. Decisões arquitecturais (ADR resumido)

| ADR | Decisão | Alternativa rejeitada | Motivo |
|---|---|---|---|
| A1 | Um `GatewayHandler` parametrizado por `Scope` sobre um `Catalog` partilhado | N servidores MCP (um por grupo) com registo próprio | Elimina callbacks de sincronização e duplicação de estado; grupos passam a ser vistas |
| A2 | `Catalog` em `ArcSwap`, rebuild total por mutação | Cache por entidade com invalidação fina; ou DB por pedido | Mutações são raras e o conjunto é pequeno; rebuild total é trivialmente correcto |
| A3 | `Principal` tipado em `Parts.extensions` → `RequestContext` (validado no Spike S1) | Contexto dinâmico; um service por principal | Fail-closed por tipo; sem panics; testável sem HTTP |
| A4 | Lock por servidor no `SessionManager` | Lock global | Um upstream lento não bloqueia os outros |
| A5 | Ports (traits) no domínio, repos em `trunks-db` | Domínio a chamar SQL directamente | Testes de domínio com stores em memória; D11 pode mudar a lib sem tocar no domínio |
| A6 | SQL explícito, sem ORM (D11) | sea-orm / diesel | Decisão do produto; controlo do SQL em dois dialectos; schema é pequeno (10 tabelas) |
| A7 | `snafu` com um `Error` por crate (D10) | `thiserror` + `anyhow` | Context selectors obrigam a classificar erros na fronteira de cada camada |
| A8 | Sem SSE (D4) | Implementar SSE à mão | Deprecado no spec; `rmcp` não o tem; custo sem retorno |
| A9 | Web app em Lit + `line://ui`, embebida no binário (D2) | React; Rust/WASM | Design system próprio; a app cresce para consola autenticada (OIDC, orgs) |
| A10 | `cargo-dist` para release (D9) | pipeline à mão com Actions | Padrão do ecossistema; instaladores e checksums incluídos |
| A11 | `prometheus-client` directo, sem OTel SDK | OTel SDK + exporter Prometheus | Sem OTLP em v1, OTel só adiciona peso; reavaliar quando OTLP entrar (v3) |
| A12 | Rust 2024, `clippy::unwrap_used` em `core`/`api` | — | Sem panics em request path |

## 10. Mapa para a especificação

| Secção aqui | Detalhe em `02-especificacao.md` |
|---|---|
| §3 contentores | §2 workspace, §3 stack |
| §4 componentes | §4 domínio, §5 API, §6 SDK, §7 CLI |
| §5.1 arranque | §8 |
| §5.2–5.4 fluxos | §4.4 registry, §4.5 gateway, §4.8 OAuth |
| §5.5 sessões | §4.6 |
| §6 dados | §4.2 |
| §8 transversais | §9 config, §10 observabilidade, §11 segurança |
