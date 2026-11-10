# Tool groups

A tool group is a named selection of tools with its own MCP endpoint. Use one to give an agent
only the tools it needs, or to keep a client's tool list short.

## Define a group

A group is described by a JSON file:

```json
{
  "name": "triage",
  "description": "Read issues, post comments",
  "included_servers": ["jira"],
  "included_tools": ["github__list_issues", "github__add_issue_comment"],
  "excluded_tools": ["jira__delete_issue"]
}
```

| Field | Meaning |
|---|---|
| `name` | starts with a letter or digit; then letters, digits, `_`, `-` |
| `included_servers` | every tool of these servers, including tools they gain later |
| `included_tools` | individual tools, by full name (`<server>__<tool>`) |
| `excluded_tools` | removed from the result of the two lists above |

The tools a group actually exposes, its **effective tools**, are the included servers' tools
plus the included tools, minus the excluded ones, counting only tools that are currently
enabled.

```bash
trunks create group -c triage.json
```

The command prints the stored definition, the group's endpoint and its effective tools. For a
group of two tools of the [quickstart](quickstart.md)'s server:

```text
Tool group math created.
math
  Only the arithmetic tools
  included tools:   everything__echo, everything__get-sum
  included servers: (none)
  excluded tools:   (none)
  MCP endpoint:     http://127.0.0.1:8080/v0/groups/math/mcp
  effective tools (2):
    everything__echo
    everything__get-sum
```

Creation fails if an included tool or server does not exist, or if the group would have no
tools.

## Use a group

Point the MCP client at the group's endpoint instead of `/mcp`:

```text
http://127.0.0.1:8080/v0/groups/triage/mcp
```

The client sees and can call only the effective tools. A group endpoint lists no prompts and
no resources.

From the CLI:

```bash
trunks list groups
trunks get group triage
trunks list tools --group triage
trunks invoke github__list_issues --group triage --input '{"repo":"websublime/trunks"}'
```

## Change or remove a group

```bash
trunks update group triage -c triage.json   # prints what changed
trunks delete group triage
```

`update` replaces the whole definition with the file's contents; the `name` in the file is
ignored.

## When things a group refers to change

A group stores names, not copies. If a tool it includes is disabled, or a server it includes
is removed, the group keeps working with whatever is left, down to an empty tool list, and
recovers by itself when the tool or server is back.

## Groups and access (enterprise mode)

A group does not grant access by existing. An administrator grants it to a user or a team:

```bash
trunks grant --team support --group triage
```

Whoever holds that grant may use the group's effective tools, on the group endpoint and on
`/mcp`, without having access to the whole servers behind them. A grant on a group does not
give access to prompts or resources. See [Enterprise mode](enterprise.md).
