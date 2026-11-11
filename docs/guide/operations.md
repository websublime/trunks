# Operations

For whoever installs, configures and keeps a Trunks server running. Every setting is listed in
the [configuration reference](reference/configuration.md).

## Install

### Binary

Each release publishes archives for Linux, macOS and Windows (x86_64 and arm64), with
checksums and installers:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/websublime/trunks/releases/latest/download/trunks-installer.sh | sh
trunks start            # development mode, SQLite in ./trunks.db, port 8080
```

The same binary is the server (`trunks start`) and the management CLI (every other command).

### Container images

| Image | Contents | For |
|---|---|---|
| `ghcr.io/websublime/trunks:<version>` | the binary only (distroless) | upstreams reached over HTTP |
| `ghcr.io/websublime/trunks:<version>-stdio` | plus Node.js 24 (`npx`) and uv with Python 3.13 (`uvx`) | `stdio` upstreams distributed as npm or PyPI packages |

`latest` and `latest-stdio` follow the newest final release. Both are built for `linux/amd64`
and `linux/arm64`.

In both images the process **does not run as root** (uid 65532 in the distroless image, 10001
in the `-stdio` image), the data lives in `/data` (a volume) and `TRUNKS_DATABASE_URL` defaults
to `sqlite:///data/trunks.db`. The distroless image has no shell and no `npx`: registering a
`stdio` upstream there fails with "failed to spawn".

```bash
docker run -d --name trunks -p 127.0.0.1:8080:8080 -v trunks-data:/data \
  -e TRUNKS_ALLOWED_HOSTS=localhost,127.0.0.1 \
  ghcr.io/websublime/trunks:latest-stdio
```

### Compose

- `docker-compose.yaml`: one person, one machine. Development mode, SQLite in a volume, the
  `-stdio` image, port published on `127.0.0.1` only.
- `docker-compose.prod.yaml`: enterprise mode on PostgreSQL. Copy `.env.example` to `.env`,
  fill it in, and put a TLS-terminating reverse proxy in front ([Network](#network)).

## Modes

| | `development` | `enterprise` |
|---|---|---|
| Authentication | none | tokens (or single sign-on) on everything |
| Who sees what | everyone sees everything | users and MCP clients see only what they were granted |
| Initialization | automatic | `trunks init-server`, once |
| `/metrics` | off by default | on by default |

**Development mode has no access control at all.** By default it listens on `127.0.0.1`
only. Give it another address (`--host`) only on a network where everyone who can reach it is
trusted; it warns at start-up when you do. Enterprise mode listens on every interface by
default. The container images listen on every interface in both modes, since there the port
mapping decides who can reach the server.

The mode is recorded in the database at initialization. Starting later with a different
`TRUNKS_MODE` is a start-up error, not a conversion.

First start in enterprise mode:

```bash
trunks --registry https://trunks.example.com init-server
```

This prints the administrator's token **once** and stores it in the CLI configuration
(`~/.config/trunks/config.toml`, or the path in `TRUNKS_CONFIG`). Until it is run, anyone who
can reach the server can run it; do it before exposing the server. Losing the token when there
is no other administrator means restoring or resetting the database. Continue with
[Enterprise mode](enterprise.md).

## Database

| | SQLite | PostgreSQL |
|---|---|---|
| URL | `sqlite://path/trunks.db` (`sqlite:///abs/trunks.db` for an absolute path) | `postgres://user:pass@host:5432/db` |
| For | one person, a small team | enterprise mode |
| Backup | `sqlite3 trunks.db ".backup copy.db"` while the server runs; or stop it and copy `trunks.db` **together with** its `-wal` and `-shm` files | `pg_dump` |

- **Migrations** are applied automatically at start-up and only add to the schema. Back up
  before upgrading.
- **One instance per database.** The catalog (servers, tools, groups) is held in memory and
  reloaded only by the instance where a change is made. Other instances on the same database
  do not see the change until they restart.
- **The PostgreSQL URL contains the password.** To keep it out of an environment variable,
  point `TRUNKS_DATABASE_URL_FILE` at a file holding the URL (a Docker or Kubernetes secret
  mounted as a file). When set, it takes precedence over `TRUNKS_DATABASE_URL`.
- **The database contains secrets in clear text**: access tokens of users and MCP clients,
  upstream credentials (bearer tokens, headers, `env` values) and OAuth tokens. Protect the
  file or the instance, and the backups, the way you protect a password vault. Of browser
  sessions only a hash is stored.

## Network

Trunks speaks plain HTTP. In production it sits behind a reverse proxy that terminates TLS.

| Setting | What it is | In production |
|---|---|---|
| `TRUNKS_BIND_HOST` / `TRUNKS_PORT` | where it listens (enterprise mode: every interface unless set) | an internal interface, or the port published to the proxy only |
| `TRUNKS_PUBLIC_URL` | the address as seen from outside | `https://trunks.example.com`; decides the cookie's `Secure` flag, the origin accepted for changes made with a browser session, and the single sign-on redirect |
| `TRUNKS_ALLOWED_HOSTS` | `Host` headers accepted on the MCP endpoints (protection against DNS rebinding); loopback names only by default | the public host name; any other `Host` gets 403 |

At the proxy:

- **Do not buffer responses** of `/mcp` and `/v0/groups/*/mcp`: they are streams
  (`text/event-stream`).
- Read timeout above `TRUNKS_REQUEST_TIMEOUT_SEC` (300 s by default), so that Trunks answers
  504 instead of the proxy cutting the connection.
- Keep-alive connections from the proxy to Trunks with an idle timeout **below 30 s**: Trunks
  closes connections that have been idle for 30 s.
- Request bodies: Trunks refuses more than 4 MiB (413).

Trunks' own limits, to size things around: 30 s for a client to send the request header;
300 s per REST request and per call to an upstream (`TRUNKS_REQUEST_TIMEOUT_SEC`, `0`
disables); 30 s for the handshake with an upstream (`TRUNKS_UPSTREAM_INIT_TIMEOUT_SEC`).

## Upstreams

- **`stdio`**: the process runs inside the Trunks machine or container, as the Trunks user,
  with Trunks' environment plus the configured `env`. Only register commands you trust:
  whoever can register servers can run programs. In enterprise mode only administrators can.
- `stateless` (the default) starts one process per call; `stateful` keeps one open and reopens
  it after a failure. For servers launched by `npx` or `uvx`, `stateful` saves the start-up
  on every call ([measurements](../07-desempenho.md), in Portuguese);
  `TRUNKS_SESSION_IDLE_TIMEOUT_SEC`
  closes the ones left idle.
- When a session ends, the process and everything it started are terminated.
- The process's stderr appears in the log with `target = upstream.stderr`, with the configured
  `env` values replaced by `[redacted]`. A secret the server obtains some other way and
  prints is logged as it is.

## Observe

- The web console at `/` shows what the gateway holds. Its Diagnostics page (administrators
  only in enterprise mode) shows the database backend, the listen address, the allowed hosts,
  the time limits, the upstreams that failed their last connection and the authorizations
  still pending. It is embedded in the release binaries and the container images.
- `GET /health` answers 200 while the process is serving. It does not check upstreams.
- `GET /metrics` (Prometheus). It has no authentication: do not publish it.
  - `trunks_tool_calls_total{mcp_server_name,tool_name,outcome}` and
    `trunks_tool_call_latency_seconds`: calls and latency per tool;
  - `trunks_upstream_sessions_active{mcp_server_name,mode}`: upstream sessions open right now;
  - `trunks_upstream_connect_errors_total{mcp_server_name}`: failed connections to upstreams;
    when it climbs, an upstream is down;
  - `trunks_build_info{version}`: the running version.
- Logs go to stdout: text in development mode; in enterprise mode JSON, one object per line
  and nothing else, ready for a log collector. `TRUNKS_LOG` is a `tracing` filter (`info` by
  default):
  - one line per response, with method, path (without the query string), status, latency and
    the kind of caller;
  - `TRUNKS_LOG=info,upstream.stderr=off` silences the upstreams' stderr;
  - `debug` and `trace` record tool arguments and results and the SQL text. Do not use them
    in production without need; start-up warns when they are on.
- The state of each upstream (last connection error, open sessions) is in
  `GET /api/dashboard/diagnostics`.

## Stop, upgrade, roll back

- `SIGTERM` (or Ctrl-C): Trunks stops accepting connections, gives requests in flight 10 s,
  closes the upstream sessions and exits with 0. MCP streams still open after the 10 s are
  cut; clients reconnect.
- **Upgrade**: back up, replace the binary or the image, start. Migrations run at start-up.
- **Roll back**: also restore the backup taken before the upgrade. An older version on a
  newer schema is not supported.
- Tokens do not change in an upgrade: MCP clients and CLIs stay configured.
- `trunks version` shows the CLI's version and the server's. A version with `+<commit>` is
  not a release build.

## Common problems

| Symptom | Likely cause |
|---|---|
| `403 server is not initialized` | enterprise mode without `init-server` |
| start-up fails mentioning the mode | `TRUNKS_MODE` differs from the mode the database was initialized with |
| an MCP client gets 403 as soon as it connects | the `Host` it uses is not in `TRUNKS_ALLOWED_HOSTS` |
| `401 invalid MCP client token` | a user's token used on an MCP endpoint (or the reverse): they are different identities |
| a user or client "sees nothing" | a grant is missing (`trunks grant …`) or the server is not in the client's allow list |
| `tool '…' not found` for a tool that exists | it is disabled, outside the group, or out of the caller's reach; the answer is the same on purpose |
| `failed to spawn 'npx'` | distroless image; use the `-stdio` one |
| `504` with `did not answer within …s` | the upstream did not answer within `TRUNKS_REQUEST_TIMEOUT_SEC` |
| MCP answers cut short or arriving all at the end | the proxy is buffering or has a short timeout |
| occasional "connection closed" in a client that keeps connections open | the client reused a connection idle for more than 30 s; lower the idle timeout of the client or the proxy |
