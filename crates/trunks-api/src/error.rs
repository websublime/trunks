//! Maps domain errors to the `{"error","code"}` envelope and HTTP status codes.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use trunks_core::Error;
use trunks_types::ApiErrorResponse;
use trunks_types::error::{CODE_UPSTREAM_OAUTH_REQUIRED, messages};

/// JSON body extractor whose rejection is our `{"error": …}` envelope instead of axum's text:
/// 413 when the body exceeds the limit, 400 for anything else (axum would answer 415/422).
#[derive(Debug, Clone, Copy, Default)]
pub struct JsonBody<T>(pub T);

impl<S, T> axum::extract::FromRequest<S> for JsonBody<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = Response;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(Self(v)),
            Err(rej) if rej.status() == StatusCode::PAYLOAD_TOO_LARGE => Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(ApiErrorResponse::new(messages::REQUEST_BODY_TOO_LARGE)),
            )
                .into_response()),
            Err(rej) => Err(ApiError(Error::invalid_input(rej.body_text())).into_response()),
        }
    }
}

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

/// Domain error → HTTP status mapping (see `docs/02-especificacao.md` §5).
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
        Error::UpstreamTimeout { .. } => (StatusCode::GATEWAY_TIMEOUT, ApiErrorResponse::new(msg)),
        Error::Upstream { .. }
        | Error::UpstreamAuthRequired { .. }
        | Error::Store { .. }
        | Error::Internal { .. } => (StatusCode::INTERNAL_SERVER_ERROR, ApiErrorResponse::new(msg)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_domain_errors_to_status_codes() {
        let (s, b) = classify(&Error::not_found("tool group"));
        assert_eq!(s, StatusCode::NOT_FOUND);
        assert!(b.error.contains("not found"));

        let (s, b) = classify(&Error::UpstreamOAuthRequired { reason: "x".into() });
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(b.code.as_deref(), Some("upstream_oauth_required"));

        let (s, _) = classify(&Error::Internal { reason: "boom".into() });
        assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR);

        let (s, b) = classify(&Error::UpstreamTimeout { server: "fs".into(), seconds: 300 });
        assert_eq!(s, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(
            b.error,
            "upstream 'fs' did not answer within 300s (TRUNKS_REQUEST_TIMEOUT_SEC)"
        );
    }
}
