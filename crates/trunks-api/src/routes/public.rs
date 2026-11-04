//! Routes that need neither initialization nor authentication.

use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct Metadata {
    version: String,
}

pub fn router() -> Router {
    Router::new().route("/health", get(health)).route("/metadata", get(metadata))
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

async fn metadata() -> Json<Metadata> {
    Json(Metadata { version: trunks_types::version::version() })
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    #[tokio::test]
    async fn health_and_metadata() {
        let app = super::router();
        let res = app
            .clone()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], br#"{"status":"ok"}"#);

        let res =
            app.oneshot(Request::get("/metadata").body(Body::empty()).unwrap()).await.unwrap();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["version"].as_str().unwrap().starts_with('v'));
    }
}
