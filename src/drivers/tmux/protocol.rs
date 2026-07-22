//! Pure parsing functions for tmux control-mode (`tmux -CC`) protocol lines.
//!
//! No I/O, no tmux process — everything here operates on already-read
//! strings/bytes and is exhaustively unit-tested in this module.

/// A single parsed line of tmux control-mode output.
#[derive(Debug)]
pub enum CmLine {
    /// `%begin <ts> <seq> <flags>` — start of a command reply block.
    Begin { seq: u64 },
    /// `%end <ts> <seq> <flags>` — end of a command reply block.
    End { seq: u64 },
    /// `%error <ts> <seq> <flags>` — command reply block ended in error.
    CmdError { seq: u64 },
    /// `%output <pane> <octal-escaped bytes>` — pane output, unescaped.
    Output { pane: String, data: Vec<u8> },
    /// `%pause <pane>` — pane output paused (pause-after limit reached).
    Pause { pane: String },
    /// `%exit` — tmux control-mode client is detaching.
    Exit,
    /// Any other `%`-prefixed notification line, verbatim.
    Notification(String),
    /// A plain (non-`%`-prefixed) body line, e.g. inside a begin/end block.
    Body(String),
}

/// Extracts the sequence number (second whitespace-delimited field) from the
/// remainder of a `%begin`/`%end`/`%error` line after its prefix is stripped.
fn seq_of(rest: &str) -> u64 {
    rest.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Parses one line of tmux control-mode output into a [`CmLine`].
pub fn parse_line(line: &str) -> CmLine {
    if let Some(r) = line.strip_prefix("%begin ") {
        return CmLine::Begin { seq: seq_of(r) };
    }
    if let Some(r) = line.strip_prefix("%end ") {
        return CmLine::End { seq: seq_of(r) };
    }
    if let Some(r) = line.strip_prefix("%error ") {
        return CmLine::CmdError { seq: seq_of(r) };
    }
    if let Some(r) = line.strip_prefix("%output ") {
        if let Some((pane, data)) = r.split_once(' ') {
            return CmLine::Output {
                pane: pane.to_string(),
                data: unescape_octal(data),
            };
        }
    }
    if let Some(r) = line.strip_prefix("%pause ") {
        return CmLine::Pause {
            pane: r.trim().to_string(),
        };
    }
    if line == "%exit" || line.starts_with("%exit ") {
        return CmLine::Exit;
    }
    if line.starts_with('%') {
        return CmLine::Notification(line.to_string());
    }
    CmLine::Body(line.to_string())
}

/// Unescapes tmux control-mode octal byte escapes (`\NNN`, exactly three
/// octal digits) and escaped backslashes (`\\`) into raw bytes. Any other
/// byte is copied through unchanged.
pub fn unescape_octal(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            if i + 3 < b.len()
                && b[i + 1].is_ascii_digit()
                && b[i + 2].is_ascii_digit()
                && b[i + 3].is_ascii_digit()
            {
                let oct = std::str::from_utf8(&b[i + 1..i + 4]).unwrap_or("0");
                if let Ok(v) = u8::from_str_radix(oct, 8) {
                    out.push(v);
                    i += 4;
                    continue;
                }
            }
            if i + 1 < b.len() && b[i + 1] == b'\\' {
                out.push(b'\\');
                i += 2;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// Strips ANSI CSI (`ESC [ ... final-byte`) and OSC (`ESC ] ... BEL` or
/// `ESC ] ... ESC \`) escape sequences from raw bytes, keeping printable
/// text. Decodes the remainder as lossy UTF-8.
pub fn strip_ansi(data: &[u8]) -> String {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x1b {
            if i + 1 < data.len() && data[i + 1] == b'[' {
                i += 2;
                while i < data.len() && !(0x40..=0x7e).contains(&data[i]) {
                    i += 1;
                }
                i += 1; // final byte
                continue;
            }
            if i + 1 < data.len() && data[i + 1] == b']' {
                i += 2;
                while i < data.len() && data[i] != 0x07 {
                    if data[i] == 0x1b && i + 1 < data.len() && data[i + 1] == b'\\' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                i += 1;
                continue;
            }
            i += 2; // other short escape
            continue;
        }
        out.push(data[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Single-quote-wraps a string for safe use as a tmux control-mode command
/// argument, escaping embedded single quotes as `'\''`.
pub fn quote_cm_arg(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_begin_end_error() {
        assert!(matches!(
            parse_line("%begin 1721600000 42 1"),
            CmLine::Begin { seq: 42 }
        ));
        assert!(matches!(
            parse_line("%end 1721600000 42 1"),
            CmLine::End { seq: 42 }
        ));
        assert!(matches!(
            parse_line("%error 1721600000 42 1"),
            CmLine::CmdError { seq: 42 }
        ));
    }

    #[test]
    fn parses_output_with_octal() {
        match parse_line(r"%output %3 hello\040world\134x") {
            CmLine::Output { pane, data } => {
                assert_eq!(pane, "%3");
                assert_eq!(data, b"hello world\\x");
            }
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn parses_pause_exit_notification_body() {
        assert!(matches!(parse_line("%pause %3"), CmLine::Pause { .. }));
        assert!(matches!(parse_line("%exit"), CmLine::Exit));
        assert!(matches!(
            parse_line("%session-changed $1 main"),
            CmLine::Notification(_)
        ));
        assert!(matches!(parse_line("plain body line"), CmLine::Body(_)));
    }

    #[test]
    fn strips_ansi() {
        let s = strip_ansi(b"\x1b[1;32mgreen\x1b[0m id:ab12cd34 \x1b]0;title\x07tail");
        assert_eq!(s, "green id:ab12cd34 tail");
    }

    #[test]
    fn quotes_cm_arg() {
        assert_eq!(quote_cm_arg("plain"), "'plain'");
        assert_eq!(quote_cm_arg("it's"), r"'it'\''s'");
    }
}
