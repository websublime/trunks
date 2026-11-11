#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! The Trunks web app (`web/app`, a static Astro build) embedded into the binary, and the
//! rules that turn a request path into one of its files.
//!
//! The `dist/` folder is produced by `cargo xtask build-dashboard`; an empty folder still
//! compiles, so server builds never depend on Node being present.

use std::borrow::Cow;

use rust_embed::Embed;

#[derive(Embed, Debug)]
#[folder = "dist/"]
#[exclude = ".gitkeep"]
pub struct Assets;

impl Assets {
    /// True when a real build (not just the placeholder) was embedded.
    #[must_use]
    pub fn is_built() -> bool {
        Self::get("index.html").is_some()
    }
}

/// Where the files of the web app come from.
pub trait Source: Send + Sync {
    /// The file at `path` (no leading slash), with a tag that changes when its content does.
    fn file(&self, path: &str) -> Option<(Cow<'static, [u8]>, String)>;
}

/// The build embedded in the binary.
#[derive(Debug, Clone, Copy, Default)]
pub struct Embedded;

impl Source for Embedded {
    fn file(&self, path: &str) -> Option<(Cow<'static, [u8]>, String)> {
        let file = Assets::get(path)?;
        let tag = file.metadata.sha256_hash().iter().take(12).fold(String::new(), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        });
        Some((file.data, tag))
    }
}

/// One file of the web app, ready to be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub bytes: Cow<'static, [u8]>,
    pub content_type: &'static str,
    /// Changes when the content changes.
    pub etag: String,
    /// The file name carries a content hash, so the file can be cached forever.
    pub immutable: bool,
}

/// What a request path resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    Found(Asset),
    /// No such page. Carries the app's own "not found" page when the build has one.
    NotFound(Option<Asset>),
}

/// Folder of the files Astro names after their content.
const HASHED: &str = "_astro/";
const NOT_FOUND_PAGE: &str = "404.html";

/// Resolves a request path against the build: the exact file, then `<path>/index.html` (each
/// page is a folder), then the "not found" page. A path that tries to leave the build
/// (`..`, a backslash, an empty segment in the middle) resolves to nothing.
#[must_use]
pub fn resolve(source: &dyn Source, request_path: &str) -> Resolved {
    let asset = |path: &str| {
        source.file(path).map(|(bytes, etag)| Asset {
            bytes,
            content_type: content_type(path),
            etag,
            immutable: path.starts_with(HASHED),
        })
    };
    let Some(path) = clean(request_path) else {
        return Resolved::NotFound(asset(NOT_FOUND_PAGE));
    };
    let found = if path.is_empty() {
        asset("index.html")
    } else if request_path.ends_with('/') {
        asset(&format!("{path}/index.html"))
    } else {
        asset(&path).or_else(|| asset(&format!("{path}/index.html")))
    };
    match found {
        Some(asset) => Resolved::Found(asset),
        None => Resolved::NotFound(asset(NOT_FOUND_PAGE)),
    }
}

/// The path without its outer slashes, or `None` when a segment is not a plain name.
fn clean(request_path: &str) -> Option<String> {
    let path = request_path.trim_matches('/');
    let plain = |segment: &str| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && !segment.contains(['\\', '\0', '%'])
    };
    (path.is_empty() || path.split('/').all(plain)).then(|| path.to_owned())
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map_or("", |(_, extension)| extension) {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "webmanifest" => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    struct Files(BTreeMap<&'static str, &'static str>);

    impl Source for Files {
        fn file(&self, path: &str) -> Option<(Cow<'static, [u8]>, String)> {
            let body = *self.0.get(path)?;
            Some((Cow::Borrowed(body.as_bytes()), format!("tag-{path}")))
        }
    }

    fn build() -> Files {
        Files(BTreeMap::from([
            ("index.html", "overview"),
            ("404.html", "not found"),
            ("servers/index.html", "servers"),
            ("servers/detail/index.html", "server detail"),
            ("favicon.svg", "<svg/>"),
            ("_astro/app.B1x9.js", "js"),
            ("_astro/app.Qz81.css", "css"),
        ]))
    }

    fn body(resolved: &Resolved) -> (&'static str, String) {
        match resolved {
            Resolved::Found(a) => ("found", String::from_utf8_lossy(&a.bytes).into_owned()),
            Resolved::NotFound(Some(a)) => {
                ("not found", String::from_utf8_lossy(&a.bytes).into_owned())
            }
            Resolved::NotFound(None) => ("not found", String::new()),
        }
    }

    #[test]
    fn a_path_is_a_file_then_a_page_folder_then_the_not_found_page() {
        let cases = [
            ("/", "found", "overview"),
            ("", "found", "overview"),
            ("/index.html", "found", "overview"),
            ("/servers", "found", "servers"),
            ("/servers/", "found", "servers"),
            ("/servers/index.html", "found", "servers"),
            ("/servers/detail/", "found", "server detail"),
            ("/servers/detail", "found", "server detail"),
            ("/favicon.svg", "found", "<svg/>"),
            ("/_astro/app.B1x9.js", "found", "js"),
            // a file is not a folder
            ("/favicon.svg/", "not found", "not found"),
            ("/servers/github", "not found", "not found"),
            ("/nope", "not found", "not found"),
            ("/_astro/", "not found", "not found"),
            // nothing leaves the build
            ("/../Cargo.toml", "not found", "not found"),
            ("/servers/../index.html", "not found", "not found"),
            ("/servers//detail", "not found", "not found"),
            ("/%2e%2e/secret", "not found", "not found"),
            ("/servers\\detail", "not found", "not found"),
            ("/./index.html", "not found", "not found"),
        ];
        let build = build();
        for (path, kind, expected) in cases {
            let resolved = resolve(&build, path);
            assert_eq!(body(&resolved), (kind, expected.to_owned()), "{path}");
        }
    }

    #[test]
    fn only_content_hashed_files_are_immutable_and_types_follow_the_extension() {
        let build = build();
        let cases = [
            ("/", "text/html; charset=utf-8", false),
            ("/servers/", "text/html; charset=utf-8", false),
            ("/favicon.svg", "image/svg+xml", false),
            ("/_astro/app.B1x9.js", "text/javascript; charset=utf-8", true),
            ("/_astro/app.Qz81.css", "text/css; charset=utf-8", true),
        ];
        for (path, content_type, immutable) in cases {
            let Resolved::Found(asset) = resolve(&build, path) else { panic!("{path}") };
            assert_eq!((asset.content_type, asset.immutable), (content_type, immutable), "{path}");
            assert!(!asset.etag.is_empty());
        }
    }

    #[test]
    fn a_build_without_a_not_found_page_still_answers() {
        let build = Files(BTreeMap::from([("index.html", "overview")]));
        assert_eq!(resolve(&build, "/nope"), Resolved::NotFound(None));
        // and an empty build (server compiled without the web app) has nothing at all
        let empty = Files(BTreeMap::new());
        assert_eq!(resolve(&empty, "/"), Resolved::NotFound(None));
    }
}
