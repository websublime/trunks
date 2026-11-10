# CLI reference

<!-- Generated from the command definitions. Do not edit by hand:
     TRUNKS_UPDATE_DOCS=1 cargo test -p trunks cli_reference -->

`trunks start` runs the server; every other command talks to a running server, whose address
comes from `--registry`, the `TRUNKS_REGISTRY` environment variable or the CLI's configuration
file, in that order (default `http://127.0.0.1:8080`). In enterprise mode the commands send the
access token stored by `trunks login` or `trunks init-server`.

## `trunks start`

```text
Start the Trunks gateway server

Usage: trunks start [OPTIONS]

Options:
      --port <PORT>
          Port to listen on

          [env: TRUNKS_PORT]
          [default: 8080]

      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --host <HOST>
          Host to bind to; empty = all interfaces [default: 127.0.0.1 in development mode, all interfaces in enterprise mode]

          [env: TRUNKS_BIND_HOST]

      --mode <MODE>
          Operating mode: development | enterprise

          [env: TRUNKS_MODE]
          [default: development]

      --database-url <DATABASE_URL>
          Database URL: sqlite://<path> or postgres://user:pass@host:port/db

          [env: TRUNKS_DATABASE_URL]
          [default: sqlite://trunks.db]

      --database-url-file <DATABASE_URL_FILE>
          Read the database URL from this file instead (for a secret mounted as a file); when set, it takes precedence over the database URL

          [env: TRUNKS_DATABASE_URL_FILE]

      --metrics <METRICS>
          Expose Prometheus metrics at /metrics (default: on in enterprise mode, off in development)

          [env: TRUNKS_METRICS_ENABLED]
          [possible values: true, false]

      --upstream-init-timeout-sec <UPSTREAM_INIT_TIMEOUT_SEC>
          Timeout (seconds) for the MCP initialize handshake with upstream servers

          [env: TRUNKS_UPSTREAM_INIT_TIMEOUT_SEC]
          [default: 30]

      --request-timeout-sec <REQUEST_TIMEOUT_SEC>
          Upper bound (seconds) for one REST request and for one upstream call (0 = no limit)

          [env: TRUNKS_REQUEST_TIMEOUT_SEC]
          [default: 300]

      --session-idle-timeout-sec <SESSION_IDLE_TIMEOUT_SEC>
          Close stateful upstream sessions idle for this many seconds (0 = never)

          [env: TRUNKS_SESSION_IDLE_TIMEOUT_SEC]
          [default: 0]

      --allowed-hosts <ALLOWED_HOSTS>
          Host header values accepted on the MCP endpoints (comma-separated; "*" disables the check)

          [env: TRUNKS_ALLOWED_HOSTS]

      --public-url <PUBLIC_URL>
          Public base URL of this server (e.g. https://trunks.example.com). Required for OIDC; also decides the `Secure` cookie flag and the same-origin check

          [env: TRUNKS_PUBLIC_URL]

      --session-ttl-hours <SESSION_TTL_HOURS>
          Browser session lifetime, in hours

          [env: TRUNKS_SESSION_TTL_HOURS]
          [default: 12]

      --oidc-issuer <OIDC_ISSUER>
          OIDC issuer URL; setting it enables "Sign in with SSO" (enterprise mode)

          [env: TRUNKS_OIDC_ISSUER]

      --oidc-client-id <OIDC_CLIENT_ID>
          [env: TRUNKS_OIDC_CLIENT_ID]

      --oidc-client-secret <OIDC_CLIENT_SECRET>
          Prefer the environment variable over the flag: flags are visible in process listings

          [env: TRUNKS_OIDC_CLIENT_SECRET]

      --oidc-scopes <OIDC_SCOPES>
          Scopes requested in addition to `openid` (comma-separated)

          [env: TRUNKS_OIDC_SCOPES]
          [default: profile,email]

      --oidc-groups-claim <OIDC_GROUPS_CLAIM>
          Claim (ID token or userinfo) holding the user's groups

          [env: TRUNKS_OIDC_GROUPS_CLAIM]
          [default: groups]

      --oidc-admin-group <OIDC_ADMIN_GROUP>
          Group that makes a user an administrator (role is re-synced on every login)

          [env: TRUNKS_OIDC_ADMIN_GROUP]

      --oidc-sync-teams
          Sync team memberships from the groups claim on login

          [env: TRUNKS_OIDC_SYNC_TEAMS]

      --oidc-team-prefix <OIDC_TEAM_PREFIX>
          Only groups with this prefix become teams (the prefix is stripped)

          [env: TRUNKS_OIDC_TEAM_PREFIX]
          [default: ""]

  -h, --help
          Print help
```

## `trunks register`

```text
Register an upstream MCP server

Usage: trunks register [OPTIONS]

Options:
  -c, --conf <FILE>
          JSON file with the server definition (`${VAR}` references are expanded)

      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --name <NAME>
          Server name (letters, digits, `_` and `-`)

      --transport <TRANSPORT>
          [possible values: stdio, streamable-http]

      --description <DESCRIPTION>


      --url <URL>
          streamable_http: upstream URL

      --bearer-token <BEARER_TOKEN>
          streamable_http: sent as `Authorization: Bearer …`

      --header <NAME=VALUE>
          streamable_http: extra header, `Name: value` or `Name=value` (repeatable)

      --command <COMMAND>
          stdio: program to run

      --arg <ARG>
          stdio: program argument (repeatable, in order)

      --env <NAME=VALUE>
          stdio: environment variable `NAME=value` (repeatable)

      --session-mode <SESSION_MODE>
          [possible values: stateless, stateful]

      --force
          Replace an existing server of the same name

      --oauth-redirect-uri <OAUTH_REDIRECT_URI>
          OAuth: redirect URI of a pre-registered client (default: a loopback listener started by the CLI). With a custom URI the flow must be completed with `trunks oauth-complete`

      --oauth-client-id <OAUTH_CLIENT_ID>
          OAuth: client id of a pre-registered client (skips dynamic registration)

      --oauth-client-secret <OAUTH_CLIENT_SECRET>


      --oauth-scope <SCOPE>
          OAuth: scope to request (repeatable)

  -h, --help
          Print help
```

## `trunks deregister`

```text
Deregister an upstream MCP server

Usage: trunks deregister [OPTIONS] <NAME>

Arguments:
  <NAME>
          Server name

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks enable`

```text
Enable a server, tool, prompt or resource

Usage: trunks enable [OPTIONS] <COMMAND>

Commands:
  server    A registered server (its tools, prompts and resources follow)
  tool      A canonical tool name, or a server name for all its tools
  prompt    A canonical prompt name, or a server name for all its prompts
  resource  A trunks:// resource URI, or a server name for all its resources

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks enable server`

```text
A registered server (its tools, prompts and resources follow)

Usage: trunks enable server [OPTIONS] <NAME>

Arguments:
  <NAME>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks enable tool`

```text
A canonical tool name, or a server name for all its tools

Usage: trunks enable tool [OPTIONS] <ENTITY>

Arguments:
  <ENTITY>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks enable prompt`

```text
A canonical prompt name, or a server name for all its prompts

Usage: trunks enable prompt [OPTIONS] <ENTITY>

Arguments:
  <ENTITY>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks enable resource`

```text
A trunks:// resource URI, or a server name for all its resources

Usage: trunks enable resource [OPTIONS] <ENTITY>

Arguments:
  <ENTITY>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks disable`

```text
Disable a server, tool, prompt or resource

Usage: trunks disable [OPTIONS] <COMMAND>

Commands:
  server    A registered server (its tools, prompts and resources follow)
  tool      A canonical tool name, or a server name for all its tools
  prompt    A canonical prompt name, or a server name for all its prompts
  resource  A trunks:// resource URI, or a server name for all its resources

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks disable server`

```text
A registered server (its tools, prompts and resources follow)

Usage: trunks disable server [OPTIONS] <NAME>

Arguments:
  <NAME>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks disable tool`

```text
A canonical tool name, or a server name for all its tools

Usage: trunks disable tool [OPTIONS] <ENTITY>

Arguments:
  <ENTITY>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks disable prompt`

```text
A canonical prompt name, or a server name for all its prompts

Usage: trunks disable prompt [OPTIONS] <ENTITY>

Arguments:
  <ENTITY>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks disable resource`

```text
A trunks:// resource URI, or a server name for all its resources

Usage: trunks disable resource [OPTIONS] <ENTITY>

Arguments:
  <ENTITY>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks get`

```text
Show a tool group, render a prompt or show/read a resource

Usage: trunks get [OPTIONS] <COMMAND>

Commands:
  group     Show a tool group and its effective tools
  prompt    Render a prompt
  resource  Show a resource; `--read` fetches its contents from the upstream

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks get group`

```text
Show a tool group and its effective tools

Usage: trunks get group [OPTIONS] <NAME>

Arguments:
  <NAME>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks get prompt`

```text
Render a prompt

Usage: trunks get prompt [OPTIONS] <NAME>

Arguments:
  <NAME>
          Canonical prompt name (<server>__<prompt>)

Options:
      --arg <KEY=VALUE>
          Prompt argument `key=value` (repeatable)

      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks get resource`

```text
Show a resource; `--read` fetches its contents from the upstream

Usage: trunks get resource [OPTIONS] <URI>

Arguments:
  <URI>
          trunks:// resource URI

Options:
      --read


      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks export`

```text
Export server definitions as JSON files (includes credentials)

Usage: trunks export [OPTIONS]

Options:
  -d, --dir <DIR>
          Output directory (must not exist or be empty)

          [default: .trunks]

      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks oauth-complete`

```text
Finish an upstream OAuth authorization started with a custom redirect URI

Usage: trunks oauth-complete [OPTIONS] --code <CODE> --state <STATE> <SESSION_ID>

Arguments:
  <SESSION_ID>
          Session id printed by `trunks register`

Options:
      --code <CODE>


      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --state <STATE>


  -h, --help
          Print help
```

## `trunks add-team-member`

```text
Add a user to a team (enterprise mode)

Usage: trunks add-team-member [OPTIONS] <TEAM> <USER>

Arguments:
  <TEAM>
          Team name

  <USER>
          Username

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --role <ROLE>
          [default: member]
          [possible values: member, maintainer]

  -h, --help
          Print help
```

## `trunks remove-team-member`

```text
Remove a user from a team (enterprise mode)

Usage: trunks remove-team-member [OPTIONS] <TEAM> <USER>

Arguments:
  <TEAM>


  <USER>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks grant`

```text
Grant a user or team access to a server or tool group (enterprise mode)

Usage: trunks grant [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --user <USER>


      --team <TEAM>


      --server <SERVER>


      --group <GROUP>


  -h, --help
          Print help
```

## `trunks revoke`

```text
Revoke a grant (enterprise mode)

Usage: trunks revoke [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --user <USER>


      --team <TEAM>


      --server <SERVER>


      --group <GROUP>


  -h, --help
          Print help
```

## `trunks list`

```text
List servers, tools, prompts or resources

Usage: trunks list [OPTIONS] <COMMAND>

Commands:
  servers      Registered upstream servers
  tools        Tools exposed by the gateway
  groups       Tool groups
  prompts      Prompts exposed by the gateway
  resources    Resources exposed by the gateway
  users        Users of the REST API (enterprise mode)
  mcp-clients  MCP client identities (enterprise mode)
  teams        Teams (enterprise mode)
  grants       Access grants (enterprise mode)

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks list servers`

```text
Registered upstream servers

Usage: trunks list servers [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks list tools`

```text
Tools exposed by the gateway

Usage: trunks list tools [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --server <SERVER>
          Only tools of this server

      --group <GROUP>
          Only the effective tools of this tool group

  -h, --help
          Print help
```

## `trunks list groups`

```text
Tool groups

Usage: trunks list groups [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks list prompts`

```text
Prompts exposed by the gateway

Usage: trunks list prompts [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --server <SERVER>


  -h, --help
          Print help
```

## `trunks list resources`

```text
Resources exposed by the gateway

Usage: trunks list resources [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --server <SERVER>


  -h, --help
          Print help
```

## `trunks list users`

```text
Users of the REST API (enterprise mode)

Usage: trunks list users [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks list mcp-clients`

```text
MCP client identities (enterprise mode)

Usage: trunks list mcp-clients [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks list teams`

```text
Teams (enterprise mode)

Usage: trunks list teams [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks list grants`

```text
Access grants (enterprise mode)

Usage: trunks list grants [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --server <SERVER>
          Only grants on this server

      --group <GROUP>
          Only grants on this tool group

  -h, --help
          Print help
```

## `trunks usage`

```text
Show how to call a tool (input schema)

Usage: trunks usage [OPTIONS] <NAME>

Arguments:
  <NAME>
          Canonical tool name (<server>__<tool>)

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks invoke`

```text
Invoke a tool through the gateway

Usage: trunks invoke [OPTIONS] <NAME>

Arguments:
  <NAME>
          Canonical tool name (<server>__<tool>)

Options:
      --input <INPUT>
          JSON object with the tool's arguments

          [default: {}]

      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --group <GROUP>
          Only allow the call if the tool is an effective tool of this group

  -h, --help
          Print help
```

## `trunks init-server`

```text
Initialize a server running in enterprise mode (creates the admin user)

Usage: trunks init-server [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks login`

```text
Store a user access token for this registry

Usage: trunks login [OPTIONS] <TOKEN>

Arguments:
  <TOKEN>
          Access token of a user (shown once by `init-server` or `create user`)

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks create`

```text
Create a tool group, a user or an MCP client

Usage: trunks create [OPTIONS] <COMMAND>

Commands:
  group       A tool group from a JSON file (`-c`)
  team        A team (enterprise mode)
  user        A human user of the REST API and the CLI (role `user`)
  mcp-client  A machine identity for the MCP endpoints

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks create group`

```text
A tool group from a JSON file (`-c`)

Usage: trunks create group [OPTIONS] --conf <FILE>

Options:
  -c, --conf <FILE>
          JSON file: `{name, description, included_tools[], included_servers[], excluded_tools[]}`

      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks create team`

```text
A team (enterprise mode)

Usage: trunks create team [OPTIONS] <NAME>

Arguments:
  <NAME>
          Team name (no whitespace)

Options:
      --description <DESCRIPTION>
          [default: ""]

      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks create user`

```text
A human user of the REST API and the CLI (role `user`)

Usage: trunks create user [OPTIONS] [NAME]

Arguments:
  [NAME]
          Username (no whitespace)

Options:
      --access-token <ACCESS_TOKEN>
          Custom token (≥ 8 chars, no whitespace); generated when omitted

      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -c, --conf <FILE>
          JSON file with `username` and optional `access_token` (`${VAR}` expanded)

  -h, --help
          Print help
```

## `trunks create mcp-client`

```text
A machine identity for the MCP endpoints

Usage: trunks create mcp-client [OPTIONS] [NAME]

Arguments:
  [NAME]
          Client name (no whitespace)

Options:
      --allow <SERVER>
          Server the client may reach (repeatable, or comma-separated); `*` for all

      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --description <DESCRIPTION>


      --access-token <ACCESS_TOKEN>
          Custom token (≥ 8 chars, no whitespace); generated when omitted

      --owner <KIND:NAME>
          Owner: `user:<name>` or `team:<name>` (admins may omit; users default to themselves)

  -c, --conf <FILE>
          JSON file with `name`, `description`, `allow_list`, `access_token`, `owner` (`${VAR}` expanded)

  -h, --help
          Print help
```

## `trunks update`

```text
Update a tool group, a user's token or an MCP client

Usage: trunks update [OPTIONS] <COMMAND>

Commands:
  group       Replace a tool group's definition from a JSON file (`-c`)
  user        Rotate a user's token
  mcp-client  Change an MCP client's allow list, description or token

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks update group`

```text
Replace a tool group's definition from a JSON file (`-c`)

Usage: trunks update group [OPTIONS] --conf <FILE> <NAME>

Arguments:
  <NAME>
          Group name (the file's `name` is ignored)

Options:
  -c, --conf <FILE>


      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks update user`

```text
Rotate a user's token

Usage: trunks update user [OPTIONS] <NAME>

Arguments:
  <NAME>


Options:
      --access-token <ACCESS_TOKEN>
          Custom token; generated when omitted

      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks update mcp-client`

```text
Change an MCP client's allow list, description or token

Usage: trunks update mcp-client [OPTIONS] <NAME>

Arguments:
  <NAME>


Options:
      --allow <SERVER>
          New allow list (repeatable or comma-separated); replaces the current one

      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

      --description <DESCRIPTION>


      --access-token <ACCESS_TOKEN>
          Set a custom token

      --rotate-token
          Generate a new token

  -h, --help
          Print help
```

## `trunks delete`

```text
Delete a tool group, a user or an MCP client

Usage: trunks delete [OPTIONS] <COMMAND>

Commands:
  group
  team
  user
  mcp-client

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks delete group`

```text
Usage: trunks delete group [OPTIONS] <NAME>

Arguments:
  <NAME>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks delete team`

```text
Usage: trunks delete team [OPTIONS] <NAME>

Arguments:
  <NAME>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks delete user`

```text
Usage: trunks delete user [OPTIONS] <NAME>

Arguments:
  <NAME>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks delete mcp-client`

```text
Usage: trunks delete mcp-client [OPTIONS] <NAME>

Arguments:
  <NAME>


Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```

## `trunks version`

```text
Print the CLI and server versions

Usage: trunks version [OPTIONS]

Options:
      --registry <REGISTRY>
          Base URL of the Trunks server (overrides the config file)

          [env: TRUNKS_REGISTRY]

  -h, --help
          Print help
```
