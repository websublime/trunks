//! Properties every name and URI parser must hold for **any** input. Shared by the seeded
//! random test in this crate (`tests/names_random.rs`, runs everywhere) and by the
//! coverage-guided fuzz targets in `fuzz/` (nightly only).
//!
//! Each function panics when a property is violated; none of them may panic otherwise.
#![allow(dead_code)] // every consumer uses a subset

use trunks_types::{CanonicalName, GroupName, ResourceUri, ServerName};

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// An accepted server name is returned unchanged and obeys the documented rules.
pub fn server_name(input: &str) {
    if let Ok(name) = ServerName::parse(input) {
        assert_eq!(name.as_str(), input);
        assert_eq!(name.to_string(), input);
        assert!(!input.is_empty() && input.chars().all(is_name_char), "{input:?}");
        assert!(!input.contains("__") && !input.ends_with('_'), "{input:?}");
    }
}

/// An accepted group name is returned unchanged and obeys the documented rules.
pub fn group_name(input: &str) {
    if let Ok(name) = GroupName::parse(input) {
        assert_eq!(name.as_str(), input);
        assert!(input.chars().all(is_name_char), "{input:?}");
        assert!(input.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()), "{input:?}");
    }
}

/// Parsing and printing a canonical name are inverses, and the split is unambiguous.
pub fn canonical_name(input: &str) {
    if let Ok(name) = CanonicalName::parse(input) {
        assert_eq!(name.to_string(), input, "printing must reproduce the input");
        assert_eq!(format!("{}__{}", name.server(), name.local()), input);
        assert_eq!(CanonicalName::parse(&name.to_string()).as_ref(), Ok(&name));
        server_name(name.server().as_str());
        assert!(ServerName::parse(name.server().as_str()).is_ok());
    }
    // Any upstream entity name survives the round trip behind a valid server name.
    for server in ["s", "a-b_c", "srv9"] {
        let server = ServerName::parse(server).unwrap();
        let built = CanonicalName::new(server.clone(), input);
        let parsed = CanonicalName::parse(&built.to_string()).unwrap();
        assert_eq!(parsed, built, "{input:?}");
        assert_eq!((parsed.server(), parsed.local()), (&server, input));
    }
}

/// Parsing and printing a resource URI are inverses, and any non-empty original URI can be
/// carried (an empty one has no encoding; the registry skips such resources).
pub fn resource_uri(input: &str) {
    if let Ok(uri) = ResourceUri::parse(input) {
        assert!(input.starts_with("trunks://res/"), "{input:?}");
        assert_eq!(uri.to_string(), input, "printing must reproduce the input");
        assert_eq!(ResourceUri::parse(&uri.to_string()).as_ref(), Ok(&uri));
        server_name(uri.server().as_str());
    }
    let server = ServerName::parse("srv").unwrap();
    let built = ResourceUri::new(server.clone(), input);
    if input.is_empty() {
        assert!(ResourceUri::parse(&built.to_string()).is_err());
        return;
    }
    let parsed = ResourceUri::parse(&built.to_string()).unwrap();
    assert_eq!(parsed, built, "{input:?}");
    assert_eq!((parsed.server(), parsed.original()), (&server, input));
}
