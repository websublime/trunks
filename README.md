# Trunks

**Run all your MCP servers behind one endpoint.**

Trunks is a self-hosted MCP gateway and registry written in Rust. Register your
MCP servers once; let Claude, Cursor, Copilot, Codex and your own agents connect
to a single Streamable HTTP endpoint with unified discovery, tool groups, access
control and metrics.

Trunks is a ground-up rewrite of [MCPJungle](https://github.com/mcpjungle/mcpjungle)
(Go). The v1 goal is **drop-in compatibility**: same REST API, same CLI, same
database schema — point Trunks at an existing MCPJungle database and your clients
keep working. After parity, Trunks grows beyond it: OIDC, organisations and teams,
declarative configuration. See the roadmap in `docs/`.

> Status: **pre-alpha** — Phase 0 (foundations) of `docs/04-plano.md`.

## Documentation

| Doc | What it covers |
|---|---|
| [`docs/01-produto.md`](docs/01-produto.md) | Product: what, for whom, concepts, decisions (D1–D12) |
| [`docs/02-especificacao.md`](docs/02-especificacao.md) | Technical specification of the Rust implementation |
| [`docs/03-migracao.md`](docs/03-migracao.md) | Go → Rust mapping, contracts to preserve, operator migration guide |
| [`docs/04-plano.md`](docs/04-plano.md) | Execution plan: phases, milestones, definition of done |
| [`docs/05-arquitectura.md`](docs/05-arquitectura.md) | Architecture views (C4, sequences, data, deployment) and ADRs |
| [`docs/06-roadmap.md`](docs/06-roadmap.md) | Beyond parity: `line://ui` web app, OIDC, organisations and teams, governance |

Documents are written in Portuguese (pt-PT) with English technical terms.

## Layout

```
crates/
  trunks-types/      shared contracts: canonical names, enums, REST types
  trunks-core/       domain: registry, catalog, gateway, sessions, identity, OAuth
  trunks-db/         data access (SQLite | PostgreSQL), explicit SQL, migrations
  trunks-api/        axum router, middleware, REST + MCP mounts
  trunks-client/     HTTP SDK for the management API
  trunks-dashboard/  embedded web app assets
  trunks/            the `trunks` binary: server (`start`) + management CLI
web/app/             web application (line://ui) — see web/README.md
xtask/               repo automation (`cargo xtask build-dashboard`, `cargo xtask ci`)
docs/                design documents
tests/               conformance fixtures and black-box tests
```

## Development

```bash
cargo build
cargo test --workspace
cargo xtask ci            # fmt --check, clippy -D warnings, tests
cargo run -- start        # http://127.0.0.1:8080/health
cargo run -- version
```

Requirements: Rust stable ≥ 1.85 (edition 2024). Node 20 is only needed to build the web app.

## Releases

Built and published with [`cargo-dist`](https://opensource.axo.dev/cargo-dist/):
archives for Linux/macOS/Windows (x86_64 and arm64), shell and PowerShell
installers, Homebrew tap `websublime/homebrew-trunks`, checksums. Container
images are built by a separate workflow.

## License

[MPL-2.0](LICENSE).
