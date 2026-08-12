//! Pure permission-policy decisions for the tmux driver.
//!
//! Everything here is a function of (pane marks, config) — no I/O, no tmux.
//! The driver fetches marks and config; this module decides. The grant
//! markers themselves (`@uplink_profile`, `@uplink_read`) are human-set;
//! the driver only ever reads them.

use serde::{Deserialize, Serialize};

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
