# Concepts

## Gateway and upstream servers

Trunks sits between MCP clients and MCP servers. The servers you register are **upstream
servers**. Clients connect to Trunks' own MCP endpoint and never to the upstreams directly.

```text
MCP clients ──▶  Trunks  /mcp  ──▶  upstream "github"  (HTTP)
                                ──▶  upstream "files"   (local process)
                                ──▶  upstream "jira"    (HTTP + OAuth)
```

When a server is registered, Trunks connects to it once, reads its tools, prompts and
resources, and stores them. That stored list is the **catalog**: listing tools never contacts
an upstream; only calling one does. To pick up tools an upstream added later, register it
again with `--force`.

Trunks uses the upstream credentials you configured and nothing else. Headers sent by an MCP
client (including its `Authorization` header) are never passed on to an upstream.

## Names

Everything is published under the name of the server it came from.

| Kind | Published as | Example |
|---|---|---|
| Tool | `<server>__<tool>` | `github__create_issue` |
| Prompt | `<server>__<prompt>` | `docs__summarize` |
| Resource | `trunks://res/<server>/<encoded original URI>` | `trunks://res/files/ZmlsZTovLy9SRUFETUUubWQ` |

A server name may contain letters, digits, `_` and `-`; it cannot contain `__` or end in `_`,
so the first `__` in a tool name always separates the server from the tool. Resource URIs are
rewritten in both directions: clients only ever see `trunks://` URIs, upstreams only their own.

## Transports

| Upstream transport | How Trunks reaches it |
|---|---|
| `stdio` | Trunks starts the command as a child process and speaks MCP over its stdin/stdout |
| `streamable_http` | Trunks sends HTTP requests to the server's URL |

Towards clients, Trunks speaks Streamable HTTP only. The older HTTP+SSE transport is not
supported in either direction.

## Session modes

Each upstream has a session mode, chosen at registration:

- `stateless` (default): every call opens a new connection to the upstream and closes it
  afterwards. For a `stdio` server that means starting the process on every call.
- `stateful`: one connection is kept open and reused. It is reopened on the next call after a
  failure, and closed after `TRUNKS_SESSION_IDLE_TIMEOUT_SEC` of inactivity when that is set.

Use `stateful` for servers that are slow to start (anything launched through `npx` or `uvx`)
or that keep state between calls. The connection is shared by all clients of the gateway.

## Enabled and disabled

Servers, tools, prompts and resources can be disabled without being removed. A disabled entry
is neither listed nor callable. Disabling a server hides everything it provides; enabling it
again restores each tool's own previous state.

## Tool groups

A **tool group** is a named selection of tools with its own MCP endpoint,
`/v0/groups/<name>/mcp`. A client connected there sees only that selection. Groups contain
tools only, not prompts or resources. See [Tool groups](tool-groups.md).

## Modes

A server runs in one of two modes, fixed when its database is first initialized:

| | `development` | `enterprise` |
|---|---|---|
| Authentication | none | an access token on every request |
| Who sees what | everyone sees everything | only what was granted |
| Management | anyone who can reach the server | administrators |

Development mode is for one person on one machine.

## Identities (enterprise mode)

| Identity | Used by | Authenticates on |
|---|---|---|
| **User** (role `admin` or `user`) | people, through the CLI and the REST API | `/api/v0` |
| **MCP client** | agents and MCP client applications | `/mcp` and `/v0/groups/<name>/mcp` |

The two are separate on purpose: a user token is refused on the MCP endpoints and an MCP
client token is refused on the REST API.

- **Teams** group users. A team member is a `member` or a `maintainer`; maintainers manage the
  team's members and its MCP clients.
- **Grants** give a user or a team the use of a server or a tool group. Administrators can use
  everything; a user with no grant sees nothing.
- An MCP client can have an **owner** (a user or a team). An owned client can never reach more
  than its owner can.

Anything outside a caller's reach is answered exactly as if it did not exist, in listings and
in errors alike.

## Endpoints

| Path | What | Who |
|---|---|---|
| `/mcp` | MCP endpoint with everything the caller may use | MCP clients |
| `/v0/groups/<name>/mcp` | MCP endpoint limited to one tool group | MCP clients |
| `/api/v0/...` | REST API used by the CLI | users |
| `/health`, `/metadata`, `/metrics` | liveness, version, Prometheus metrics | anyone who can reach them |

Full details are in the [HTTP API reference](reference/api.md).
