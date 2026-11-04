#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! The Trunks web app (`line://ui`, `web/app`) embedded into the binary.
//!
//! The `dist/` folder is produced by `cargo xtask build-dashboard`; an empty
//! folder still compiles, so server builds never depend on Node being present.

use rust_embed::Embed;

#[derive(Embed, Debug)]
#[folder = "dist/"]
#[exclude = ".gitkeep"]
pub struct Assets;

impl Assets {
    /// True when a real bundle (not just the placeholder) was embedded.
    #[must_use]
    pub fn is_built() -> bool {
        Self::get("index.html").is_some()
    }
}
