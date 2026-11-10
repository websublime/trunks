//! Domain error type.
//!
//! Variants classify failures the way the HTTP layer needs to map them to
//! status codes; nothing here knows about HTTP.

use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    /// Maps to 404.
    #[snafu(display("{what} not found"))]
    NotFound { what: String },

    /// Maps to 400. The reason is shown verbatim (clients match on it).
    #[snafu(display("{reason}"))]
    InvalidInput { reason: String },

    /// Maps to 400 with `code = upstream_oauth_required`.
    #[snafu(display("upstream OAuth authorization required: {reason}"))]
    UpstreamOAuthRequired { reason: String },

    /// Maps to 401.
    #[snafu(display("{reason}"))]
    Unauthorized { reason: String },

    /// Maps to 403.
    #[snafu(display("{reason}"))]
    Forbidden { reason: String },

    /// Maps to 403 (`server is not initialized`).
    #[snafu(display("server is not initialized"))]
    NotInitialized,

    /// Failure talking to an upstream MCP server. Maps to 500.
    #[snafu(display("upstream '{server}': {reason}"))]
    Upstream { server: String, reason: String },

    /// The upstream answered 401 with a `WWW-Authenticate` challenge: the registry turns this
    /// into the OAuth bootstrap (or a 400 `upstream_oauth_required` when it cannot). Maps to
    /// 500 if it ever escapes.
    /// The upstream did not answer a request within the configured limit.
    #[snafu(display(
        "upstream '{server}' did not answer within {seconds}s (TRUNKS_REQUEST_TIMEOUT_SEC)"
    ))]
    UpstreamTimeout { server: String, seconds: u64 },

    #[snafu(display("upstream '{server}' requires authorization"))]
    UpstreamAuthRequired { server: String, www_authenticate: String },

    /// Failure in the persistence layer. Maps to 500.
    #[snafu(display("store error: {source}"))]
    Store { source: Box<dyn std::error::Error + Send + Sync + 'static> },

    /// Anything else. Maps to 500.
    #[snafu(display("{reason}"))]
    Internal { reason: String },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    /// Convenience constructor used by the ports' implementations.
    pub fn store(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Store { source: Box::new(source) }
    }

    #[must_use]
    pub fn not_found(what: impl Into<String>) -> Self {
        Self::NotFound { what: what.into() }
    }

    #[must_use]
    pub fn invalid_input(reason: impl Into<String>) -> Self {
        Self::InvalidInput { reason: reason.into() }
    }
}

impl From<trunks_types::NameError> for Error {
    fn from(e: trunks_types::NameError) -> Self {
        Self::InvalidInput { reason: e.to_string() }
    }
}
