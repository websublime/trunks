// Generated from the Rust API types (trunks-types, trunks-core::dashboard).
// Do not edit by hand: cargo xtask gen-types

/** Any JSON value: schemas, annotations and other content the gateway passes through. */
export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export type ServerName = string;

export type CanonicalName = string;

export type ResourceUri = string;

export type GroupName = string;

export type Transport = "stdio" | "streamable_http" | "sse";

export type SessionMode = "stateless" | "stateful";

export type Role = "admin" | "user";

export type ServerMode = "development" | "enterprise";

export type ApiErrorResponse = { error: string, code?: string | null, };

export type Health = { status: string, };

export type ServerMetadata = { version: string, };

export type AuthConfig = { 
/**
 * `None` until the server is initialized.
 */
mode: ServerMode | null, 
/**
 * Whether any login is needed (enterprise mode).
 */
login_required: boolean, 
/**
 * Whether `GET /auth/login` is available.
 */
oidc: boolean, };

export type InitRequest = { mode: ServerMode, };

export type InitResponse = { status: string, admin_access_token?: string | null, };

export type McpServer = { name: ServerName, transport: Transport, enabled: boolean, description: string, session_mode: SessionMode, url?: string | null, header_keys?: Array<string> | null, command?: string | null, args?: Array<string> | null, env_keys?: Array<string> | null, created_at?: string | null, updated_at?: string | null, };

export type Tool = { name: CanonicalName, server: ServerName, enabled: boolean, description: string, input_schema?: JsonValue | null, annotations?: JsonValue | null, };

export type Prompt = { name: CanonicalName, server: ServerName, enabled: boolean, description: string, arguments?: JsonValue | null, };

export type Resource = { uri: ResourceUri, server: ServerName, name: string, enabled: boolean, description: string, mime_type?: string | null, annotations?: JsonValue | null, meta?: JsonValue | null, };

export type RegisterServerInput = { name: string, transport: Transport, description: string, url?: string | null, bearer_token?: string | null, headers?: { [key in string]: string } | null, command?: string | null, args?: Array<string> | null, env?: { [key in string]: string } | null, session_mode: SessionMode, oauth_redirect_uri?: string | null, oauth_client_id?: string | null, oauth_client_secret?: string | null, oauth_scopes?: Array<string> | null, };

export type RegisterServerResult = { server?: McpServer | null, authorization_required?: AuthorizationRequired | null, };

export type AuthorizationRequired = { session_id: string, authorization_url: string, expires_at: string, };

export type CompleteOAuthRequest = { code: string, state: string, };

export type EnableDisableResult = { name?: string | null, tools_affected?: Array<string> | null, prompts_affected?: Array<string> | null, resources_affected?: Array<string> | null, };

export type ToolGroup = { name: string, description: string, 
/**
 * Canonical tool names.
 */
included_tools: Array<string>, 
/**
 * Server names: every enabled tool of each server.
 */
included_servers: Array<string>, 
/**
 * Canonical tool names removed from the result. Exclusion always wins.
 */
excluded_tools: Array<string>, };

export type ToolGroupWithEndpoints = { 
/**
 * Path of the group's MCP endpoint (`/v0/groups/{name}/mcp`), relative to the server.
 */
streamable_http_endpoint: string, name: string, description: string, 
/**
 * Canonical tool names.
 */
included_tools: Array<string>, 
/**
 * Server names: every enabled tool of each server.
 */
included_servers: Array<string>, 
/**
 * Canonical tool names removed from the result. Exclusion always wins.
 */
excluded_tools: Array<string>, };

export type UpdateToolGroupResponse = { name: string, old: ToolGroup, new: ToolGroup, };

export type User = { username: string, role: Role, };

export type CreateUserRequest = { username: string, 
/**
 * Custom token (≥ 8 chars, no whitespace); generated when omitted.
 */
access_token?: string | null, };

export type UpdateUserRequest = { access_token?: string | null, };

export type UserWithToken = { username: string, role: Role, access_token: string, };

export type Owner = { 
/**
 * `user` or `team`
 */
kind: string, name: string, };

export type McpClient = { name: string, description: string, 
/**
 * Server names this client may reach; `["*"]` means every server, `[]` none. With an
 * owner it narrows the owner's grants instead (empty = everything the owner can).
 */
allow_list: Array<string>, owner?: Owner | null, };

export type CreateMcpClientRequest = { name: string, description: string, allow_list: Array<string>, 
/**
 * Custom token (≥ 8 chars, no whitespace); generated when omitted.
 */
access_token?: string | null, owner?: Owner | null, };

export type UpdateMcpClientRequest = { description?: string | null, allow_list?: Array<string> | null, access_token?: string | null, rotate_token: boolean, };

export type McpClientWithToken = { name: string, description: string, allow_list: Array<string>, owner?: Owner | null, access_token?: string | null, };

export type Team = { name: string, description: string, };

export type TeamMember = { username: string, 
/**
 * `maintainer` or `member`
 */
role: string, };

export type TeamDetail = { members: Array<TeamMember>, name: string, description: string, };

export type AddTeamMemberRequest = { username: string, 
/**
 * `maintainer` or `member` (default)
 */
role?: string | null, };

export type GrantSpec = { 
/**
 * `user` or `team`
 */
subject_kind: string, subject: string, 
/**
 * `server` or `tool_group`
 */
target_kind: string, 
/**
 * A server name, a tool group name, or `*` (every server).
 */
target: string, };

export type AccessView = { all: boolean, servers: Array<string>, tool_groups: Array<string>, };

export type TeamMembership = { name: string, role: string, };

export type WhoAmI = { username: string, role: Role, teams: Array<TeamMembership>, access: AccessView, 
/**
 * Where `access` comes from: the grants held by the user and by each of their teams.
 * An administrator's access does not come from grants; theirs are listed all the same.
 */
grants: Array<GrantSpec>, };

export type Counts = { servers: number, servers_enabled: number, tools: number, tools_enabled: number, prompts: number, resources: number, tool_groups: number, pending_oauth: number, };

export type SessionStats = { stateful_active: number, };

export type Overview = { mode: ServerMode, version: string, uptime_seconds: number, 
/**
 * Of what the viewer may use.
 */
counts: Counts, 
/**
 * A fact about the whole process: present for administrators only.
 */
sessions?: SessionStats | null, metrics_enabled: boolean, };

export type ConfigSummary = { url?: string | null, header_keys?: Array<string> | null, 
/**
 * Truncated to 80 characters.
 */
command?: string | null, args?: Array<string> | null, env_keys?: Array<string> | null, };

export type OAuthStatus = "none" | "pending" | "authorized";

export type OAuthSummary = { status: OAuthStatus, session_id?: string | null, expires_at?: string | null, };

export type EntityCounts = { tools: number, tools_enabled: number, prompts: number, resources: number, };

export type ServerSummary = { name: ServerName, transport: Transport, enabled: boolean, description: string, session_mode: SessionMode, config_summary: ConfigSummary, counts: EntityCounts, oauth: OAuthSummary, last_error?: string | null, created_at?: string | null, updated_at?: string | null, };

export type ServerDetail = { tools: Array<Tool>, prompts: Array<Prompt>, resources: Array<Resource>, name: ServerName, transport: Transport, enabled: boolean, description: string, session_mode: SessionMode, config_summary: ConfigSummary, counts: EntityCounts, oauth: OAuthSummary, last_error?: string | null, created_at?: string | null, updated_at?: string | null, };

export type GroupSummary = { streamable_http_endpoint: string, effective_tools: Array<string>, 
/**
 * `true` when the definition resolves to no enabled tool.
 */
degraded: boolean, name: string, description: string, 
/**
 * Canonical tool names.
 */
included_tools: Array<string>, 
/**
 * Server names: every enabled tool of each server.
 */
included_servers: Array<string>, 
/**
 * Canonical tool names removed from the result. Exclusion always wins.
 */
excluded_tools: Array<string>, };

export type UnreachableServer = { name: ServerName, error: string, at?: string | null, };

export type PendingOAuth = { session_id: string, server_name: ServerName, expires_at: string, };

export type DatabaseInfo = { backend: string, url_redacted: string, migrations_latest?: number | null, };

export type BindInfo = { host: string, port: number, };

export type Diagnostics = { database: DatabaseInfo, bind: BindInfo, allowed_hosts: Array<string>, upstream_init_timeout_sec: number, session_idle_timeout_sec: number, metrics_enabled: boolean, servers_unreachable: Array<UnreachableServer>, pending_oauth: Array<PendingOAuth>, };

export type OAuthSessionState = "pending" | "expired" | "completed" | "unknown";

export type OAuthSessionStatus = { session_id: string, server_name?: ServerName | null, status: OAuthSessionState, expires_at?: string | null, };
