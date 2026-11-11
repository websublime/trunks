# Changelog

All notable changes to Trunks are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/). Within a major version the REST API, the CLI
(flags and output), error messages and the database schema are stable.

## [Unreleased]

First public version. Everything below is new.

### Gateway

- One MCP endpoint (`/mcp`, Streamable HTTP) in front of any number of upstream MCP servers.
  Tools and prompts are exposed as `<server>__<name>`, resources as
  `trunks://res/<server>/<encoded-uri>`.
- Upstreams over **stdio** (a process Trunks starts) or **Streamable HTTP**, with a bearer
  token, extra headers, or OAuth 2.1 (authorization code + PKCE, dynamic client
  registration, token refresh).
- Per-server session mode: `stateless` (a session per call) or `stateful` (one session kept
  open and reopened when it breaks), with an optional idle timeout.
- Tool groups: a named subset of tools with its own MCP endpoint
  (`/v0/groups/<name>/mcp`).
- No header a client sends ever reaches an upstream.

### Registry and CLI

- `trunks register | deregister | enable | disable | list | get | usage | invoke | export`,
  from flags or JSON files with `${VAR}` expansion.
- Enabling and disabling at the level of a server, a tool, a prompt or a resource.
- REST API under `/api/v0` behind every CLI command; `trunks-client` is the HTTP SDK.

### Access control (enterprise mode)

- Two modes: `development` (no authentication, for one person on one machine) and
  `enterprise`. An enterprise server is initialized once with `trunks init-server`, which
  hands out the administrator's token; a server can only be initialized in the mode it was
  started in.
- Users (`admin` or `user`) with access tokens for the REST API and the CLI; MCP clients
  with their own tokens and an allow-list of servers.
- Teams and grants: a user or a team is granted the use of a server or a tool group.
  Users see nothing until something is granted to them.
- MCP clients can belong to a user or a team and never reach more than their owner;
  deleting the owner deletes its clients.
- Single sign-on with OpenID Connect (authorization code + PKCE). Users are created on
  first login; the administrator role and team memberships can follow a groups claim.
- Browser sessions for the web console: `HttpOnly` cookie, same-origin check on every
  change, never accepted on the MCP endpoints.
- What a caller may not use is neither listed nor distinguishable from what does not exist,
  in the REST API, the MCP endpoints and the dashboard views alike.

### Operations

- Development mode listens on `127.0.0.1` unless told otherwise; enterprise mode listens on
  every interface and logs JSON, one object per line.
- SQLite (default) or PostgreSQL; migrations are applied at start-up. The database URL
  can be read from a file (`TRUNKS_DATABASE_URL_FILE`) to keep it out of the environment.
- `/health`, Prometheus metrics at `/metrics` (`trunks_tool_calls_total`,
  `trunks_tool_call_latency_seconds`, `trunks_upstream_sessions_active`,
  `trunks_upstream_connect_errors_total`, `trunks_build_info`), one access-log line per
  response.
- `trunks version`, `/metadata` and `trunks_build_info` report `v<version>` for a release
  build and `v<version>+<commit>` for any other build.
- Limits: 4 MiB request bodies, a request timeout for REST calls and for each call to an
  upstream (`TRUNKS_REQUEST_TIMEOUT_SEC`, default 300 s), 30 s for a client to send its
  request head, graceful shutdown bounded to 10 s.
- A stdio upstream is run as a process tree that is killed as a whole when its session
  ends; its stderr is logged with its configured environment values redacted.
- Container images `ghcr.io/websublime/trunks` (`:<version>`, distroless) and
  `:<version>-stdio` (adds Node.js and uv for stdio upstreams), for `linux/amd64` and
  `linux/arm64`; `docker-compose.yaml` and `docker-compose.prod.yaml`.
- Binaries, checksums and shell/PowerShell installers for Linux, macOS and Windows
  (x86_64 and arm64).

### Documentation

- User guide in `docs/guide`: quickstart, concepts, upstream servers, tool groups,
  enterprise mode, connecting clients, operations, and CLI, configuration and HTTP API
  references.

### Web console

- A console embedded in the binary and served at `/`: sign-in with an access token (and
  single sign-on when configured) and an overview of what the gateway holds, limited to what
  the viewer was granted. Light and dark, usable at phone width.
- Read-only catalogue: servers and how each is reached (never a credential), tools with their
  parameters, prompts with their arguments, resources, tool groups with their endpoint and
  effective tools, and, for administrators, diagnostics.
- Managing the gateway (administrators; everyone in development mode): register a server as a
  local process or a remote one, including the OAuth authorization of a remote server in the
  browser; enable and disable servers, tools, prompts and resources; delete a server; create,
  change and delete tool groups. Users, teams, MCP clients and grants are still managed with
  the CLI or the API.

### Not in this version

- The legacy HTTP+SSE transport, for clients and for upstreams: Streamable HTTP only.
- Access tokens are stored as they are, not hashed; protect the database and its backups.
