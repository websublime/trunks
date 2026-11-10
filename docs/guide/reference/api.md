# HTTP API reference

Trunks serves three things over HTTP: the MCP endpoints for MCP clients, a REST API under
`/api/v0` (what the CLI uses), and a few unauthenticated service endpoints. Paths, JSON
shapes and error messages are stable within a major version.

## Conventions

- **Bodies** are JSON (`Content-Type: application/json`), at most 4 MiB.
- **Authentication** (enterprise mode): `Authorization: Bearer <token>`. The REST API takes a
  **user** token, the MCP endpoints take an **MCP client** token; neither accepts the other.
  In development mode no token is needed and every caller acts as an administrator.
- **Errors** are `{"error": "<message>"}`, sometimes with a machine-readable `"code"`.

| Status | Meaning |
|---|---|
| 400 | invalid input; the message says what |
| 401 | missing or invalid token |
| 403 | authenticated but not allowed; server not initialized; enterprise-only endpoint in development mode |
| 404 | not found, **or not within the caller's reach** (the two are indistinguishable on purpose) |
| 413 | body larger than 4 MiB |
| 500 | failure reaching an upstream or an internal error; the message carries the reason |
| 504 | the request, or an upstream, exceeded `TRUNKS_REQUEST_TIMEOUT_SEC` |

- **Names**: tools and prompts are `<server>__<name>`; resources are `trunks://res/<server>/…`
  URIs. See [Concepts](../concepts.md#names).
- Listings and lookups return only what the caller may use.

## Service endpoints

No authentication.

| Method and path | Response |
|---|---|
| `GET /health` | `{"status":"ok"}` while the process is serving |
| `GET /metadata` | `{"version":"v1.2.3"}` |
| `GET /metrics` | Prometheus text format; only when metrics are enabled |
| `POST /init` | initializes the server; see below |

`POST /init` with `{"mode":"enterprise"}` answers `{"status":"…","admin_access_token":"…"}`
once; this is what `trunks init-server` calls. The token is not shown again. A server that is
already initialized answers 400, and so does a `mode` other than the one the server was
started in. Development servers initialize themselves at start-up.

## MCP endpoints

| Path | Exposes |
|---|---|
| `/mcp` | every tool, prompt and resource the client may use |
| `/v0/groups/{name}/mcp` | the effective tools of one tool group; no prompts, no resources |

Both speak the MCP Streamable HTTP transport: JSON-RPC requests are sent with `POST` and
`Accept: application/json, text/event-stream`, and answers arrive as an event stream. The
older HTTP+SSE transport is not offered.

- Supported requests: `tools/list`, `tools/call`, `prompts/list`, `prompts/get`,
  `resources/list`, `resources/read`. Resource subscriptions and list-change notifications are
  not offered.
- A tool, prompt or resource that does not exist, is disabled, or is outside the client's
  reach answers JSON-RPC error `-32602` with `tool '<name>' not found` (likewise `prompt`,
  `resource`).
- An upstream that does not answer in time yields a JSON-RPC internal error naming the
  upstream.
- The `Host` header must be one of `TRUNKS_ALLOWED_HOSTS` (loopback names by default),
  otherwise 403.
- Unknown group: 404 `{"error":"tool group not found: <name>"}`.
- Headers sent by the client are never forwarded to upstreams.

## REST API: reading and calling

For any user. Base path `/api/v0`.

| Method and path | Request | Response |
|---|---|---|
| `GET /servers` | | `[Server]` |
| `GET /tools?server=` | `server` optional | `[Tool]` |
| `GET /tool?name=` | canonical tool name | `Tool` |
| `POST /tools/invoke` | `{"name": "<server>__<tool>", …arguments}` | the tool's result: `{"content":[…], "isError"?, "structuredContent"?}` |
| `GET /prompts?server=` | `server` optional | `[Prompt]` |
| `GET /prompt?name=` | canonical prompt name | `Prompt` |
| `POST /prompts/render` | `{"name": "<server>__<prompt>", "arguments": {…}}` | `{"description"?, "messages":[…]}` |
| `GET /resources?server=` | `server` optional | `[Resource]` |
| `POST /resources/get` | `{"uri": "trunks://…"}` | `Resource` |
| `POST /resources/read` | `{"uri": "trunks://…"}` | `{"contents":[…]}` |
| `GET /tool-groups` | | `[ToolGroup]` |
| `GET /tool-groups/{name}` | | `ToolGroup` plus `streamable_http_endpoint` |
| `GET /tool-groups/{name}/effective-tools` | | `[Tool]` |
| `GET /users/whoami` | enterprise mode | `{"username", "role", "teams":[{"name","role"}], "access":{"all","servers":[],"tool_groups":[]}}` |

In `POST /tools/invoke` the tool's arguments are the other members of the same object, next to
`name`:

```bash
curl -s https://trunks.example.com/api/v0/tools/invoke \
  -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"name":"everything__get-sum","a":2,"b":3}'
```

Shapes:

```text
Server    {name, transport, enabled, description, session_mode,
           url?, header_keys[]?, command?, args[]?, env_keys[]?, created_at?, updated_at?}
Tool      {name, server, enabled, description, input_schema?, annotations?}
Prompt    {name, server, enabled, description, arguments?}
Resource  {uri, server, name, enabled, description, mime_type?, annotations?, meta?}
ToolGroup {name, description, included_tools[], included_servers[], excluded_tools[]}
```

`transport` is `stdio` or `streamable_http`; `session_mode` is `stateless` or `stateful`. A
`Server` never includes credentials: only the names of its headers and environment variables.

## REST API: administration

Administrators only (in development mode, everyone).

### Servers

| Method and path | Request | Response |
|---|---|---|
| `POST /servers?force=` | server definition; `force=true` replaces a server of the same name | 201 `{"server": Server}`, or 202 `{"authorization_required": {"session_id","authorization_url","expires_at"}}` when the upstream needs OAuth |
| `POST /upstream_oauth/sessions/{id}/complete` | `{"code","state"}` | 201 `{"server": Server}` |
| `DELETE /servers/{name}` | | 204 |
| `POST /servers/{name}/enable`, `/disable` | | `{"name", "tools_affected"?, "prompts_affected"?, "resources_affected"?}` |
| `POST /tools/enable?entity=`, `/tools/disable?entity=` | `entity` is a tool name, or a server name for all its tools | `{"tools_affected":[…]}` |
| `POST /prompts/enable?entity=`, `/prompts/disable?entity=` | prompt name or server name | `{"prompts_affected":[…]}` |
| `POST /resources/enable?entity=`, `/resources/disable?entity=` | `trunks://` URI or server name | `{"resources_affected":[…]}` |
| `GET /server_configs` | | `[server definition]`, **including credentials**; what `trunks export` writes |

The server definition is the JSON accepted by `trunks register -c`; its fields are listed in
[Upstream servers](../upstreams.md#from-a-file). Registration connects to the upstream first:
if that fails, the answer is 500 with the reason and nothing is stored. When the upstream
demands OAuth and the request has no `oauth_redirect_uri`, the answer is 400 with
`"code":"upstream_oauth_required"`.

### Tool groups

| Method and path | Request | Response |
|---|---|---|
| `POST /tool-groups` | `ToolGroup` | 201 `ToolGroup` plus `streamable_http_endpoint` |
| `PUT /tool-groups/{name}` | `ToolGroup` (its `name` is ignored) | `{"name","old": ToolGroup,"new": ToolGroup}` |
| `DELETE /tool-groups/{name}` | | 204, also when it did not exist |

## REST API: identities and access

Enterprise mode only; in development mode these answer 403.

### Users (administrators)

| Method and path | Request | Response |
|---|---|---|
| `GET /users` | | `[{"username","role"}]` |
| `POST /users` | `{"username", "access_token"?}` | 201 `{"username","role","access_token"}` |
| `PUT /users/{username}` | `{"access_token"?}`; generates a new token when omitted | `{"username","role","access_token"}` |
| `DELETE /users/{username}` | | 204; the last administrator cannot be deleted (400) |

Tokens appear in a response only when created or replaced. A custom token has at least 8
characters and no whitespace.

### MCP clients

| Method and path | Who | Request | Response |
|---|---|---|---|
| `GET /clients` | any user | | `[McpClient]`: all for an administrator; otherwise the caller's own and their teams' |
| `POST /clients` | any user | `{"name","description"?,"allow_list"?:[…],"access_token"?,"owner"?:{"kind","name"}}` | 201 `McpClient` plus `access_token` |
| `PUT /clients/{name}` | administrator, owner, or maintainer of the owning team | `{"description"?,"allow_list"?,"access_token"?,"rotate_token"?}`; only the given fields change | `McpClient`, plus `access_token` when it changed |
| `DELETE /clients/{name}` | the same | | 204 |

`McpClient` is `{name, description, allow_list[], owner?: {kind: "user"|"team", name}}`.
`allow_list` holds server names or `"*"`. A user who names no owner becomes the owner; naming
a team requires being one of its maintainers; only an administrator can create a client
without an owner. How owner and allow list combine is described in
[Enterprise mode](../enterprise.md#mcp-clients).

### Teams

| Method and path | Who | Request | Response |
|---|---|---|---|
| `GET /teams` | any user | | `[{"name","description"}]` |
| `GET /teams/{name}` | any user | | `{"name","description","members":[{"username","role"}]}` |
| `POST /teams` | administrator | `{"name","description"?}` | 201, the team with its members |
| `PUT /teams/{name}` | administrator | `{"description"}` | the team |
| `DELETE /teams/{name}` | administrator | | 204 |
| `POST /teams/{name}/members` | administrator or team maintainer | `{"username","role"?}`; `role` is `member` (default) or `maintainer`; an existing member's role is updated | the team with its members |
| `DELETE /teams/{name}/members/{username}` | administrator or team maintainer | | 204 |

### Grants (administrators)

A grant is `{"subject_kind": "user"|"team", "subject": "<name>", "target_kind":
"server"|"tool_group", "target": "<name>"}`. The target `"*"` with `target_kind` `server`
means every server.

| Method and path | Request | Response |
|---|---|---|
| `GET /grants?target_kind=&target=` | both filters optional | `[grant]` |
| `POST /grants` | grant | 201; 200 when it already existed |
| `DELETE /grants` | grant, as the body | 204 |
| `GET /servers/{name}/access` | | `[grant]` on that server |
| `GET /tool-groups/{name}/access` | | `[grant]` on that group |

The subject must exist (404 otherwise). The target is a name and does not have to exist yet.

## Browser sign-in

These endpoints serve the web application (not part of this version yet). `GET /auth/config`
always answers; the others exist in enterprise mode only and answer 403 in development mode.

| Method and path | Purpose |
|---|---|
| `GET /auth/config` | `{"mode", "login_required", "oidc"}`: what a sign-in page needs to know |
| `GET /auth/login?return_to=` | redirects to the OIDC provider; 404 when single sign-on is not configured |
| `GET /auth/callback` | the provider's redirect target; sets the session cookie and redirects to `return_to` |
| `POST /auth/token` | `{"access_token"}` (a user token) → `{"username","role"}` and a session cookie |
| `POST /auth/logout` | 204; ends the session |

A session is an `HttpOnly`, `SameSite=Lax` cookie named `trunks_session`. It is accepted by the
REST API in place of a bearer token and never by the MCP endpoints. Requests that change
something and are authenticated by the cookie must carry an `Origin` header equal to the
public URL (or to the request's `Host`); otherwise 403.

## Dashboard API

`GET /api/dashboard/{overview, servers, servers/{name}, tools, prompts, resources,
tool-groups, tool-groups/{name}, diagnostics, oauth/session/{id}}` returns aggregated,
read-only views for the web application. No response contains a credential: servers are
described by their URL or command and by the *names* of their headers and environment
variables.

In enterprise mode these views need a user token but are **not limited by the caller's
grants**: any user can read the full list of servers, tools and groups here.

`diagnostics` is the one most useful on its own: it reports the database backend, the listen
address, the allowed hosts, the time limits, the upstreams whose last connection failed (with
the error) and the OAuth authorizations still pending.
