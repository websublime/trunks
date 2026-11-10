//! Records which commit a build comes from, as `TRUNKS_BUILD_METADATA`:
//!
//! - empty when the commit is exactly the release tag `v<package version>`, or when there is
//!   no git checkout (a source archive, a container build);
//! - otherwise the abbreviated commit, plus `.dirty` when the working tree has changes.
//!
//! The version shown to users is `v<package version>` followed by `+<metadata>` when it is
//! not empty (`src/version.rs`).

use std::path::Path;
use std::process::Command;

/// The trimmed standard output of a git command, when it succeeds.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn metadata() -> String {
    let Some(commit) = git(&["rev-parse", "--short=7", "HEAD"]).filter(|c| !c.is_empty()) else {
        return String::new();
    };
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|changes| !changes.is_empty());
    let release_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    let at_release_tag = git(&["tag", "--points-at", "HEAD"])
        .is_some_and(|tags| tags.lines().any(|tag| tag == release_tag));
    match (at_release_tag, dirty) {
        (true, false) => String::new(),
        (_, false) => commit,
        (_, true) => format!("{commit}.dirty"),
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Re-run when the checked-out commit or the index changes — only if there is a checkout,
    // because a path that does not exist would make the script run on every build.
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        for file in ["HEAD", "index", "packed-refs"] {
            let path = Path::new(&git_dir).join(file);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
        // The branch HEAD points at moves on every commit.
        if let Some(reference) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
            let path = Path::new(&git_dir).join(reference);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    println!("cargo:rustc-env=TRUNKS_BUILD_METADATA={}", metadata());
}
