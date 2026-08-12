//! Pure permission-policy decisions for the tmux driver.
//!
//! Everything here is a function of (pane marks, config) — no I/O, no tmux.
//! The driver fetches marks and config; this module decides. The grant
//! markers themselves (`@uplink_profile`, `@uplink_read`) are human-set;
//! the driver only ever reads them.

use serde::{Deserialize, Serialize};
use crate::config::TmuxCfg;
use crate::error::{DriverError, ErrorKind};

/// Ordered permission tiers; each includes everything below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Observer,
    Operator,
    Godmode,
}

impl Tier {
    /// Exact lowercase names only — an unrecognised value must never grant.
    pub fn parse(s: &str) -> Option<Tier> {
        match s {
            "observer" => Some(Tier::Observer),
            "operator" => Some(Tier::Operator),
            "godmode" => Some(Tier::Godmode),
            _ => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::Observer => "observer",
            Tier::Operator => "operator",
            Tier::Godmode => "godmode",
        }
    }
}

/// `*`-only glob. Iterative backtracking matcher — no regex dependency.
pub fn glob_match(pat: &str, s: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pat.chars().collect(), s.chars().collect());
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// A pane's human-set grant state, read (never written) by the driver from
/// `#{@name}|#{@uplink_profile}|#{@uplink_read}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneMarks {
    pub label: Option<String>,
    pub profile: Option<Tier>,
    pub read_off: bool,
}

pub fn parse_marks(raw: &str) -> PaneMarks {
    let trimmed = raw.trim_end_matches(['\n', '\r']);
    let fields: Vec<&str> = trimmed.split('|').collect();
    // A '|' inside @name (the free-form field) would shift every field and
    // could flip read_off to false — defeating the sticky read-deny guarantee.
    // Fail closed on any unexpected shape: observer (least privilege) + read-blocked.
    if fields.len() != 3 {
        return PaneMarks { label: None, profile: None, read_off: true };
    }
    let (label, profile, read) = (fields[0], fields[1], fields[2]);
    PaneMarks {
        label: (!label.is_empty()).then(|| label.to_string()),
        profile: Tier::parse(profile),
        read_off: read == "off",
    }
}

/// Highest tier any matching write_allow glob grants (Observer if none).
pub fn config_write_tier(label: Option<&str>, cfg: &TmuxCfg) -> Tier {
    let Some(l) = label else { return Tier::Observer };
    cfg.write_allow
        .iter()
        .filter(|(pat, _)| glob_match(pat, l))
        .map(|(_, t)| *t)
        .max()
        .unwrap_or(Tier::Observer)
}

/// Pane option decides alone when present (explicit observer pins down);
/// config globs fill in only for unmarked panes.
pub fn effective_tier(marks: &PaneMarks, cfg: &TmuxCfg) -> Tier {
    marks
        .profile
        .unwrap_or_else(|| config_write_tier(marks.label.as_deref(), cfg))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadBlock {
    PaneOption,
    ConfigGlob(String),
}

/// Deny is sticky: either layer blocks, no tier unblocks.
pub fn read_block(marks: &PaneMarks, cfg: &TmuxCfg) -> Option<ReadBlock> {
    if marks.read_off {
        return Some(ReadBlock::PaneOption);
    }
    let l = marks.label.as_deref()?;
    cfg.read_deny
        .iter()
        .find(|pat| glob_match(pat, l))
        .map(|pat| ReadBlock::ConfigGlob(pat.clone()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyClass {
    Benign,
    Dangerous,
}

const BENIGN_KEYS: &[&str] = &[
    "Enter", "Escape", "Tab", "Space", "BSpace", "Up", "Down", "Left", "Right",
    "PageUp", "PageDown", "PPage", "NPage", "Home", "End",
];

/// Benign = navigation/submit; Dangerous = any control/meta chord (C-c kills,
/// C-d hangs up). Anything else is rejected outright — literal text belongs
/// in the `type` op, and an unrecognised key name must never slip through as
/// one.
pub fn classify_key(key: &str) -> Result<KeyClass, DriverError> {
    if BENIGN_KEYS.contains(&key) {
        return Ok(KeyClass::Benign);
    }
    if key.starts_with("C-") || key.starts_with("M-") || key.starts_with('^') {
        return Ok(KeyClass::Dangerous);
    }
    Err(
        DriverError::new(ErrorKind::Invalid, format!("unknown key '{key}'"))
            .with_hint("special keys only (Enter, Escape, C-c…); literal text goes through the type op"),
    )
}

/// The rename-as-escalation guard: renaming a pane must never raise the tier
/// the config globs would grant it beyond what it already has.
pub fn label_escalates(new_name: &str, current_effective: Tier, cfg: &TmuxCfg) -> bool {
    config_write_tier(Some(new_name), cfg) > current_effective
}

pub fn require(effective: Tier, needed: Tier, what: &str) -> Result<(), DriverError> {
    if effective >= needed {
        return Ok(());
    }
    Err(DriverError::new(
        ErrorKind::Rejected,
        format!(
            "{what} requires {} (pane is {})",
            needed.as_str(),
            effective.as_str()
        ),
    )
    .with_hint(
        "grants are human-set: `tmux set -p @uplink_profile <tier>` on the target pane \
         (prefix+g with the uplink.tmux menu), or a write_allow glob in config.toml",
    ))
}

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Mtime-checked config cache: grants take effect on file save, no MCP
/// restart. Only the tmux policy section reloads; driver enablement is
/// construction-time. Parse errors keep the last good config (fail-stable,
/// never fail-open to defaults).
pub struct PolicyCache {
    path: Option<PathBuf>,
    state: Mutex<(Option<SystemTime>, Arc<TmuxCfg>)>,
}

impl PolicyCache {
    pub fn new(initial: TmuxCfg) -> Self {
        Self::with_path(initial, crate::config::Config::path())
    }

    pub fn with_path(initial: TmuxCfg, path: Option<PathBuf>) -> Self {
        let mtime = path.as_deref().and_then(mtime_of);
        Self {
            path,
            state: Mutex::new((mtime, Arc::new(initial))),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn current(&self) -> Arc<TmuxCfg> {
        let mut st = self.state.lock().expect("policy cache poisoned");
        if let Some(p) = self.path.as_deref() {
            let now = mtime_of(p);
            if now != st.0 {
                st.0 = now;
                if let Ok(s) = std::fs::read_to_string(p) {
                    if let Ok(full) = crate::config::Config::from_str(&s) {
                        st.1 = Arc::new(full.drivers.tmux);
                    }
                }
            }
        }
        st.1.clone()
    }
}

fn mtime_of(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
mod tier_tests {
    use super::*;

    #[test]
    fn tiers_are_ordered_and_parse_lowercase() {
        assert!(Tier::Observer < Tier::Operator);
        assert!(Tier::Operator < Tier::Godmode);
        assert_eq!(Tier::parse("operator"), Some(Tier::Operator));
        assert_eq!(Tier::parse(""), None);
        assert_eq!(Tier::parse("Operator"), None); // exact lowercase only — no fuzzy grants
        assert_eq!(Tier::Godmode.as_str(), "godmode");
    }

    #[test]
    fn glob_star_only() {
        assert!(glob_match("codex", "codex"));
        assert!(!glob_match("codex", "codex2"));
        assert!(glob_match("lab-*", "lab-1"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("lab-*", "prod-1"));
        assert!(!glob_match("", "x"));
        assert!(glob_match("", ""));
    }
}

#[cfg(test)]
mod decision_tests {
    use super::*;
    use crate::config::TmuxCfg;

    fn cfg(write: &[(&str, Tier)], deny: &[&str]) -> TmuxCfg {
        TmuxCfg {
            write_allow: write.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            read_deny: deny.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn parse_marks_splits_and_rejects_junk_profile() {
        let m = parse_marks("codex|operator|");
        assert_eq!(m.label.as_deref(), Some("codex"));
        assert_eq!(m.profile, Some(Tier::Operator));
        assert!(!m.read_off);
        let m = parse_marks("|sudo-make-me-root|off");
        assert_eq!(m.label, None);
        assert_eq!(m.profile, None); // unknown value never grants
        assert!(m.read_off);
    }

    #[test]
    fn pipe_in_label_fails_closed_not_unblocked() {
        // A label containing '|' must not shift fields and flip read_off to false.
        let m = parse_marks("my|label|operator|off");
        assert_eq!(m.profile, None);   // least privilege
        assert!(m.read_off);           // read stays blocked, not unblocked
        // well-formed inputs are unchanged
        let ok = parse_marks("codex|operator|");
        assert_eq!(ok.label.as_deref(), Some("codex"));
        assert_eq!(ok.profile, Some(Tier::Operator));
        assert!(!ok.read_off);
    }

    #[test]
    fn pane_option_wins_even_downward() {
        // explicit observer pins read-only despite a godmode glob
        let c = cfg(&[("lab-*", Tier::Godmode)], &[]);
        let m = PaneMarks { label: Some("lab-1".into()), profile: Some(Tier::Observer), read_off: false };
        assert_eq!(effective_tier(&m, &c), Tier::Observer);
    }

    #[test]
    fn config_globs_highest_match_wins_only_without_pane_option() {
        let c = cfg(&[("lab-*", Tier::Operator), ("lab-9", Tier::Godmode)], &[]);
        let m = PaneMarks { label: Some("lab-9".into()), profile: None, read_off: false };
        assert_eq!(effective_tier(&m, &c), Tier::Godmode);
        let unlabeled = PaneMarks { label: None, profile: None, read_off: false };
        assert_eq!(effective_tier(&unlabeled, &c), Tier::Observer);
    }

    #[test]
    fn read_block_is_sticky_deny() {
        let c = cfg(&[], &["customer-*"]);
        let m = PaneMarks { label: Some("customer-nas".into()), profile: Some(Tier::Godmode), read_off: false };
        assert!(matches!(read_block(&m, &c), Some(ReadBlock::ConfigGlob(_))));
        let m2 = PaneMarks { label: None, profile: None, read_off: true };
        assert!(matches!(read_block(&m2, &c), Some(ReadBlock::PaneOption)));
        let clear = PaneMarks { label: Some("dev".into()), profile: None, read_off: false };
        assert!(read_block(&clear, &c).is_none());
    }

    #[test]
    fn key_classes() {
        assert!(matches!(classify_key("Enter"), Ok(KeyClass::Benign)));
        assert!(matches!(classify_key("Escape"), Ok(KeyClass::Benign)));
        assert!(matches!(classify_key("PageUp"), Ok(KeyClass::Benign)));
        assert!(matches!(classify_key("C-c"), Ok(KeyClass::Dangerous)));
        assert!(matches!(classify_key("M-x"), Ok(KeyClass::Dangerous)));
        let e = classify_key("DoRootThings").unwrap_err();
        assert!(matches!(e.kind, crate::error::ErrorKind::Invalid)); // fail closed
    }

    #[test]
    fn require_names_tier_and_remedy() {
        assert!(require(Tier::Operator, Tier::Operator, "type").is_ok());
        let e = require(Tier::Observer, Tier::Operator, "type").unwrap_err();
        assert!(matches!(e.kind, crate::error::ErrorKind::Rejected));
        assert!(e.message.contains("operator"));
        assert!(e.hint.as_deref().unwrap_or("").contains("@uplink_profile"));
    }

    #[test]
    fn label_rename_cannot_raise_config_tier() {
        let c = cfg(&[("lab-*", Tier::Godmode), ("codex", Tier::Operator)], &[]);
        // operator pane renaming itself into the godmode glob: escalation
        assert!(label_escalates("lab-1", Tier::Operator, &c));
        // sideways or downward renames are fine
        assert!(!label_escalates("codex", Tier::Operator, &c));
        assert!(!label_escalates("scratch", Tier::Operator, &c));
        assert!(!label_escalates("lab-1", Tier::Godmode, &c));
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn reloads_on_mtime_change_keeps_cache_on_parse_error() {
        let dir = std::env::temp_dir().join(format!("ccu-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.toml");
        std::fs::write(&p, "[drivers.tmux]\n").unwrap();
        let cache = PolicyCache::with_path(TmuxCfg::default(), Some(p.clone()));
        assert!(cache.current().write_allow.is_empty());

        // backdate-proof: bump mtime explicitly rather than sleeping
        std::fs::write(&p, "[drivers.tmux]\nwrite_allow = { \"hot\" = \"operator\" }\n").unwrap();
        let bumped = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
        let f = std::fs::File::options().append(true).open(&p).unwrap();
        f.set_modified(bumped).unwrap();
        assert_eq!(cache.current().write_allow.get("hot"), Some(&Tier::Operator));

        // parse error → keep last good config
        std::fs::write(&p, "not [ toml").unwrap();
        let f = std::fs::File::options().append(true).open(&p).unwrap();
        f.set_modified(bumped + std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(cache.current().write_allow.get("hot"), Some(&Tier::Operator));
        std::fs::remove_dir_all(&dir).ok();
    }
}
