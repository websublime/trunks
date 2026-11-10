//! The version this binary reports: by `trunks version`, `/metadata` and the
//! `trunks_build_info` metric.

/// `v<package version>` for a build of the release tag, `v<package version>+<commit>` (or
/// `…+<commit>.dirty`) for any other build from a git checkout.
pub fn build_version() -> String {
    with_metadata(&trunks_types::version::version(), env!("TRUNKS_BUILD_METADATA"))
}

fn with_metadata(version: &str, metadata: &str) -> String {
    if metadata.is_empty() { version.to_owned() } else { format!("{version}+{metadata}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_metadata_is_appended_when_present() {
        let cases = [
            ("v1.2.3", "", "v1.2.3"),
            ("v1.2.3", "a5ee490", "v1.2.3+a5ee490"),
            ("v1.2.3", "a5ee490.dirty", "v1.2.3+a5ee490.dirty"),
        ];
        for (version, metadata, expected) in cases {
            assert_eq!(with_metadata(version, metadata), expected);
        }
    }

    #[test]
    fn the_reported_version_starts_with_the_package_version() {
        let reported = build_version();
        let package = trunks_types::version::version();
        assert!(reported == package || reported.starts_with(&format!("{package}+")), "{reported}");
    }
}
