# Legacy-format fixtures

Databases in the on-disk format Trunks can open and normalise (decision D3). Used by the
D11 analysis, the `trunks-db` tests and the conformance suite.

| File | Mode | Contents |
|---|---|---|
| `legacy-dev.sqlite` | development | 4 servers (stdio stateful, http+bearer, http+custom `Authorization` header+OAuth token, **disabled `sse`**), 6 tools (one disabled, one whose own name contains `__`), 2 prompts (one with a space), 2 resources (`mcpj://` URIs, one disabled), 2 groups, 1 expired pending OAuth session |
| `legacy-enterprise.sqlite` | enterprise, stored as the legacy alias `production` | same + 2 users (admin, user) + 2 MCP clients (`["filesystem","context7"]`, `["*"]`) |
| `legacy-enterprise.sql` | enterprise | the same content as PostgreSQL DDL + INSERTs; `psql -f legacy-enterprise.sql` |

Edge cases on purpose: `session_mode` NULL on one server, `annotations` NULL vs `{}`, `included_tools` NULL,
URIs in the old `mcpj://` scheme, mode `production`, tokens in plaintext.

These files are **synthetic**: the schema reproduces the legacy ORM's auto-migration output column by
column. Regenerate them from a real deployment when one is available (internal generator script).
