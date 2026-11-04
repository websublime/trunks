//! The JSON error envelope returned by every REST endpoint.

use serde::{Deserialize, Serialize};

/// Machine-readable code attached to `InvalidInput` errors that require the
/// caller to run the upstream OAuth flow. Consumed by the CLI.
pub const CODE_UPSTREAM_OAUTH_REQUIRED: &str = "upstream_oauth_required";

/// Error messages that clients and tests depend on. Keep them byte-identical to `MCPJungle`.
pub mod messages {
    pub const SERVER_NOT_INITIALIZED: &str = "server is not initialized";
    pub const SERVER_ALREADY_INITIALIZED: &str = "Server is already initialized";
    pub const MISSING_ACCESS_TOKEN: &str = "missing access token";
    pub const USER_NOT_AUTHORIZED: &str = "user is not authorized to perform this action";
    pub const ENTERPRISE_ONLY: &str = "this request is only allowed in enterprise mode";
    pub const MISSING_MCP_CLIENT_TOKEN: &str = "missing MCP client access token";
    pub const INVALID_MCP_CLIENT_TOKEN: &str = "invalid MCP client token";
    pub const NAME_REQUIRED: &str = "name is required";
    pub const MISSING_NAME_QUERY: &str = "missing 'name' query parameter";
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
