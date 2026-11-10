//! The JSON error envelope returned by every REST endpoint.

use serde::{Deserialize, Serialize};

/// Machine-readable code attached to `InvalidInput` errors that require the
/// caller to run the upstream OAuth flow. Consumed by the CLI.
pub const CODE_UPSTREAM_OAUTH_REQUIRED: &str = "upstream_oauth_required";

/// Error messages that clients and tests depend on. They are part of the API contract.
pub mod messages {
    pub const SERVER_NOT_INITIALIZED: &str = "server is not initialized";
    pub const REQUEST_TIMED_OUT: &str = "request timed out";
    pub const REQUEST_BODY_TOO_LARGE: &str = "request body too large";
    pub const SERVER_ALREADY_INITIALIZED: &str = "Server is already initialized";
    pub const MISSING_ACCESS_TOKEN: &str = "missing access token";
    pub const USER_NOT_AUTHORIZED: &str = "user is not authorized to perform this action";
    pub const ENTERPRISE_ONLY: &str = "this request is only allowed in enterprise mode";
    pub const MISSING_MCP_CLIENT_TOKEN: &str = "missing MCP client access token";
    pub const INVALID_MCP_CLIENT_TOKEN: &str = "invalid MCP client token";
    pub const NAME_REQUIRED: &str = "name is required";
    pub const INVALID_ACCESS_TOKEN: &str = "invalid access token";
    pub const INIT_DEV_OK: &str = "Server initialized successfully in development mode";
    pub const INIT_ENTERPRISE_OK: &str = "Server initialized successfully";
    pub const MISSING_NAME_QUERY: &str = "missing 'name' query parameter";
    pub const MISSING_ENTITY_QUERY: &str = "missing 'entity' query parameter";
    pub const INVALID_FORCE_QUERY: &str = "invalid 'force' query parameter: expected true or false";
    pub const SSE_NOT_SUPPORTED: &str =
        "the 'sse' transport is not supported; register the server as streamable_http";
    pub const OAUTH_NOT_SUPPORTED_YET: &str = "upstream OAuth registration is not supported yet; provide a bearer_token or headers instead";
    pub const URL_REQUIRED: &str = "url is required for the streamable_http transport";
    pub const COMMAND_REQUIRED: &str = "command is required for the stdio transport";
    pub const USERNAME_REQUIRED: &str = "username is required";
    pub const NOT_OWNER: &str = "you do not own this MCP client";
    pub const CROSS_ORIGIN_REJECTED: &str =
        "cross-origin request rejected: session-authenticated changes must come from this site";
    pub const OIDC_NOT_CONFIGURED: &str = "OIDC login is not configured on this server";
    pub const INVALID_SUBJECT_KIND: &str = "subject_kind must be 'user' or 'team'";
    pub const INVALID_TARGET_KIND: &str = "target_kind must be 'server' or 'tool_group'";
    pub const INVALID_TEAM_ROLE: &str = "role must be 'maintainer' or 'member'";
    pub const GROUP_NO_TOOLS: &str = "the tool group resolves to no enabled tools; include at least one existing, enabled tool or server";
    pub const ADMIN_CANNOT_BE_DELETED: &str = "the last administrator cannot be deleted";
    pub const INVALID_TOKEN_FORMAT: &str =
        "access token must be at least 8 characters long and contain no whitespace";
}

/// `{"error": "...", "code": "..."}`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl ApiErrorResponse {
    #[must_use]
    pub fn new(error: impl Into<String>) -> Self {
        Self { error: error.into(), code: None }
    }

    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}
