//! `${VAR}` expansion in CLI configuration files. Only the `${NAME}` form is recognized; a
//! variable that is not set is an error (no silent empty strings in credentials).

use serde_json::Value;
use snafu::Whatever;

/// Expands every `${VAR}` inside the string values of `v`, recursively.
pub fn expand(v: &mut Value) -> Result<(), Whatever> {
    match v {
        Value::String(s) => {
            *s = expand_str(s)?;
        }
        Value::Array(items) => {
            for item in items {
                expand(item)?;
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                expand(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn expand_str(s: &str) -> Result<String, Whatever> {
    expand_with(s, |name| std::env::var(name).ok())
}

fn expand_with(s: &str, lookup: impl Fn(&str) -> Option<String>) -> Result<String, Whatever> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            snafu::whatever!("unterminated ${{ in '{s}'");
        };
        let name = &after[..end];
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            snafu::whatever!("invalid variable reference '${{{name}}}'");
        }
        match lookup(name) {
            Some(value) => out.push_str(&value),
            None => snafu::whatever!("environment variable '{name}' is not set"),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(name: &str) -> Option<String> {
        (name == "TOKEN").then(|| "s3cret".to_owned())
    }

    #[test]
    fn expands_known_and_rejects_unknown() {
        let cases: &[(&str, Option<&str>)] = &[
            ("Bearer ${TOKEN}!", Some("Bearer s3cret!")),
            ("plain", Some("plain")),
            ("${TOKEN}${TOKEN}", Some("s3crets3cret")),
            ("${MISSING}", None),
            ("${bad-name}", None),
            ("${}", None),
            ("${unterminated", None),
        ];
        for (input, expected) in cases {
            let got = expand_with(input, lookup);
            assert_eq!(got.ok().as_deref(), *expected, "input {input:?}");
        }
    }

    #[test]
    fn expands_nested_values() {
        let mut v = serde_json::json!({"env": {"T": "${TOKEN}"}, "args": ["${TOKEN}"], "n": 1});
        // `expand` reads the real environment; exercise the walker through a known-absent var.
        assert!(expand(&mut v).is_err());
        let mut v = serde_json::json!({"env": {"T": "x"}, "args": ["y"], "n": 1});
        expand(&mut v).unwrap();
        assert_eq!(v["env"]["T"], "x");
    }
}
