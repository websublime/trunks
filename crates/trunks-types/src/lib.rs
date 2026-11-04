#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! Shared, dependency-light contracts for Trunks.
//!
//! JSON field names, enum spellings and the canonical naming rules are part of
//! the public contract (see `docs/02-especificacao.md`) and must not change
//! within a major version.

pub mod enums;
pub mod error;
pub mod names;
pub mod version;

pub use enums::{Role, ServerMode, SessionMode, Transport};
pub use error::ApiErrorResponse;
pub use names::{CanonicalName, GroupName, NameError, ResourceUri, ServerName};
