# Upstream servers

An upstream server is an MCP server registered in Trunks. In enterprise mode only
administrators can register, change or remove one.

Registration is all-or-nothing: Trunks connects to the server, completes the MCP handshake and
lists its tools. If any of that fails, nothing is stored and the command prints the reason.

## Local process (`stdio`)

```bash
trunks register --name files \
  --command npx --arg -y --arg @modelcontextprotocol/server-filesystem --arg /srv/shared \
  --env LOG_LEVEL=warn \
  --session-mode stateful
```

- `--arg` is repeated once per argument, in order. `--env NAME=value` is repeated per variable.
- The process runs on the machine (or in the container) where Trunks runs, as the Trunks user.
  It gets Trunks' own environment plus the variables given with `--env`.
- When the session ends, the process and everything it started are terminated.
- What the process writes to stderr appears in the Trunks log (see
  [Operations](operations.md#observe)); values given with `--env` are replaced by `[redacted]`.

Registering a `stdio` server lets its command run on the Trunks host. Only register commands
you trust. The container image without a `-stdio` suffix has no `npx` or `uvx`.

## Remote server (`streamable_http`)

```bash
trunks register --name github \
  --url https://api.githubcopilot.com/mcp/ \
  --bearer-token "$GITHUB_TOKEN"
```

- `--bearer-token` is sent as `Authorization: Bearer …`.
- `--header 'Name: value'` (repeatable) adds any other header. An explicit `Authorization`
  header takes precedence over `--bearer-token`.
- A server on the Trunks host itself is `http://127.0.0.1:<port>/…`; when Trunks runs in a
  container, the host is usually `http://host.docker.internal:<port>/…`.

Servers that only speak the older HTTP+SSE transport cannot be registered.

## Remote server with OAuth

If the server answers the first connection with an OAuth challenge, the CLI runs the
authorization in your browser:

```bash
trunks register --name jira --url https://mcp.example.com/mcp
```

1. The CLI prints an authorization URL and tries to open it in your browser.
2. You sign in and approve access.
3. The browser is sent back to a temporary listener the CLI opened on `127.0.0.1`, and the
   registration finishes.

Trunks registers itself with the authorization server automatically when the server supports
dynamic client registration. Otherwise create an OAuth client there yourself and pass it:

```bash
trunks register --name jira --url https://mcp.example.com/mcp \
  --oauth-client-id "$CLIENT_ID" --oauth-client-secret "$CLIENT_SECRET" \
  --oauth-scope read --oauth-scope write
```

If the redirect cannot go to the machine running the CLI, give the redirect URI registered for
your OAuth client with `--oauth-redirect-uri`. The CLI then prints the authorization URL and a
session id and stops; after approving, take `code` and `state` from the redirect and run:

```bash
trunks oauth-complete <session-id> --code <code> --state <state>
```

An authorization that is not completed within 10 minutes expires; run `register` again.
Trunks stores the tokens and refreshes them when they expire.

## In the web console

Everything above can be done from the console's **Servers** page (administrators only in
enterprise mode):

- **Register server** opens a form with the same fields as the flags: a local process
  (command, arguments, environment variables) or a remote server (URL, bearer token, headers),
  and the session mode. Values of variables, headers and tokens are sent to the server once
  and never shown again.
- When a remote server asks for OAuth, the console shows "Authorization required". **Open
  provider** opens the provider in a new window; once you approve, the provider sends the
  browser back to Trunks and the server is registered. As with the CLI, nothing is stored
  until then, and the request expires after 10 minutes.
- For a provider without dynamic client registration, open "Authorization with your own OAuth
  client" in the form: it shows the redirect URI to register at the provider,
  `<public URL>/api/dashboard/oauth/callback`, and takes the client ID, secret and scopes.
- A server's page has a switch for the whole server and one for each tool, prompt and
  resource, and **Delete**.

Replacing the definition of a registered server (`--force`) and export and import are CLI only.

## From a file

The same definition as JSON, for version control or automation:

```json
{
  "name": "github",
  "transport": "streamable_http",
  "description": "GitHub tools",
  "url": "https://api.githubcopilot.com/mcp/",
  "bearer_token": "${GITHUB_TOKEN}",
  "session_mode": "stateless"
}
```

```bash
trunks register -c github.json
```

`${VAR}` is replaced by the environment variable of the CLI process; a missing variable is an
error. Flags given alongside `-c` override the file. All fields:

| Field | For | Meaning |
|---|---|---|
| `name` | all | server name: letters, digits, `_`, `-` |
| `transport` | all | `stdio` or `streamable_http` |
| `description` | all | free text |
| `session_mode` | all | `stateless` (default) or `stateful` |
| `command`, `args`, `env` | `stdio` | program, argument list, environment variables |
| `url` | `streamable_http` | server URL |
| `bearer_token`, `headers` | `streamable_http` | credentials and extra headers |
| `oauth_redirect_uri`, `oauth_client_id`, `oauth_client_secret`, `oauth_scopes` | `streamable_http` | OAuth, as the flags above |

## Changing and removing

| To | Run |
|---|---|
| Replace a server's definition, or re-read its tools | `trunks register … --force` |
| Hide a server without removing it | `trunks disable server <name>` / `trunks enable server <name>` |
| Hide one tool, prompt or resource | `trunks disable tool <server>__<tool>` (also `prompt`, `resource`) |
| Hide all tools of a server | `trunks disable tool <server>` |
| Remove a server | `trunks deregister <name>` |

Removing a server also removes its stored OAuth tokens. Grants and tool groups refer to
servers by name, so they apply again if a server of the same name is registered later.

## Inspecting

```bash
trunks list servers
trunks list tools --server github
trunks list prompts
trunks list resources
trunks usage github__create_issue                 # input schema of a tool
trunks get prompt docs__summarize --arg topic=mcp # render a prompt
trunks get resource '<trunks:// URI>' --read      # read a resource
```

## Backup and transfer

```bash
trunks export -d backup/
```

writes `backup/servers/<name>.json` and `backup/groups/<name>.json`, in the format `register
-c` and `create group -c` accept. **The files contain the credentials** (bearer tokens,
headers, `env` values) in clear text.

Not in the export: OAuth client registrations and tokens (a server registered with OAuth has
to be authorized again where it is imported), which entries are disabled, and anything about
users, teams, grants or MCP clients. For a full copy, back up the database
([Operations](operations.md#database)).

## Limits

- The MCP handshake with an upstream must finish within `TRUNKS_UPSTREAM_INIT_TIMEOUT_SEC`
  (30 s).
- Each call to an upstream must be answered within `TRUNKS_REQUEST_TIMEOUT_SEC` (300 s; `0`
  disables). After that the caller gets an error naming the upstream, and a `stateful`
  connection is reopened on the next call.
- A failed call is not retried.
