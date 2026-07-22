use serde::Deserialize;

use crate::error::{DriverError, ErrorKind};

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub drivers: DriversCfg,
}

#[derive(Debug, Default, Deserialize)]
pub struct DriversCfg {
    #[serde(default)]
    pub tmux: TmuxCfg,
}

#[derive(Debug, Deserialize)]
pub struct TmuxCfg {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub allowlist: Option<Vec<String>>,
}

impl Default for TmuxCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            allowlist: None,
        }
    }
}

fn default_true() -> bool {
    true
}

impl Config {
    // Intentional inherent constructor, not the `std::str::FromStr` trait: this
    // parses a whole-document TOML config, not a `FromStr`-style scalar parse,
    // and callers use it as `Config::from_str(&s)` rather than via `.parse()`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self, DriverError> {
        if s.trim().is_empty() {
            return Ok(Self::default());
        }
        toml::from_str(s).map_err(|e| DriverError::new(ErrorKind::Invalid, format!("config: {e}")))
    }

    pub fn load() -> Self {
        let path = dirs::config_dir().map(|d| d.join("cc-uplink/config.toml"));
        match path.and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(s) => Self::from_str(&s).unwrap_or_default(),
            None => Self::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_empty() {
        let c = Config::from_str("").unwrap();
        assert!(c.drivers.tmux.enabled);
        assert!(c.drivers.tmux.allowlist.is_none());
    }

    #[test]
    fn parses_allowlist() {
        let c =
            Config::from_str("[drivers.tmux]\nenabled = true\nallowlist = [\"codex\", \"%1\"]\n")
                .unwrap();
        assert_eq!(
            c.drivers.tmux.allowlist.as_deref(),
            Some(&["codex".to_string(), "%1".to_string()][..])
        );
    }

    #[test]
    fn bad_toml_is_invalid() {
        assert!(matches!(
            Config::from_str("not [ toml").unwrap_err().kind,
            crate::error::ErrorKind::Invalid
        ));
    }
}
