#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! Trunks domain layer.
//!
//! This crate owns every business rule of the gateway and depends on nothing
//! web- or database-specific: persistence is reached through the traits in
//! [`ports`], and the MCP SDK is only visible from the `gateway` and `upstream`
//! modules (added in later phases). See `docs/04-arquitectura.md`.

pub mod catalog;
pub mod domain;
pub mod error;
pub mod gateway;
pub mod identity;
pub mod metrics;
pub mod mode;
pub mod ports;
pub mod sessions;
pub mod upstream;

pub use error::{Error, Result};
