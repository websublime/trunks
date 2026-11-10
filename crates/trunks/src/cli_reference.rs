//! Keeps `docs/guide/reference/cli.md` identical to what the binary prints for `--help`.
//!
//! The reference is generated from the command definitions; this test fails when the two
//! differ. To regenerate after changing a command:
//!
//! ```bash
//! TRUNKS_UPDATE_DOCS=1 cargo test -p trunks cli_reference
//! ```

use clap::CommandFactory as _;

const HEADER: &str = "\
# CLI reference

<!-- Generated from the command definitions. Do not edit by hand:
     TRUNKS_UPDATE_DOCS=1 cargo test -p trunks cli_reference -->

`trunks start` runs the server; every other command talks to a running server, whose address
comes from `--registry`, the `TRUNKS_REGISTRY` environment variable or the CLI's configuration
file, in that order (default `http://127.0.0.1:8080`). In enterprise mode the commands send the
access token stored by `trunks login` or `trunks init-server`.
";

/// `[env: NAME=current value]` → `[env: NAME]`: the reference must not depend on the
/// environment it was generated in.
fn without_env_values(help: &str) -> String {
    let mut out = String::with_capacity(help.len());
    let mut rest = help;
    while let Some(start) = rest.find("[env: ") {
        let (before, tagged) = rest.split_at(start);
        out.push_str(before);
        let Some(end) = tagged.find(']') else { break };
        let (tag, after) = tagged.split_at(end);
        out.push_str(tag.split_once('=').map_or(tag, |(name, _)| name));
        rest = after;
    }
    out.push_str(rest);
    out
}

fn section(command: &clap::Command, path: &str, out: &mut String) {
    let help = without_env_values(&command.clone().render_long_help().to_string());
    out.push_str("\n## `");
    out.push_str(path);
    out.push_str("`\n\n```text\n");
    for line in help.trim_end().lines() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.push_str("```\n");
    for sub in command.get_subcommands().filter(|c| !c.is_hide_set() && c.get_name() != "help") {
        section(sub, &format!("{path} {}", sub.get_name()), out);
    }
}

fn generated() -> String {
    let mut root = crate::Cli::command();
    root.build();
    let mut out = HEADER.to_owned();
    for sub in root.get_subcommands().filter(|c| !c.is_hide_set() && c.get_name() != "help") {
        section(sub, &format!("trunks {}", sub.get_name()), &mut out);
    }
    out
}

#[test]
fn cli_reference_matches_the_commands() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/guide/reference/cli.md");
    let generated = generated();
    if std::env::var_os("TRUNKS_UPDATE_DOCS").is_some() {
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert!(
        on_disk == generated,
        "docs/guide/reference/cli.md is out of date; regenerate it with\n\
         TRUNKS_UPDATE_DOCS=1 cargo test -p trunks cli_reference"
    );
}

#[test]
fn environment_values_are_left_out() {
    let cases = [
        (
            "--port <PORT>  [env: TRUNKS_PORT=9000] [default: 8080]",
            "--port <PORT>  [env: TRUNKS_PORT] [default: 8080]",
        ),
        ("[env: TRUNKS_REGISTRY=]", "[env: TRUNKS_REGISTRY]"),
        ("[env: TRUNKS_OIDC_CLIENT_SECRET]", "[env: TRUNKS_OIDC_CLIENT_SECRET]"),
        ("no tags here", "no tags here"),
        ("[env: A=1] and [env: B=http://x/?a=b]", "[env: A] and [env: B]"),
    ];
    for (help, expected) in cases {
        assert_eq!(without_env_values(help), expected);
    }
}
