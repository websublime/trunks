//! Canonical naming rules shared by the gateway, the REST API and the CLI.
//!
//! Rules (verbatim from `MCPJungle`, see `internal/service/mcp/util.go`):
//! - server names match `^[a-zA-Z0-9_-]+$`, never contain `__` and never end with `_`;
//! - tools and prompts are exposed as `<server>__<local>`; the split is on the **first** `__`,
//!   so a local name may itself contain `__`;
//! - resources are exposed as `mcpj://res/<server>/<base64url-no-pad(original-uri)>`;
//! - tool-group names match `^[a-zA-Z0-9][a-zA-Z0-9_-]*$`.

use std::fmt;
use std::sync::LazyLock;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use regex::Regex;
use serde::{Deserialize, Serialize};
use snafu::{Snafu, ensure};

/// Separator between a server name and the upstream entity name.
pub const SEPARATOR: &str = "__";
/// Prefix of every gateway-assigned resource URI.
pub const RESOURCE_URI_PREFIX: &str = "mcpj://res/";

static SERVER_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"^[a-zA-Z0-9_-]+$").expect("static regex")
});
static GROUP_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9_-]*$").expect("static regex")
});

/// Errors produced while parsing names and URIs.
#[derive(Debug, Snafu, PartialEq, Eq)]
#[snafu(visibility(pub(crate)))]
pub enum NameError {
    #[snafu(display("server name cannot be empty"))]
    EmptyServerName,
    #[snafu(display(
        "invalid server name '{name}': must only contain letters, numbers, hyphens and underscores"
    ))]
    InvalidServerName { name: String },
    #[snafu(display("invalid server name '{name}': must not contain '__'"))]
    ServerNameContainsSeparator { name: String },
    #[snafu(display("invalid server name '{name}': must not end with '_'"))]
    ServerNameTrailingUnderscore { name: String },
    #[snafu(display("name '{name}' does not contain a __ separator"))]
    MissingSeparator { name: String },
    #[snafu(display(
        "invalid group name '{name}': must start with a letter or digit and only contain letters, digits, hyphens and underscores"
    ))]
    InvalidGroupName { name: String },
    #[snafu(display("invalid resource URI '{uri}': {reason}"))]
    InvalidResourceUri { uri: String, reason: String },
}

/// A validated upstream server name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ServerName(String);

impl ServerName {
    /// Validates and wraps a server name.
    pub fn parse(name: impl Into<String>) -> Result<Self, NameError> {
        let name = name.into();
        ensure!(!name.is_empty(), EmptyServerNameSnafu);
        ensure!(SERVER_NAME_RE.is_match(&name), InvalidServerNameSnafu { name });
        ensure!(!name.contains(SEPARATOR), ServerNameContainsSeparatorSnafu { name });
        ensure!(!name.ends_with('_'), ServerNameTrailingUnderscoreSnafu { name });
        Ok(Self(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ServerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ServerName {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(s).map_err(serde::de::Error::custom)
    }
}

/// `<server>__<local>` — the name under which a tool or prompt is exposed by the gateway.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CanonicalName {
    server: ServerName,
    local: String,
}

impl CanonicalName {
    #[must_use]
    pub fn new(server: ServerName, local: impl Into<String>) -> Self {
        Self { server, local: local.into() }
    }

    /// Splits on the first `__`. The server part is validated; the local part is opaque.
    pub fn parse(name: &str) -> Result<Self, NameError> {
        let (server, local) = name
            .split_once(SEPARATOR)
            .ok_or_else(|| NameError::MissingSeparator { name: name.to_owned() })?;
        Ok(Self { server: ServerName::parse(server)?, local: local.to_owned() })
    }

    #[must_use]
    pub fn server(&self) -> &ServerName {
        &self.server
    }

    #[must_use]
    pub fn local(&self) -> &str {
        &self.local
    }
}

impl fmt::Display for CanonicalName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{SEPARATOR}{}", self.server, self.local)
    }
}

impl Serialize for CanonicalName {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for CanonicalName {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// `mcpj://res/<server>/<base64(original)>` — the URI under which a resource is exposed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceUri {
    server: ServerName,
    original: String,
}

impl ResourceUri {
    #[must_use]
    pub fn new(server: ServerName, original: impl Into<String>) -> Self {
        Self { server, original: original.into() }
    }

    pub fn parse(uri: &str) -> Result<Self, NameError> {
        let invalid = |reason: &str| NameError::InvalidResourceUri {
            uri: uri.to_owned(),
            reason: reason.to_owned(),
        };
        let rest = uri
            .strip_prefix(RESOURCE_URI_PREFIX)
            .ok_or_else(|| invalid(&format!("must start with '{RESOURCE_URI_PREFIX}'")))?;
        let (server, encoded) =
            rest.split_once('/').ok_or_else(|| invalid("missing server segment"))?;
        ensure!(
            !server.is_empty(),
            InvalidResourceUriSnafu { uri, reason: "empty server segment" }
        );
        ensure!(
            !encoded.is_empty(),
            InvalidResourceUriSnafu { uri, reason: "empty encoded resource segment" }
        );
        let server = ServerName::parse(server)?;
        let bytes = STANDARD_NO_PAD
            .decode(encoded)
            .map_err(|_| invalid("resource segment is not valid unpadded base64"))?;
        let original = String::from_utf8(bytes)
            .map_err(|_| invalid("decoded resource URI is not valid UTF-8"))?;
        Ok(Self { server, original })
    }

    #[must_use]
    pub fn server(&self) -> &ServerName {
        &self.server
    }

    #[must_use]
    pub fn original(&self) -> &str {
        &self.original
    }
}

impl fmt::Display for ResourceUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{RESOURCE_URI_PREFIX}{}/{}",
            self.server,
            STANDARD_NO_PAD.encode(self.original.as_bytes())
        )
    }
}

impl Serialize for ResourceUri {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ResourceUri {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// A validated tool-group name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct GroupName(String);

impl GroupName {
    pub fn parse(name: impl Into<String>) -> Result<Self, NameError> {
        let name = name.into();
        ensure!(GROUP_NAME_RE.is_match(&name), InvalidGroupNameSnafu { name });
        Ok(Self(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GroupName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for GroupName {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // Case table transcribed from internal/service/mcp/util_test.go.
    #[test]
    fn server_name_accepts_valid_names() {
        for name in ["server_1", "server_2_multiple_underscores", "server-2", "_abc", "a"] {
            assert!(ServerName::parse(name).is_ok(), "{name} should be valid");
        }
    }

    #[test]
    fn server_name_rejects_invalid_names() {
        for name in [
            "_server_",
            "_",
            "server/3",
            "server$",
            "server__name",
            "__server",
            "server__",
            "__",
            "server___name",
            "",
            "with space",
        ] {
            assert!(ServerName::parse(name).is_err(), "{name:?} should be invalid");
        }
    }

    #[test]
    fn canonical_name_splits_on_first_separator() {
        let n = CanonicalName::parse("a__b__c").unwrap();
        assert_eq!(n.server().as_str(), "a");
        assert_eq!(n.local(), "b__c");
        assert_eq!(n.to_string(), "a__b__c");

        let n = CanonicalName::parse("myserver__my/tool").unwrap();
        assert_eq!(n.local(), "my/tool");

        let n = CanonicalName::parse("_abc__def").unwrap();
        assert_eq!(n.server().as_str(), "_abc");
    }

    #[test]
    fn canonical_name_requires_separator() {
        assert_eq!(
            CanonicalName::parse("nosep"),
            Err(NameError::MissingSeparator { name: "nosep".into() })
        );
    }

    #[test]
    fn resource_uri_round_trips() {
        let server = ServerName::parse("foo_mcp").unwrap();
        let uri = ResourceUri::new(server.clone(), "file://sample.txt");
        assert_eq!(uri.to_string(), "mcpj://res/foo_mcp/ZmlsZTovL3NhbXBsZS50eHQ");
        let parsed = ResourceUri::parse(&uri.to_string()).unwrap();
        assert_eq!(parsed, uri);
        assert_eq!(parsed.original(), "file://sample.txt");
    }

    #[test]
    fn resource_uri_rejects_bad_inputs() {
        for uri in [
            "file://x",
            "mcpj://res/",
            "mcpj://res/foo",
            "mcpj://res//ZmlsZQ",
            "mcpj://res/foo/",
            "mcpj://res/foo/!!!",
            "mcpj://res/bad__name/ZmlsZQ",
        ] {
            assert!(ResourceUri::parse(uri).is_err(), "{uri} should be invalid");
        }
    }

    #[test]
    fn group_name_rules() {
        for ok in ["g", "group-1", "G_2", "9lives"] {
            assert!(GroupName::parse(ok).is_ok(), "{ok}");
        }
        for bad in ["", "_group", "-group", "gr oup", "gr/oup", "grúpo"] {
            assert!(GroupName::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn serde_round_trip_canonical_name() {
        let n: CanonicalName = serde_json::from_str("\"ctx7__get-docs\"").unwrap();
        assert_eq!(serde_json::to_string(&n).unwrap(), "\"ctx7__get-docs\"");
    }
}
