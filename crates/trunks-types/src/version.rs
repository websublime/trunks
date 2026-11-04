//! Build version as advertised by `/metadata` and the CLI.

/// The crate version, normalized to the `vX.Y.Z` form advertised by `/metadata`.
#[must_use]
pub fn version() -> String {
    normalize(env!("CARGO_PKG_VERSION"))
}

/// Prefixes a `v` when the string starts with a digit (`0.4.2` → `v0.4.2`).
#[must_use]
pub fn normalize(v: &str) -> String {
    let v = v.trim();
    if v.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("v{v}")
    } else {
        v.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_with_v_prefix() {
        assert_eq!(normalize("0.4.2"), "v0.4.2");
        assert_eq!(normalize("v0.4.2"), "v0.4.2");
        assert_eq!(normalize("dev"), "dev");
        assert!(version().starts_with('v'));
    }
}
