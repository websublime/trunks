# Configuration reference

## Server (`trunks start`)

Each setting can be given as a command-line flag or as an environment variable. Order of
precedence: flag, environment variable, `.env` file (in the working directory or one of its
parents), default.

### Basics

| Flag | Environment variable | Meaning | Default |
|---|---|---|---|
| `--mode` | `TRUNKS_MODE` | `development` or `enterprise` (case-insensitive). Fixed once the database is initialized | `development` |
| `--host` | `TRUNKS_BIND_HOST` | address to listen on; empty means every interface | every interface |
| `--port` | `TRUNKS_PORT` | port to listen on | `8080` |
| `--database-url` | `TRUNKS_DATABASE_URL` | `sqlite://<path>` or `postgres://user:pass@host:port/db` | `sqlite://trunks.db` |
| `--database-url-file` | `TRUNKS_DATABASE_URL_FILE` | file containing the database URL (for a secret mounted as a file). Takes precedence over the URL; surrounding whitespace is ignored; an unreadable or empty file is a start-up error | unset |
| `--public-url` | `TRUNKS_PUBLIC_URL` | the server's address as seen by browsers, e.g. `https://trunks.example.com`. Decides the session cookie's `Secure` flag and the accepted origin. Required for single sign-on | unset |
| `--allowed-hosts` | `TRUNKS_ALLOWED_HOSTS` | comma-separated `Host` header values accepted on the MCP endpoints; `*` disables the check | loopback names only |

### Limits and sessions

| Flag | Environment variable | Meaning | Default |
|---|---|---|---|
| `--request-timeout-sec` | `TRUNKS_REQUEST_TIMEOUT_SEC` | upper bound, in seconds, for one REST request and for one call to an upstream; `0` means no limit | `300` |
| `--upstream-init-timeout-sec` | `TRUNKS_UPSTREAM_INIT_TIMEOUT_SEC` | seconds allowed for the MCP handshake with an upstream (at least 1) | `30` |
| `--session-idle-timeout-sec` | `TRUNKS_SESSION_IDLE_TIMEOUT_SEC` | close `stateful` upstream sessions idle for this many seconds; `0` means never | `0` |
| `--session-ttl-hours` | `TRUNKS_SESSION_TTL_HOURS` | lifetime of a browser session, in hours (at least 1) | `12` |

Fixed limits: request bodies up to 4 MiB; 30 s to receive a request's header; idle
connections are closed after 30 s; 10 s for requests in flight at shutdown.

### Observability

| Flag | Environment variable | Meaning | Default |
|---|---|---|---|
| `--metrics` | `TRUNKS_METRICS_ENABLED` | `true` or `false`: serve Prometheus metrics at `/metrics` | on in enterprise mode, off in development |
| | `TRUNKS_LOG` | log filter in `tracing` syntax, e.g. `info`, `debug`, `info,upstream.stderr=off`. `RUST_LOG` is used when it is unset | `info` |

### Single sign-on (OIDC)

Enterprise mode only; setting an issuer in development mode is a start-up error. See
[Enterprise mode](../enterprise.md#single-sign-on-oidc).

| Flag | Environment variable | Meaning | Default |
|---|---|---|---|
| `--oidc-issuer` | `TRUNKS_OIDC_ISSUER` | issuer URL; setting it enables sign-in through the provider. Requires the client id and the public URL | unset |
| `--oidc-client-id` | `TRUNKS_OIDC_CLIENT_ID` | client id registered at the provider | unset |
| `--oidc-client-secret` | `TRUNKS_OIDC_CLIENT_SECRET` | client secret; omit for a public client. Prefer the environment variable: flags show up in process listings | unset |
| `--oidc-scopes` | `TRUNKS_OIDC_SCOPES` | comma-separated scopes requested in addition to `openid` | `profile,email` |
| `--oidc-groups-claim` | `TRUNKS_OIDC_GROUPS_CLAIM` | claim holding the user's groups | `groups` |
| `--oidc-admin-group` | `TRUNKS_OIDC_ADMIN_GROUP` | group whose members are administrators; without it, sign-in never changes roles | unset |
| `--oidc-sync-teams` | `TRUNKS_OIDC_SYNC_TEAMS` | `true`: keep team memberships in step with the groups claim | `false` |
| `--oidc-team-prefix` | `TRUNKS_OIDC_TEAM_PREFIX` | only groups with this prefix become teams; the prefix is removed from the team name | empty |

The redirect URI to register at the provider is `<public URL>/auth/callback`.

### Migration

| Environment variable | Meaning | Default |
|---|---|---|
| `TRUNKS_IGNORE_SSE_SERVERS` | `1`: when the database holds enabled servers with the unsupported `sse` transport, skip them with a warning instead of refusing to start | unset |

## CLI (management commands)

| Flag | Environment variable | Meaning | Default |
|---|---|---|---|
| `--registry` | `TRUNKS_REGISTRY` | base URL of the Trunks server to manage | the configuration file's `registry_url`, else `http://127.0.0.1:8080` |
| | `TRUNKS_CONFIG` | path of the CLI configuration file | `~/.config/trunks/config.toml` |

The configuration file is written by `trunks init-server` and `trunks login`, readable by its
owner only:

```toml
registry_url = "https://trunks.example.com"
access_token = "..."
```

`access_token` is a user's token and is sent with every management command.

## Files read by commands

Files given with `-c` (`register`, `create group`, `update group`, `create user`,
`create mcp-client`) are JSON. `${VAR}` inside any string is replaced by the environment
variable `VAR` of the CLI process; a variable that is not set is an error. Formats:
[upstream servers](../upstreams.md#from-a-file), [tool groups](../tool-groups.md#define-a-group).
