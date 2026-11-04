//! Maps domain errors to the `{"error","code"}` envelope and HTTP status codes.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use trunks_core::Error;
use trunks_types::ApiErrorResponse;
use trunks_types::error::CODE_UPSTREAM_OAUTH_REQUIRED;

/// Newtype so `trunks-api` can implement `IntoResponse` for the domain error.
#[derive(Debug)]
pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = classify(&self.0);
        (status, Json(body)).into_response()
    }
}

/// The `MCPJungle` `handleServiceError` mapping.
#[must_use]
pub fn classify(e: &Error) -> (StatusCode, ApiErrorResponse) {
    let msg = e.to_string();
    match e {
        Error::NotFound { .. } => (StatusCode::NOT_FOUND, ApiErrorResponse::new(msg)),
        Error::InvalidInput { .. } => (StatusCode::BAD_REQUEST, ApiErrorResponse::new(msg)),
        Error::UpstreamOAuthRequired { .. } => (
            StatusCode::BAD_REQUEST,
            ApiErrorResponse::new(msg).with_code(CODE_UPSTREAM_OAUTH_REQUIRED),
        ),
        Error::Unauthorized { .. } => (StatusCode::UNAUTHORIZED, ApiErrorResponse::new(msg)),
        Error::Forbidden { .. } | Error::NotInitialized => {
            (StatusCode::FORBIDDEN, ApiErrorResponse::new(msg))
        }
        Error::Upstream { .. } | Error::Store { .. } | Error::Internal { .. } => {
            (StatusCode::INTERNAL_SERVER_ERROR, ApiErrorResponse::new(msg))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_like_mcpjungle() {
        let (s, b) = classify(&Error::not_found("tool group"));
        assert_eq!(s, StatusCode::NOT_FOUND);
        assert!(b.error.contains("not found"));

        let (s, b) = classify(&Error::UpstreamOAuthRequired { reason: "x".into() });
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(b.code.as_deref(), Some("upstream_oauth_required"));

        let (s, _) = classify(&Error::Internal { reason: "boom".into() });
        assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
