#![no_main]

#[path = "../../crates/trunks-types/tests/invariants/mod.rs"]
mod invariants;

libfuzzer_sys::fuzz_target!(|input: &str| invariants::resource_uri(input));
