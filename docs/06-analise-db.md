# Trunks — Análise D11: camada de acesso a dados (sem ORM)

> Versão 1.0 (2026-11-04). Fecha a decisão D11 de `01-produto.md`. Protótipos executados contra `tests/fixtures/legacy-enterprise.sqlite` e contra a mesma fixture carregada num PostgreSQL 16 real.

---

## 1. Requisitos

| # | Requisito | Origem |
|---|---|---|
| R1 | Dois dialectos (SQLite para dev, PostgreSQL para enterprise) com **um** código de repositório | produto §4.3, §5.6 |
| R2 | Sem ORM: SQL explícito, visível, revisável | D11 |
| R3 | JSON como `serde_json::Value` tipado em ambos (configs, schemas, allow-lists); `NULL` ≠ `{}` | spec §4.2 |
| R4 | `bool`, timestamps e `Option` a fazer round-trip sem conversões manuais | spec §4.2 |
| R5 | Transacções com `RETURNING id` | spec §4.4 (registo transaccional) |
| R6 | Migrações versionadas, idempotentes, por dialecto (DDL difere: `bigserial` vs `INTEGER AUTOINCREMENT`, `jsonb` vs texto) | spec §8, D3 |
| R7 | SQLite *bundled* (sem libsqlite3 do sistema), Postgres via rustls; sem OpenSSL | spec §13, `deny.toml` |
| R8 | Async nativo em tokio, pool de ligações | spec §3 |
| R9 | Custo de build e de dependências razoável | plano §5 |

## 2. Candidatos e protótipo

Cada candidato implementou o mesmo contrato (`list_servers`, `tools_for`, `server_config`, `create_group` em transacção com `RETURNING` + rollback, `UPDATE … replace()` de URIs) e passou as mesmas asserções nas duas DBs (4 servidores, `session_mode` NULL → `None`, `annotations` NULL vs `{}`, header `Authorization` dentro do JSON, bool `false` no servidor `sse`, id devolvido pelo `RETURNING`, 2 URIs a normalizar).

| | A1 `sqlx` + driver `Any` | A2 `sqlx` + pools nativos (`SqlitePool`/`PgPool`) | B `sea-query` + `sea-query-binder` + `sqlx` | C `rusqlite` + `tokio-postgres` |
|---|---|---|---|---|
| Ideia | um só tipo de pool, SQL escrito uma vez | enum `Db { Sqlite, Pg }`; SQL escrito uma vez, executado no pool certo | AST de query construída uma vez; SQL gerado por dialecto | duas implementações completas |
| Passou nas duas DBs | sim, **depois de 3 workarounds** | sim | sim | sim |
| LOC do protótipo | 67 (+ workarounds) | **74** | 102 | 127 |
| Ramos específicos de dialecto | 6 (casts `::jsonb`/`::timestamptz`, bool como `i64` em SQLite) | 6 → **0** com placeholders `$N` (ver §3) | 4 (só o `match` do pool) | 6 (tudo duplicado) |
| JSON tipado | **não** (texto + `from_str`; `::text` em PG) | `sqlx::types::Json<Value>` | `Json<Value>` | PG sim; SQLite texto |
| `bool`/chrono | **não** no `Any` (bool chega como `BIGINT` em SQLite; chrono não implementa `Encode<Any>`) | sim | sim | sim (rusqlite bool = `i64`) |
| Transacções + `RETURNING` | sim | sim | sim | rusqlite sim; `tokio-postgres` precisa de `&mut Client` → pool (`deadpool-postgres`) para ser utilizável |
| Async | sim | sim | sim | rusqlite é **sync** (`spawn_blocking` + `Mutex`) |
| Migrações | `sqlx::migrate!` | `sqlx::migrate!` por dialecto | idem (+ `sea-query` para DDL opcional) | `refinery` ou à mão |
| Deps transitivas (stack mínima) | 195 | **195** | 208 | **86** |
| Build release limpo (stack mínima) | 160 s | **160 s** | 161 s | **81 s** |
| Risco | tipos "menor denominador comum" escondem bugs até ao runtime | nenhum encontrado | abstracção extra para pouco ganho: as queries de Trunks são estáticas | drift entre as duas implementações; dois modelos de erro; dois modelos de pool |

## 3. Achado decisivo: placeholders `$N` em SQLite

O único ponto onde SQLite e Postgres divergiam no DML era o estilo de placeholder (`?` vs `$1`). Verificado empiricamente com `sqlx` 0.8: **SQLite aceita `$1`, `$2`, … incluindo repetidos** (`VALUES ($1, $1, $2)` liga um parâmetro por nome, como o Postgres).

```rust
sqlx::query("INSERT INTO t (a, b, c) VALUES ($1, $1, $2)").bind("x").bind(7).execute(&sqlite_pool).await?;
// → a=x b=x c=7
```

Consequência: com pools nativos, **todo o DML é uma única string por operação**, partilhada pelos dois dialectos. Só o DDL (migrações) é escrito duas vezes — e esse tem mesmo de ser, seja qual for a lib.

## 4. Decisão

**A2 — `sqlx` 0.8 com `SqlitePool` e `PgPool` nativos atrás de um enum, SQL explícito escrito uma vez com placeholders `$N`.**

Rejeitados:
- **A1 (`Any`)**: viola R3 e R4 (JSON e chrono não tipados; bool mal mapeado em SQLite). Foi preciso injectar casts no SQL por dialecto — exactamente o que se queria evitar. É um driver para "qualquer DB em runtime sem features", não para "duas DBs conhecidas com tipos ricos".
- **B (`sea-query`)**: +13 dependências e +40 % de código para gerar SQL que, com `$N`, já é idêntico nos dois dialectos. Valeria se houvesse queries dinâmicas (filtros compostos pelo utilizador); o catálogo é carregado inteiro para memória e filtrado em Rust, logo não há. Fica como opção se um dia aparecerem.
- **C (nativos)**: metade das dependências e do tempo de build, mas duplicação integral dos repositórios, `rusqlite` síncrono num servidor async, e `tokio-postgres` sem pool nem transacções ergonómicas sem mais crates. O ganho de build não compensa 10 tabelas × 2 implementações mantidas à mão.

## 5. Padrão de implementação (`trunks-db`)

```rust
// crates/trunks-db/src/lib.rs
pub enum Db { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }

/// Executa um fetch_all com o mesmo SQL/binds no pool activo.
macro_rules! fetch_all {
    ($db:expr, $sql:expr, $ty:ty, $($bind:expr),* $(,)?) => {
        match $db {
            Db::Sqlite(p) => sqlx::query_as::<_, $ty>($sql)$(.bind($bind))*.fetch_all(p).await,
            Db::Postgres(p) => sqlx::query_as::<_, $ty>($sql)$(.bind($bind))*.fetch_all(p).await,
        }
    };
}

// crates/trunks-db/src/repos/servers.rs
const LIST: &str = "SELECT id, name, transport, enabled, description, config, session_mode
                    FROM mcp_servers WHERE deleted_at IS NULL ORDER BY name";

#[derive(sqlx::FromRow)]
struct ServerRow { id: i64, name: String, transport: String, enabled: bool,
                   description: Option<String>, config: sqlx::types::Json<serde_json::Value>,
                   session_mode: Option<String> }

pub async fn list(db: &Db) -> Result<Vec<ServerRow>> { Ok(fetch_all!(db, LIST, ServerRow,)?) }
```

Regras:
1. **Uma string SQL por operação**, `const`, com `$N`. Nunca `format!` para injectar dialecto; se um dia for inevitável, a divergência vive numa função `dialect::expr_x(db)` com teste nas duas DBs.
2. Tipos de linha (`*Row`) com `#[derive(FromRow)]` em `trunks-db`; conversão para tipos de domínio em `trunks-db`, nunca expostos para fora.
3. JSON sempre `Json<serde_json::Value>` (ou `Json<T>` quando o shape é fixo); `Option<Json<_>>` para colunas nullable.
4. Transacções: `match db { Db::Sqlite(p) => p.begin().await?, … }` devolve `Transaction<'_, Sqlite>` vs `<Postgres>` — tipos diferentes; os repositórios expõem `with_tx(|tx| …)` que esconde isso, usando o mesmo macro pattern (`execute_in_tx!`).
5. Migrações: `sqlx::migrate!("./migrations/sqlite")` e `sqlx::migrate!("./migrations/postgres")`; a mesma sequência numerada nos dois directórios; DML de normalização (`normalize_legacy`) idêntico nos dois ficheiros. Tabela de versões `_sqlx_migrations`.
6. Testes de repositório correm contra `sqlite::memory:` **e** contra Postgres (via `testcontainers` em CI; `TRUNKS_TEST_PG_URL` localmente); ambos obrigatórios para fechar a Fase 1.
7. Sem `query!`/`query_as!` (macros com verificação em compile-time): exigem `DATABASE_URL` no build e uma DB por dialecto; a verificação é feita pelos testes contra as fixtures.

Features Cargo: `sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "tls-rustls", "sqlite", "postgres", "json", "chrono", "migrate", "macros"] }` — `sqlite` compila libsqlite3 *bundled* (R7).

## 6. Riscos aceites

| Risco | Mitigação |
|---|---|
| Afinidade de tipos em SQLite (`enabled numeric`, `config jsonb`) — colunas criadas pelo ORM legacy com nomes de tipo que o SQLite não conhece | `sqlx` decodifica `bool` de `INTEGER 0/1` e `Json` de texto sem problemas (verificado na fixture). As migrações novas usam tipos canónicos (`INTEGER`, `TEXT`). |
| Timestamps gravados pelo legacy como texto `2026-11-01 10:00:00.000+00:00` | `sqlx` sqlite + chrono lê esse formato (verificado). Trunks grava RFC 3339. |
| Build de 160 s a frio | cache no CI (`Swatinem/rust-cache`); localmente só paga uma vez |
| `sqlx` 0.9 pode mudar APIs | pin exacto; superfície de uso pequena (`query_as`, `Json`, `migrate`) |
