//! CLI configuration file: `~/.config/trunks/config.toml` (XDG), mode 0600.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Whatever};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CliConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
}

pub fn path() -> Option<PathBuf> {
    std::env::var_os("TRUNKS_CONFIG")
        .map(PathBuf::from)
        .or_else(|| dirs::config_dir().map(|d| d.join("trunks").join("config.toml")))
}

/// Best-effort load: a missing or unreadable file is an empty config.
pub fn load() -> CliConfig {
    let Some(p) = path() else { return CliConfig::default() };
    match std::fs::read_to_string(&p) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => CliConfig::default(),
    }
}

pub fn save(cfg: &CliConfig) -> Result<PathBuf, Whatever> {
    let Some(p) = path() else {
        snafu::whatever!("cannot determine the config directory");
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).whatever_context("creating config directory")?;
    }
    let text = toml::to_string_pretty(cfg).whatever_context("serializing config")?;
    std::fs::write(&p, text).whatever_context("writing config file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    Ok(p)
}
