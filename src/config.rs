use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::drivers::tmux::policy::Tier;
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
    #[serde(default, rename = "image-openai")]
    pub image_openai: ImageOpenAiCfg,
    #[serde(default, rename = "image-codex")]
    pub image_codex: ImageCodexCfg,
}

#[derive(Debug, Deserialize)]
pub struct TmuxCfg {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Label glob → granted tier. Consulted only when the pane carries no
    /// `@uplink_profile` option. Highest matching tier wins.
    #[serde(default)]
    pub write_allow: BTreeMap<String, Tier>,
    /// Label globs whose panes are read-blocked at every tier (sticky deny).
    #[serde(default)]
    pub read_deny: Vec<String>,
    /// REMOVED in 0.1.0 — parsed only so doctor can tell you to migrate.
    #[serde(default)]
    pub allowlist: Option<Vec<String>>,
}

impl Default for TmuxCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            write_allow: BTreeMap::new(),
            read_deny: Vec::new(),
            allowlist: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageOpenAiCfg {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_openai_model")]
    pub model: String,
    #[serde(default = "default_openai_base")]
    pub base_url: String,
}

impl Default for ImageOpenAiCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            api_key_env: default_key_env(),
            model: default_openai_model(),
            base_url: default_openai_base(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageCodexCfg {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_codex_bin")]
    pub codex_bin: String,
}

impl Default for ImageCodexCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            codex_bin: default_codex_bin(),
        }
    }
}

fn default_codex_bin() -> String {
    "codex".into()
}

fn default_true() -> bool {
    true
}

fn default_key_env() -> String {
    "OPENAI_API_KEY".into()
}
fn default_openai_model() -> String {
    "gpt-image-1".into()
}
fn default_openai_base() -> String {
    "https://api.openai.com/v1".into()
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

    /// Config file location: `$CC_UPLINK_CONFIG` wins (tests, unusual setups),
    /// else the platform config dir.
    pub fn path() -> Option<PathBuf> {
        std::env::var_os("CC_UPLINK_CONFIG")
            .map(PathBuf::from)
            .or_else(|| dirs::config_dir().map(|d| d.join("cc-uplink/config.toml")))
    }

    pub fn load() -> Self {
        match Config::path().and_then(|p| std::fs::read_to_string(p).ok()) {
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
        assert!(c.drivers.tmux.write_allow.is_empty());
    }

    #[test]
    fn parses_write_allow_and_read_deny() {
        let c = Config::from_str(
            "[drivers.tmux]\nwrite_allow = { \"codex\" = \"operator\", \"lab-*\" = \"godmode\" }\nread_deny = [\"customer-*\"]\n",
        )
        .unwrap();
        use crate::drivers::tmux::policy::Tier;
        assert_eq!(
            c.drivers.tmux.write_allow.get("codex"),
            Some(&Tier::Operator)
        );
        assert_eq!(
            c.drivers.tmux.write_allow.get("lab-*"),
            Some(&Tier::Godmode)
        );
        assert_eq!(c.drivers.tmux.read_deny, vec!["customer-*".to_string()]);
    }

    #[test]
    fn legacy_allowlist_still_parses_but_is_separate() {
        let c = Config::from_str("[drivers.tmux]\nallowlist = [\"codex\"]\n").unwrap();
        assert!(c.drivers.tmux.allowlist.is_some()); // doctor will warn on this
        assert!(c.drivers.tmux.write_allow.is_empty());
    }

    #[test]
    fn config_path_honours_env_override() {
        // SAFETY: single-threaded test process section; guarded name unique to this test
        unsafe { std::env::set_var("CC_UPLINK_CONFIG", "/tmp/x.toml") };
        assert_eq!(
            Config::path(),
            Some(std::path::PathBuf::from("/tmp/x.toml"))
        );
        unsafe { std::env::remove_var("CC_UPLINK_CONFIG") };
        // fallback path ends with the canonical suffix
        if let Some(p) = Config::path() {
            assert!(p.ends_with("cc-uplink/config.toml"));
        }
    }

    #[test]
    fn bad_toml_is_invalid() {
        assert!(matches!(
            Config::from_str("not [ toml").unwrap_err().kind,
            crate::error::ErrorKind::Invalid
        ));
    }

    #[test]
    fn image_openai_defaults() {
        let c = Config::from_str("").unwrap();
        assert!(c.drivers.image_openai.enabled);
        assert_eq!(c.drivers.image_openai.api_key_env, "OPENAI_API_KEY");
        assert_eq!(c.drivers.image_openai.model, "gpt-image-1");
        assert_eq!(c.drivers.image_openai.base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn image_openai_section_parses_with_dash_name() {
        let c = Config::from_str(
            "[drivers.image-openai]\nenabled = false\napi_key_env = \"MY_KEY\"\nmodel = \"gpt-image-1-mini\"\nbase_url = \"http://127.0.0.1:8080/v1\"\n",
        )
        .unwrap();
        assert!(!c.drivers.image_openai.enabled);
        assert_eq!(c.drivers.image_openai.api_key_env, "MY_KEY");
        assert_eq!(c.drivers.image_openai.model, "gpt-image-1-mini");
        assert_eq!(c.drivers.image_openai.base_url, "http://127.0.0.1:8080/v1");
    }

    #[test]
    fn image_codex_defaults() {
        let c = Config::from_str("").unwrap();
        assert!(c.drivers.image_codex.enabled);
        assert_eq!(c.drivers.image_codex.codex_bin, "codex");
    }

    #[test]
    fn image_codex_section_parses() {
        let c = Config::from_str(
            "[drivers.image-codex]\nenabled = false\ncodex_bin = \"/opt/codex/bin/codex\"\n",
        )
        .unwrap();
        assert!(!c.drivers.image_codex.enabled);
        assert_eq!(c.drivers.image_codex.codex_bin, "/opt/codex/bin/codex");
    }
}
