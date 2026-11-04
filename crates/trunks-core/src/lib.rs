#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! Trunks domain layer.
//!
//! This crate owns every business rule of the gateway and depends on nothing
//! web- or database-specific: persistence is reached through the traits in
//! [`ports`], and the MCP SDK is only visible from the `gateway` and `upstream`
//! modules (added in later phases). See `docs/05-arquitectura.md`.

pub mod error;
pub mod ports;

pub use error::{Error, Result};
