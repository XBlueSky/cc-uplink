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
    #[serde(default, rename = "image-openai")]
    pub image_openai: ImageOpenAiCfg,
    #[serde(default, rename = "image-codex")]
    pub image_codex: ImageCodexCfg,
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
