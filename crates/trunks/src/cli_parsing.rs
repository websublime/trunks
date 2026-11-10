//! Values that may legitimately start with `-`: generated tokens are base64url, so one in 64
//! does, and the arguments of an upstream command (`npx -y …`) usually do.

use clap::Parser as _;
use clap::error::ErrorKind;

use crate::Cli;

#[test]
fn values_starting_with_a_hyphen_are_values() {
    let cases: &[(&[&str], &str)] = &[
        (&["login", "-9fQ_token"], "-9fQ_token"),
        (&["login", "--registry", "http://r", "-9fQ_token"], "-9fQ_token"),
        (&["create", "user", "ana", "--access-token", "-secret-1"], "-secret-1"),
        (&["create", "mcp-client", "ci", "--access-token", "-secret-2"], "-secret-2"),
        (&["update", "user", "ana", "--access-token", "-secret-3"], "-secret-3"),
        (&["update", "mcp-client", "ci", "--access-token", "-secret-4"], "-secret-4"),
        (&["oauth-complete", "sid", "--code", "-c0de", "--state", "-st4te"], "-c0de"),
        (&["oauth-complete", "sid", "--code", "-c0de", "--state", "-st4te"], "-st4te"),
        (&["register", "--name", "s", "--url", "http://u", "--bearer-token", "-b34rer"], "-b34rer"),
        (&["register", "--name", "s", "--url", "http://u", "--oauth-client-secret", "-cs"], "-cs"),
        (&["register", "--name", "s", "--command", "npx", "--arg", "-y", "--arg", "pkg"], "\"-y\""),
        (
            &["register", "--name", "s", "--command", "sh", "--arg", "--flag", "--arg", "-c"],
            "--flag",
        ),
        (&["start", "--oidc-client-secret", "-oidc"], "-oidc"),
    ];
    for (args, expected) in cases {
        let argv = std::iter::once("trunks").chain(args.iter().copied());
        let cli = Cli::try_parse_from(argv).unwrap_or_else(|e| panic!("{args:?}: {e}"));
        assert!(format!("{cli:?}").contains(expected), "{args:?} lost {expected}: {cli:?}");
    }
}

#[test]
fn flags_are_still_flags_next_to_those_values() {
    // `--registry` after the token is the global flag, not part of the token.
    let cli = Cli::try_parse_from(["trunks", "login", "-tok", "--registry", "http://r"]).unwrap();
    assert_eq!(cli.registry.as_deref(), Some("http://r"));

    // `--force` after `--arg` values is the flag, not a program argument.
    let cli = Cli::try_parse_from([
        "trunks",
        "register",
        "--name",
        "s",
        "--command",
        "npx",
        "--arg",
        "-y",
        "--force",
    ])
    .unwrap();
    let shown = format!("{cli:?}");
    assert!(shown.contains("force: true"), "{shown}");
    assert!(shown.contains("argv: [\"-y\"]"), "{shown}");

    for args in
        [&["trunks", "login", "--help"][..], &["trunks", "register", "--arg", "-y", "--help"]]
    {
        let err = Cli::try_parse_from(args).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayHelp, "{args:?}");
    }

    // A missing value is still an error rather than swallowing the next flag.
    let err = Cli::try_parse_from(["trunks", "login"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
}
