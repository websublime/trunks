//! Repositories: explicit SQL per operation, one string for both dialects.
//!
//! Phase 1 scope: everything the catalog and the server-mode service need to **read**, plus the
//! `ConfigStore` port. Writes arrive with the registry (Phase 2).

pub mod catalog;
pub mod config;
pub mod identity;
pub mod oauth;
