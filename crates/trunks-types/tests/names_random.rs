//! Seeded random inputs against the parser invariants. Deterministic, so a failure is
//! reproducible; the coverage-guided counterpart lives in `fuzz/`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod invariants;

/// xorshift64*: small, fast and good enough to shuffle fragments.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).unwrap()
    }
}

/// Pieces that steer the inputs towards the interesting corners of the grammars.
const FRAGMENTS: &[&str] = &[
    "trunks://res/",
    "trunks://",
    "res/",
    "mcpj://res/",
    "__",
    "___",
    "_",
    "-",
    "/",
    "//",
    ":",
    "a",
    "Z",
    "0",
    "srv",
    "my-server",
    "tool_name",
    "x_",
    "QQ",
    "YQ",
    "YWJj",
    "YWI",
    "aGVsbG8",
    "+",
    "=",
    "==",
    "é",
    "日本",
    " ",
    "\t",
    "\n",
    "\0",
    "%2F",
    "..",
    "?",
    "#",
    "🦀",
];

const SERVERS: &[&str] = &["srv", "my-server", "a", "x_", "a__b", ""];
const BASE64: &[&str] = &["QQ", "YQ", "YWJj", "YWI", "aGVsbG8", "+", "/", "=", "QR", "!"];

/// Mostly unstructured; one input in four has the shape of a resource URI, so the deeper
/// branches of that parser (base64, UTF-8) are reached too.
fn random_input(rng: &mut Rng) -> String {
    if rng.below(4) == 0 {
        let server = SERVERS[rng.below(SERVERS.len())];
        let encoded: String = (0..rng.below(4)).map(|_| BASE64[rng.below(BASE64.len())]).collect();
        return format!("trunks://res/{server}/{encoded}");
    }
    let pieces = rng.below(9);
    (0..pieces).map(|_| FRAGMENTS[rng.below(FRAGMENTS.len())]).collect()
}

#[test]
fn parsers_hold_their_invariants_on_random_input() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..100_000 {
        let input = random_input(&mut rng);
        invariants::server_name(&input);
        invariants::group_name(&input);
        invariants::canonical_name(&input);
        invariants::resource_uri(&input);
    }
}

#[test]
fn the_generator_reaches_accepted_inputs() {
    // Guards the test above against silently exercising only the rejection paths.
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut servers, mut groups, mut canonical, mut uris) = (0, 0, 0, 0);
    for _ in 0..100_000 {
        let input = random_input(&mut rng);
        servers += usize::from(trunks_types::ServerName::parse(input.as_str()).is_ok());
        groups += usize::from(trunks_types::GroupName::parse(input.as_str()).is_ok());
        canonical += usize::from(trunks_types::CanonicalName::parse(&input).is_ok());
        uris += usize::from(trunks_types::ResourceUri::parse(&input).is_ok());
    }
    assert!(
        servers > 1000 && groups > 1000 && canonical > 1000 && uris > 1000,
        "{servers} {groups} {canonical} {uris}"
    );
}
