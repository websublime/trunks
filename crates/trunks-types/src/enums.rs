//! Enumerations that are part of the wire and database contract.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Transport used to reach an upstream MCP server.
///
/// `Sse` is kept only so that legacy rows can be deserialized;
/// Trunks does not connect to SSE upstreams (decision D4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    Stdio,
    StreamableHttp,
    Sse,
}

impl Transport {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::StreamableHttp => "streamable_http",
            Self::Sse => "sse",
        }
    }

    #[must_use]
    pub fn is_supported(self) -> bool {
        !matches!(self, Self::Sse)
    }
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How the gateway manages the connection to an upstream server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    #[default]
    Stateless,
    Stateful,
}

impl SessionMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stateless => "stateless",
            Self::Stateful => "stateful",
        }
    }
}

impl fmt::Display for SessionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Role of a human user (enterprise mode only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

impl Role {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Operating mode of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ServerMode {
    #[default]
    Development,
    Enterprise,
}

impl ServerMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Enterprise => "enterprise",
        }
    }

    #[must_use]
    pub fn is_enterprise(self) -> bool {
        matches!(self, Self::Enterprise)
    }

    /// Case-insensitive parse.
    #[must_use]
    pub fn parse_lenient(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "development" => Some(Self::Development),
            "enterprise" => Some(Self::Enterprise),
            _ => None,
        }
    }
}

impl fmt::Display for ServerMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ServerMode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse_lenient(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid server mode '{s}': expected development or enterprise"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_mode_parses_case_insensitively() {
        assert_eq!(ServerMode::parse_lenient("ENTERPRISE"), Some(ServerMode::Enterprise));
        assert_eq!(ServerMode::parse_lenient("Development"), Some(ServerMode::Development));
        assert_eq!(ServerMode::parse_lenient("production"), None);
        assert_eq!(ServerMode::parse_lenient("staging"), None);
        let m: ServerMode = serde_json::from_str("\"enterprise\"").unwrap();
        assert_eq!(serde_json::to_string(&m).unwrap(), "\"enterprise\"");
    }

    #[test]
    fn transport_wire_names() {
        assert_eq!(
            serde_json::to_string(&Transport::StreamableHttp).unwrap(),
            "\"streamable_http\""
        );
        let t: Transport = serde_json::from_str("\"sse\"").unwrap();
        assert!(!t.is_supported());
    }

    #[test]
    fn session_mode_defaults_to_stateless() {
        assert_eq!(SessionMode::default(), SessionMode::Stateless);
    }
}
