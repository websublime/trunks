# Enterprise mode

Enterprise mode is for a server shared by several people and agents. Every request carries an
access token, and each user and MCP client sees only what it has been granted.

## Start and initialize

```bash
TRUNKS_MODE=enterprise trunks start
```

A new enterprise server refuses everything until it is initialized. Initialize it right away,
before the server is reachable by anyone else: the first caller becomes the administrator.

```bash
trunks --registry https://trunks.example.com init-server
```

```text
Server initialized successfully
Admin access token: <token>
Saved to /home/you/.config/trunks/config.toml — this token is shown only once.
```

The token is stored in the CLI's configuration file together with the server address, so the
commands below need neither. There is one local administrator, `admin`. If its token is lost
and no other administrator exists, the only way back is to restore or reset the database.

For the server itself (database, TLS, public URL) see [Operations](operations.md).

## Users

```bash
trunks create user ana
```

```text
User ana created.
Access token: <token>
Log in with: trunks login <token>
This token is shown only once.
```

Give the token to the person. They store it on their own machine with:

```bash
trunks --registry https://trunks.example.com login <token>
```

| To | Run |
|---|---|
| List users | `trunks list users` |
| Replace a user's token (the old one stops working at once) | `trunks update user ana` |
| Remove a user | `trunks delete user ana` |

Users created this way have the role `user`. Removing a user also removes their sessions,
team memberships, grants and the MCP clients they own.

## Grants

A new user can use nothing. An administrator grants access to whole servers or to
[tool groups](tool-groups.md):

```bash
trunks grant --user ana --server github      # every tool, prompt and resource of one server
trunks grant --user ana --group triage       # only the tools of one group
trunks grant --user ana --server '*'         # every server, including future ones
trunks revoke --user ana --server github
trunks list grants
```

Grants are stored by name. A grant on a server that does not exist yet, or that is removed and
registered again, applies as soon as a server of that name exists.

## Teams

Teams let you grant once for several people:

```bash
trunks create team platform --description "Platform team"
trunks add-team-member platform ana --role maintainer
trunks add-team-member platform rui
trunks grant --team platform --server github
trunks list teams
```

A user can use everything granted to them directly plus everything granted to their teams.

| Team role | Can |
|---|---|
| `member` | use what the team was granted |
| `maintainer` | the same, and add or remove team members and manage the team's MCP clients |

Deleting a team removes its memberships, its grants and **the MCP clients it owns**; their
tokens stop working at once.

## MCP clients

Agents and MCP client applications authenticate on the MCP endpoints with the token of an
**MCP client**, never with a user's token.

```bash
trunks create mcp-client ci-agent --owner team:platform --allow github --description "CI agent"
```

```text
MCP client ci-agent created.
Owner: team platform
Access token: <token>
Allowed servers: github
This token is shown only once.
```

What a client can reach depends on its owner and its allow list:

| Owner | Allow list | The client can use |
|---|---|---|
| user or team | empty, or `*` | everything its owner can use, tool groups included |
| user or team | server names | those servers, as far as the owner can use them; nothing through the owner's tool groups |
| none | server names | those servers |
| none | `*` | every server |
| none | empty | nothing |

An owned client follows its owner: revoke a grant from the owner and the client loses it too.

Who can create which client:

- A user creates clients owned by themselves (the default) or, with `--owner team:<name>`, by a
  team they maintain.
- An administrator can set any owner, or none.

| To | Run |
|---|---|
| List clients (your own and your teams'; all, for an administrator) | `trunks list mcp-clients` |
| Change the allow list | `trunks update mcp-client ci-agent --allow github,jira` |
| Replace the token | `trunks update mcp-client ci-agent --rotate-token` |
| Remove a client | `trunks delete mcp-client ci-agent` |

Use one client per agent or per person's application, so that a token can be replaced or
removed without affecting the others. Configure the token in the application as described in
[Connecting clients](clients.md).

## Who can do what

| Action | `user` | team `maintainer` | `admin` |
|---|---|---|---|
| List and call what they were granted (CLI, REST API) | yes | yes | everything |
| Create and manage their own MCP clients | yes | yes | yes |
| Manage a team's members and MCP clients | no | their teams | yes |
| Register, change, enable, disable, remove servers | no | no | yes |
| Create, change, remove tool groups | no | no | yes |
| Create and remove users and teams; grant and revoke | no | no | yes |

Check your own identity and access with the REST API:

```bash
curl -s https://trunks.example.com/api/v0/users/whoami -H "Authorization: Bearer $TOKEN"
```

```json
{"username":"ana","role":"user","teams":[{"name":"platform","role":"maintainer"}],
 "access":{"all":false,"servers":["github"],"tool_groups":[]},
 "grants":[{"subject_kind":"team","subject":"platform","target_kind":"server","target":"github"}]}
```

`grants` lists where the access comes from: your own grants and those of your teams.

## In the web console

Everything in this chapter can also be done in the console, under **Access** in the navigation:

| Page | Who | Does |
|---|---|---|
| **MCP clients** | every user | lists your clients and your teams' (all, for an administrator); creates one, replaces its token, deletes it |
| **Users** | administrators | adds a user, replaces a user's token, deletes a user |
| **Teams** | every user reads; administrators create and delete; maintainers manage their team's members | members and their roles |
| **Access** | administrators | adds and revokes grants |
| **Account** (in the menu with your name) | every user | what you can use and through which grant; sign out |

A token is shown once, when it is created or replaced, in a dialog that only closes with
"Done". The console keeps it nowhere.

Two things are not in the console: changing a client's allow list
(`trunks update mcp-client <name> --allow …`) and changing a team's description (API only).

## Single sign-on (OIDC)

Trunks can sign people in through an OpenID Connect provider. This is a browser sign-in: the
console's sign-in page shows "Sign in with SSO" next to the access-token field, and a
successful sign-in opens a session cookie. The CLI and MCP clients keep using access tokens.

At the identity provider, register a client with the redirect URI
`<public URL>/auth/callback`. Then start Trunks with:

```bash
TRUNKS_MODE=enterprise
TRUNKS_PUBLIC_URL=https://trunks.example.com
TRUNKS_OIDC_ISSUER=https://idp.example.com/realms/main
TRUNKS_OIDC_CLIENT_ID=trunks
TRUNKS_OIDC_CLIENT_SECRET=...            # omit for a public client; PKCE is always used
```

How identities are handled:

- A person is identified by the provider's issuer and subject. The first sign-in creates a
  Trunks user named after `preferred_username`, else `email`, else the subject.
- A provider identity is never attached to an existing local user of the same name; that
  sign-in is refused. This keeps a provider account called `admin` from taking over the local
  administrator.
- `TRUNKS_OIDC_ADMIN_GROUP=<group>`: members of that group become administrators and others
  are made plain users, on every sign-in. The last administrator is never demoted. Without
  this setting, sign-in never changes a role.
- `TRUNKS_OIDC_SYNC_TEAMS=true`: groups become teams (created when missing) and the user's
  memberships follow the provider on every sign-in. `TRUNKS_OIDC_TEAM_PREFIX=trunks-` limits
  this to groups with that prefix and drops the prefix from the team name. Memberships added
  by hand are never touched.
- Groups are read from the `groups` claim of the ID token, or of the userinfo endpoint when
  the ID token has none (`TRUNKS_OIDC_GROUPS_CLAIM` changes the claim name). If the claim is
  absent, roles and teams are left as they are.
- **Grants are always managed in Trunks**, whatever the provider says.

All settings are listed in the [configuration reference](reference/configuration.md#single-sign-on-oidc).
