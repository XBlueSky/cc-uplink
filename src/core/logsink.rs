//! Best-effort JSONL conversation log sink.
//!
//! Appends one JSON object per line to a shared log file so `cc-uplink log`
//! can tail the conversation history. Every operation here is best-effort:
//! a missing state directory, an unwritable path, or any I/O error is
//! silently swallowed — logging must never fail or block a driver op.

use std::io::Write;
use std::path::PathBuf;

/// Shared log file location: `$XDG_STATE_HOME/cc-uplink/log.jsonl`, falling
/// back to `dirs::data_local_dir()` when no state dir is available (e.g.
/// macOS). Returns `None` if neither is available.
pub fn log_path() -> Option<PathBuf> {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .map(|d| d.join("cc-uplink/log.jsonl"))
}

/// Append-only JSONL sink. Never errors: construction and appends are both
/// best-effort no-ops when the log path or its parent directory can't be
/// resolved/created.
pub struct LogSink {
    path: Option<PathBuf>,
}

impl LogSink {
    pub fn open() -> Self {
        let path = log_path();
        if let Some(p) = &path {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
        }
        Self { path }
    }

    pub fn append(&self, entry: &serde_json::Value) {
        let Some(p) = &self.path else { return };
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
        {
            let _ = writeln!(f, "{entry}");
        }
    }
}
