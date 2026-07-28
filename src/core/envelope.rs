use crate::core::driver::ReplyHint;

pub struct Inbound {
    pub kind: InboundKind,
    pub from: Option<String>,
    pub id: Option<String>,
    pub body: String,
}

pub enum InboundKind {
    Uplink,
    Reply,
}

pub fn new_correlation_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

pub fn format_outbound(
    from: &str,
    own_pane: &str,
    id: &str,
    message: &str,
    hint: ReplyHint,
) -> String {
    let head = format!("[uplink from:{from} pane:{own_pane} id:{id}] {message}");
    match hint {
        ReplyHint::None => head,
        ReplyHint::Short => format!("{head} (reply-to:{own_pane} id:{id})"),
        ReplyHint::Full => format!(
            "{head} (reply: run `tmux send-keys -t {own_pane} -l '[reply id:{id}] <your answer>' \\; send-keys -t {own_pane} Enter`)"
        ),
    }
}

/// Prompt/TUI decoration a pane may render in front of an injected envelope: a
/// Claude Code input box echoes `❯ [reply …]`, a shell echoes `$ [reply …]`, a
/// boxed TUI echoes `│ [reply …]`.
const DECORATION: &[char] = &[
    ' ', '\t', '❯', '›', '>', '$', '#', '%', '|', '│', '┃', '▌', '▎', '⏺', '⎿', '●', '•', '*',
];

/// Trims a LEADING run of [`DECORATION`] so the envelope match stays anchored.
/// Anchoring is the point: `format_outbound`'s `ReplyHint::Full` text embeds the
/// literal `[reply id:<id>] <your answer>`, so a scan-anywhere match would parse
/// an echo of our own instruction as an inbound reply.
fn strip_decoration(line: &str) -> &str {
    line.trim_start_matches(DECORATION)
}

pub fn parse_inbound(line: &str) -> Option<Inbound> {
    let line = strip_decoration(line.trim());
    if let Some(rest) = line.strip_prefix("[reply id:") {
        let (id, body) = rest.split_once(']')?;
        return Some(Inbound {
            kind: InboundKind::Reply,
            from: None,
            id: Some(id.trim().to_string()),
            body: body.trim().to_string(),
        });
    }
    if let Some(rest) = line.strip_prefix("[uplink ") {
        let (fields, body) = rest.split_once(']')?;
        let mut from = None;
        let mut id = None;
        for tok in fields.split_whitespace() {
            if let Some(v) = tok.strip_prefix("from:") {
                from = Some(v.to_string());
            }
            if let Some(v) = tok.strip_prefix("id:") {
                id = Some(v.to_string());
            }
        }
        return Some(Inbound {
            kind: InboundKind::Uplink,
            from,
            id,
            body: body.trim().to_string(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::driver::ReplyHint;

    #[test]
    fn outbound_full() {
        let s = format_outbound("claude", "%5", "ab12cd34", "hello", ReplyHint::Full);
        assert!(s.starts_with("[uplink from:claude pane:%5 id:ab12cd34] hello"));
        assert!(s.contains("tmux send-keys -t %5 -l '[reply id:ab12cd34]"));
    }

    #[test]
    fn outbound_none_has_no_reply_block() {
        let s = format_outbound("claude", "%5", "ab12cd34", "hello", ReplyHint::None);
        assert_eq!(s, "[uplink from:claude pane:%5 id:ab12cd34] hello");
    }

    #[test]
    fn parse_uplink_and_reply() {
        let u = parse_inbound("[uplink from:codex pane:%2 id:ffffffff] hi there").unwrap();
        assert!(matches!(u.kind, InboundKind::Uplink));
        assert_eq!(u.from.as_deref(), Some("codex"));
        assert_eq!(u.id.as_deref(), Some("ffffffff"));
        assert_eq!(u.body, "hi there");

        let r = parse_inbound("[reply id:ab12cd34] the answer").unwrap();
        assert!(matches!(r.kind, InboundKind::Reply));
        assert_eq!(r.id.as_deref(), Some("ab12cd34"));
        assert_eq!(r.body, "the answer");

        assert!(parse_inbound("plain output line").is_none());
    }

    #[test]
    fn parse_reply_behind_tui_prompt_glyph() {
        // A Claude Code caller's own pane renders the peer's injected reply
        // inside its input box, so the echoed line carries the prompt glyph.
        let r = parse_inbound("❯ [reply id:deadbeef] forty-two").unwrap();
        assert!(matches!(r.kind, InboundKind::Reply));
        assert_eq!(r.id.as_deref(), Some("deadbeef"));
        assert_eq!(r.body, "forty-two");
    }

    #[test]
    fn parse_uplink_behind_shell_prompt() {
        let u = parse_inbound("$ [uplink from:codex pane:%2 id:ffffffff] hi there").unwrap();
        assert!(matches!(u.kind, InboundKind::Uplink));
        assert_eq!(u.from.as_deref(), Some("codex"));
        assert_eq!(u.body, "hi there");
    }

    #[test]
    fn echoed_reply_instruction_is_not_inbound() {
        // `format_outbound`'s ReplyHint::Full text embeds this literal shape, so
        // a scan-anywhere match would parse an echo of our OWN instruction as a
        // peer reply. Decoration stripping must stay anchored.
        assert!(
            parse_inbound(
                "tmux send-keys -t %0 -l '[reply id:6b3b20e6] 391' \\; send-keys -t %0 Enter"
            )
            .is_none()
        );
    }

    #[test]
    fn correlation_id_is_8_hex() {
        let id = new_correlation_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
