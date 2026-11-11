//! How the web app's static files are served: which paths are pages, which belong to the
//! server, and the caching and security headers (`docs/08-web-app.md` §8).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::to_bytes;
use axum::http::{Method, Request, StatusCode};
use trunks_api::routes::web::serve;
use trunks_dashboard::Source;

struct Build(BTreeMap<&'static str, &'static str>);

impl Source for Build {
    fn file(&self, path: &str) -> Option<(Cow<'static, [u8]>, String)> {
        let body = *self.0.get(path)?;
        Some((Cow::Borrowed(body.as_bytes()), format!("v1-{}", path.len())))
    }
}

fn build() -> Arc<dyn Source> {
    Arc::new(Build(BTreeMap::from([
        ("index.html", "<h1>Overview</h1>"),
        ("404.html", "<h1>Page not found</h1>"),
        ("servers/index.html", "<h1>Servers</h1>"),
        ("_astro/app.B1x9.js", "console.log(1)"),
    ])))
}

async fn get(
    source: &Arc<dyn Source>,
    method: Method,
    path: &str,
    etag: Option<&str>,
) -> (StatusCode, BTreeMap<String, String>, String) {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(etag) = etag {
        req = req.header("if-none-match", etag);
    }
    let res = serve(source, &req.body(()).unwrap());
    let status = res.status();
    let headers = res
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap().to_owned()))
        .collect();
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, headers, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn pages_and_files_are_served_with_the_right_caching_and_security_headers() {
    let web = build();

    let (status, headers, body) = get(&web, Method::GET, "/servers/", None).await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "<h1>Servers</h1>"));
    assert_eq!(headers["content-type"], "text/html; charset=utf-8");
    assert_eq!(headers["cache-control"], "no-cache", "pages are revalidated");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["referrer-policy"], "same-origin");
    let csp = &headers["content-security-policy"];
    for directive in [
        "default-src 'self'",
        "script-src 'self'",
        "style-src 'self'",
        "connect-src 'self'",
        "object-src 'none'",
        "frame-ancestors 'none'",
    ] {
        assert!(csp.split("; ").any(|d| d == directive), "{directive} missing from {csp}");
    }
    assert!(!csp.contains("unsafe"), "nothing inline, nothing evaluated: {csp}");

    let (status, headers, _) = get(&web, Method::GET, "/_astro/app.B1x9.js", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "public, max-age=31536000, immutable");
    assert_eq!(headers["content-type"], "text/javascript; charset=utf-8");

    // revalidation: the tag the server gave is enough to skip the body
    let (_, headers, _) = get(&web, Method::GET, "/", None).await;
    let tag = headers["etag"].clone();
    let (status, headers, body) = get(&web, Method::GET, "/", Some(&tag)).await;
    assert_eq!((status, body.as_str()), (StatusCode::NOT_MODIFIED, ""));
    assert_eq!(headers["etag"], tag);
    let (status, _, body) = get(&web, Method::GET, "/", Some("\"something-else\"")).await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "<h1>Overview</h1>"));

    // HEAD: the headers of GET, no body
    let (status, headers, body) = get(&web, Method::HEAD, "/servers", None).await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, ""));
    assert_eq!(headers["content-type"], "text/html; charset=utf-8");
}

#[tokio::test]
async fn an_unknown_page_gets_the_apps_not_found_page_and_a_server_path_never_gets_a_page() {
    let web = build();

    let (status, headers, body) = get(&web, Method::GET, "/servers/github", None).await;
    assert_eq!((status, body.as_str()), (StatusCode::NOT_FOUND, "<h1>Page not found</h1>"));
    assert_eq!(headers["cache-control"], "no-cache");

    // Paths of the API, the MCP endpoints and the service endpoints: a bare 404, so a client
    // that mistypes a route does not receive HTML.
    for path in [
        "/api/v0/nope",
        "/api",
        "/auth/nope",
        "/mcp/extra",
        "/v0/groups",
        "/health/x",
        "/metadata/x",
        "/metrics",
        "/init/x",
    ] {
        let (status, headers, body) = get(&web, Method::GET, path, None).await;
        assert_eq!((status, body.as_str()), (StatusCode::NOT_FOUND, ""), "{path}");
        assert!(!headers.contains_key("content-type"), "{path}");
    }

    // The web app is read with GET and HEAD only.
    for method in [Method::POST, Method::PUT, Method::DELETE, Method::PATCH] {
        let (status, _, body) = get(&web, method.clone(), "/servers/", None).await;
        assert_eq!((status, body.as_str()), (StatusCode::NOT_FOUND, ""), "{method}");
    }

    // A server built without the web app has no pages at all.
    let empty: Arc<dyn Source> = Arc::new(Build(BTreeMap::new()));
    let (status, _, body) = get(&empty, Method::GET, "/", None).await;
    assert_eq!((status, body.as_str()), (StatusCode::NOT_FOUND, ""));
}
