#![no_main]

#[path = "../../crates/trunks-types/tests/invariants/mod.rs"]
mod invariants;

libfuzzer_sys::fuzz_target!(|input: &str| {
    invariants::server_name(input);
    invariants::group_name(input);
    invariants::canonical_name(input);
});
