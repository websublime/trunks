//! Repositories: explicit SQL per operation, one string for both dialects.
//!
//! Reads for the catalog and the identity/config ports; registry writes in `registry`.

pub mod catalog;
pub mod config;
pub mod groups;
pub mod identity;
pub mod oauth;
pub mod registry;
