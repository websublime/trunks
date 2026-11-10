# Connecting clients

Any MCP client that supports the Streamable HTTP transport can use Trunks. It needs two
things:

| | Development mode | Enterprise mode |
|---|---|---|
| URL | `http://127.0.0.1:8080/mcp` | `https://trunks.example.com/mcp` |
| Header | none | `Authorization: Bearer <MCP client token>` |

To expose only a [tool group](tool-groups.md), use `/v0/groups/<name>/mcp` instead of `/mcp`.

The token is the one printed by `trunks create mcp-client` ([Enterprise mode](enterprise.md#mcp-clients)).
A user's token (the one used by `trunks login`) does not work here.

The examples below use the enterprise form. For development mode, use the local URL and leave
out the header.

## Claude Code

```bash
claude mcp add --transport http trunks https://trunks.example.com/mcp \
  --header "Authorization: Bearer $TRUNKS_MCP_TOKEN"
```

Or, shared with a project through `.mcp.json`, with the token taken from each person's
environment:

```json
{
  "mcpServers": {
    "trunks": {
      "type": "http",
      "url": "https://trunks.example.com/mcp",
      "headers": { "Authorization": "Bearer ${TRUNKS_MCP_TOKEN}" }
    }
  }
}
```

## Cursor

`~/.cursor/mcp.json` (all projects) or `.cursor/mcp.json` (one project):

```json
{
  "mcpServers": {
    "trunks": {
      "url": "https://trunks.example.com/mcp",
      "headers": { "Authorization": "Bearer ${env:TRUNKS_MCP_TOKEN}" }
    }
  }
}
```

## VS Code (GitHub Copilot)

`.vscode/mcp.json` in the workspace, or the user-level `mcp.json`. VS Code asks for the token
once and stores it:

```json
{
  "inputs": [
    { "type": "promptString", "id": "trunks-token", "description": "Trunks MCP client token", "password": true }
  ],
  "servers": {
    "trunks": {
      "type": "http",
      "url": "https://trunks.example.com/mcp",
      "headers": { "Authorization": "Bearer ${input:trunks-token}" }
    }
  }
}
```

## Claude Desktop

Claude Desktop starts local MCP servers from `claude_desktop_config.json`. Connect it to
Trunks through [`mcp-remote`](https://github.com/geelen/mcp-remote), a small local bridge
(needs Node.js):

```json
{
  "mcpServers": {
    "trunks": {
      "command": "npx",
      "args": [
        "-y", "mcp-remote", "https://trunks.example.com/mcp",
        "--transport", "http-only",
        "--header", "Authorization:${TRUNKS_AUTH}"
      ],
      "env": { "TRUNKS_AUTH": "Bearer <MCP client token>" }
    }
  }
}
```

The header value is passed through `env` because some clients mishandle spaces inside `args`.
For a plain `http://` address that is not on your own machine, `mcp-remote` also needs
`--allow-http`; use that only on a network you trust.

## Other clients and your own agents

Configure the URL and the `Authorization` header wherever the client takes a remote
(Streamable HTTP) MCP server. A client that only supports local `stdio` servers can use
`mcp-remote` as shown for Claude Desktop. Clients that only speak the older HTTP+SSE
transport cannot connect.

## What the client sees

- Tools named `<server>__<tool>`, prompts named `<server>__<prompt>` and resources with
  `trunks://` URIs, from every server the client may use.
- Trunks does not notify clients when the list changes. After registering a server or changing
  a grant, refresh or reconnect the client to see the new list.
- A tool that was disabled, removed or put out of the client's reach answers
  `tool '<name>' not found`.

## If it does not connect

| Symptom | Cause |
|---|---|
| `403 Forbidden: Host header is not allowed` | The host name the client uses is not in `TRUNKS_ALLOWED_HOSTS`. By default only loopback names (`localhost`, `127.0.0.1`) are accepted; set it to the server's public name. |
| `401 missing MCP client access token` | Enterprise mode, and the client sent no `Authorization` header. |
| `401 invalid MCP client token` | The token is not an MCP client's token: wrong, replaced, or a user token. |
| `403 server is not initialized` | Enterprise server on which `trunks init-server` has not been run. |
| Connects, but the tool list is empty | The MCP client can reach nothing (no owner and an empty allow list, or an owner without grants), or the URL is a tool group with no enabled tools. |
| Answers arrive late or all at once | A reverse proxy is buffering the response; see [Operations](operations.md#network). |

These setups follow each client's own documentation. Trunks' automated tests cover the MCP
protocol with a generic client; they do not run these applications.
