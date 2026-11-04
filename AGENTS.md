# Trunks — agent guide

Trunks is a self-hosted MCP gateway and registry in Rust. Read `docs/` before non-trivial
changes: `01-produto.md` (decisions D1–D12), `02-especificacao.md` (contracts and modules),
`03-plano.md` (phases), `04-arquitectura.md`, `05-roadmap.md`.
Explicit task instructions take precedence over this file.

## Principles

- **Contract first.** REST paths, JSON shapes, error messages, CLI flags and output strings, and
  the database schema are stable within a major version and specified in `docs/02-especificacao.md`.
  Change the spec in the same PR as the code.
- **Own names only.** Env vars are `TRUNKS_*`, the resource URI scheme is `trunks://`, config lives
  in `~/.config/trunks/`. No aliases for other products' names.
- **Fail closed.** Authorization is carried by a typed `Principal`; a missing principal in
  enterprise mode yields empty lists and denied calls, never a panic.
- **Secrets never leak.** Tokens, bearer headers, stdio `env` values and OAuth secrets live in
  `secrecy::SecretString`; they are never logged and only serialized where the contract requires.
- **No headers forwarded upstream.** Upstream connections are built from configured credentials
  only; downstream request headers never reach an upstream.
- **Smallest change that solves the problem.** Reuse existing modules before adding abstractions.

## Code map

| Path | Owns |
|---|---|
| `crates/trunks-types` | Canonical names (`ServerName`, `CanonicalName`, `ResourceUri`, `GroupName`), enums, REST types, error envelope and message constants |
| `crates/trunks-core` | Domain: `error`, `ports` (store traits), registry, catalog, gateway (`rmcp` `ServerHandler`), sessions, upstream connections, OAuth, identity, groups |
| `crates/trunks-db` | SQL over SQLite/PostgreSQL (no ORM), migrations, implementations of the ports |
| `crates/trunks-api` | axum router, middleware, REST handlers, MCP mounts, dashboard API, error → HTTP mapping |
| `crates/trunks-client` | HTTP SDK used by the CLI (public, wire-compatible) |
| `crates/trunks-dashboard` | `rust-embed` of `web/app` build output |
| `crates/trunks` | The binary: `start` (server) and management subcommands |
| `web/app` | Web application built with `line://ui` |
| `xtask` | `cargo xtask build-dashboard`, `cargo xtask ci` |
| `tests/` | Conformance fixtures (legacy-format DBs) and black-box tests |

Dependency rules: `types ← core ← {db, api}`; `client` depends only on `types`; only the binary
crate knows every crate. `trunks-core` must not import axum, the DB driver or clap; `rmcp` is
imported only from its `gateway` and `upstream` modules.

## Conventions

- Rust edition 2024, stable toolchain. `cargo xtask ci` must pass: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Errors: `snafu`. One `Error` enum per crate, context selectors, `ensure!`; `Whatever` only in
  `main`. No `unwrap`/`expect`/`panic` outside tests (lints enforce it; tests opt out via the
  crate-level `cfg_attr`).
- Logging: `tracing`. Metrics: `prometheus-client`.
- Strings that clients depend on live in `trunks_types::error::messages`, not inline.
- Keep functions `async` and `&self`; share state through `Arc` + `ArcSwap`/`DashMap`, no global
  mutable state.
- Tests: table-driven cases for parsing/validation; add a conformance test whenever a route or
  CLI command is added.

## Commands

```bash
cargo build                      # all crates
cargo run -- start               # server on :8080
cargo run -- version
cargo test --workspace
cargo xtask ci                   # what CI runs
cargo xtask build-dashboard      # needs Node 20; builds web/app into crates/trunks-dashboard/dist
dist plan                        # release artefacts (cargo-dist)
```

## Before handing off

- `cargo xtask ci` green.
- Docs updated when a contract, a decision or a phase changes (`docs/`).
- Report what was run and what was skipped; never imply the full suite passed if it did not.
