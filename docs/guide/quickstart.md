# Quickstart

This walks through a local, single-user setup: start Trunks, register one MCP server, call a
tool through the gateway, and connect a client. It uses development mode, which has **no
authentication** and is therefore reachable from your own machine only. For a shared server,
continue with [Enterprise mode](enterprise.md) afterwards.

## 1. Install

Tagged releases publish archives and installers for Linux, macOS and Windows (x86_64 and arm64):

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/websublime/trunks/releases/latest/download/trunks-installer.sh | sh
```

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/websublime/trunks/releases/latest/download/trunks-installer.ps1 | iex"
```

These links follow the newest final release. To install a pre-release, name it: replace
`latest/download` with `download/<tag>`, for example `download/v0.1.0-rc.2`.

To build from source instead (Rust 1.94 or newer):

```bash
cargo install --locked --git https://github.com/websublime/trunks trunks
```

There is one binary. `trunks start` is the server; every other command is the management CLI.

## 2. Start the server

```bash
trunks start
```

```text
Trunks HTTP server listening on 127.0.0.1:8080
```

In development mode Trunks listens on this machine only. The data goes to `trunks.db`
(SQLite) in the current directory. The web console is at <http://127.0.0.1:8080/>: it shows the
servers, tools, prompts, resources and tool groups the gateway holds, and servers and tool
groups can be managed there too. This guide uses the CLI.

## 3. Register an MCP server

In another terminal, register a server that Trunks starts as a local process. This one is the
reference "everything" server and needs Node.js:

```bash
trunks register --name everything \
  --command npx --arg -y --arg @modelcontextprotocol/server-everything \
  --session-mode stateful
```

```text
Server everything registered successfully!

Tools (13):
  everything__echo  Echoes back the input string
  everything__get-sum  Returns the sum of two numbers
  ...
```

Trunks connected to the server, asked it for its tools, prompts and resources, and stored
them. Each one is now published under the server's name: the server's `echo` tool is
`everything__echo`.

A server reachable over HTTP is registered by its address instead:

```bash
trunks register --name docs --url https://mcp.example.com/mcp --bearer-token "$DOCS_TOKEN"
```

## 4. Call a tool

```bash
trunks usage everything__echo
```

```text
everything__echo
  Echoes back the input string
  server: everything   status: enabled

Input parameters:
  - message (string, required)
      Message to echo
```

```bash
trunks invoke everything__echo --input '{"message":"hello"}'
```

```text
Response from tool:
Echo: hello
```

The same call over the REST API:

```bash
curl -s http://127.0.0.1:8080/api/v0/tools/invoke \
  -H 'content-type: application/json' \
  -d '{"name":"everything__get-sum","a":2,"b":3}'
```

```json
{"content":[{"text":"The sum of 2 and 3 is 5.","type":"text"}]}
```

## 5. Connect an MCP client

The gateway's MCP endpoint is `http://127.0.0.1:8080/mcp` (Streamable HTTP). With Claude Code:

```bash
claude mcp add --transport http trunks http://127.0.0.1:8080/mcp
```

Other clients are covered in [Connecting clients](clients.md). The client now lists
`everything__echo` and the other tools, and every server you register later shows up on the
same endpoint.

## Where to go next

- Register more servers, with credentials or OAuth: [Upstream servers](upstreams.md).
- Expose only some tools to a client: [Tool groups](tool-groups.md).
- Share one server between people and agents, each with their own access:
  [Enterprise mode](enterprise.md).
- Run it in a container or behind a reverse proxy: [Operations](operations.md).
