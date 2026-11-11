//! The web app: static files of the embedded build (`docs/08-web-app.md` §8).
//!
//! The files are shells without data, the same for everyone, so they are public and served
//! even before the server is initialized (the app itself shows that state). Everything the
//! app displays comes from the API, with the viewer's own session.

use std::sync::Arc;

use axum::body::Body;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, ETAG, IF_NONE_MATCH, REFERRER_POLICY,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderValue, Method, Request, Response, StatusCode};
use trunks_dashboard::{Asset, Resolved, Source};

/// Paths that belong to the server: an unknown one there is an API "not found", never a page.
const SERVER_PREFIXES: [&str; 8] =
    ["api", "auth", "mcp", "v0", "health", "metadata", "metrics", "init"];

const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const REVALIDATE: &str = "no-cache";

/// What a page of the console may do: load its own scripts, styles, images and fonts, talk
/// to this server, and nothing else. No inline script or style, no framing, no plugins.
/// The build is checked against this (`web/app/scripts/check-dist.mjs`).
pub const CONTENT_SECURITY_POLICY_VALUE: &str = "default-src 'self'; script-src 'self'; \
     style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; \
     object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'";

/// Answers a request no route matched.
pub fn serve<B>(source: &Arc<dyn Source>, req: &Request<B>) -> Response<Body> {
    let path = req.uri().path();
    let first = path.trim_start_matches('/').split('/').next().unwrap_or_default();
    if !matches!(*req.method(), Method::GET | Method::HEAD) || SERVER_PREFIXES.contains(&first) {
        return plain(StatusCode::NOT_FOUND);
    }
    let head = *req.method() == Method::HEAD;
    match trunks_dashboard::resolve(source.as_ref(), path) {
        Resolved::Found(asset) => {
            let tag = format!("\"{}\"", asset.etag);
            let fresh = req
                .headers()
                .get(IF_NONE_MATCH)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|sent| sent.split(',').any(|t| t.trim() == tag));
            if fresh {
                return file(StatusCode::NOT_MODIFIED, &asset, &tag, true);
            }
            file(StatusCode::OK, &asset, &tag, head)
        }
        Resolved::NotFound(Some(page)) => {
            let tag = format!("\"{}\"", page.etag);
            file(StatusCode::NOT_FOUND, &page, &tag, head)
        }
        Resolved::NotFound(None) => plain(StatusCode::NOT_FOUND),
    }
}

fn plain(status: StatusCode) -> Response<Body> {
    let mut res = Response::new(Body::empty());
    *res.status_mut() = status;
    res
}

fn file(status: StatusCode, asset: &Asset, tag: &str, no_body: bool) -> Response<Body> {
    let body = if no_body { Body::empty() } else { Body::from(asset.bytes.clone().into_owned()) };
    let mut res = Response::new(body);
    *res.status_mut() = status;
    let headers = res.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(asset.content_type));
    let cache =
        if asset.immutable && status != StatusCode::NOT_FOUND { IMMUTABLE } else { REVALIDATE };
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(cache));
    if let Ok(tag) = HeaderValue::from_str(tag) {
        headers.insert(ETAG, tag);
    }
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    headers
        .insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(CONTENT_SECURITY_POLICY_VALUE));
    res
}
