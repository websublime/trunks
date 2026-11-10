# Trunks guide

Trunks puts all your MCP servers behind one endpoint. You register each server once; MCP
clients (Claude, Cursor, Copilot, your own agents) connect to Trunks and see every tool,
prompt and resource they are allowed to use.

| Page | Read it to |
|---|---|
| [Quickstart](quickstart.md) | Go from nothing to a tool call through the gateway in five minutes |
| [Concepts](concepts.md) | Understand names, modes, sessions, groups and identities |
| [Upstream servers](upstreams.md) | Register stdio and HTTP servers, with credentials or OAuth |
| [Tool groups](tool-groups.md) | Give a client a curated subset of tools on its own endpoint |
| [Enterprise mode](enterprise.md) | Add users, teams, grants, MCP client tokens and single sign-on |
| [Connecting clients](clients.md) | Point Claude Code, Claude Desktop, Cursor and VS Code at Trunks |
| [Operations](operations.md) | Install, configure, observe, upgrade and troubleshoot a server |

Reference:

| Page | Contents |
|---|---|
| [CLI](reference/cli.md) | Every command and flag (generated from the binary) |
| [Configuration](reference/configuration.md) | Every server setting, its environment variable and default |
| [HTTP API](reference/api.md) | The REST API and the MCP endpoints |

The design documents next to this guide (`docs/01-produto.md` to `docs/07-desempenho.md`) are
written for people working on Trunks itself.
