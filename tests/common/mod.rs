use std::process::Command;
use std::sync::OnceLock;
use tempfile::TempDir;
use tokio::sync::{Mutex, MutexGuard};

/// Serializes every test that points `$TMUX`/`$TMUX_PANE` at its own private
/// server. Those variables are process-global, so two such tests running
/// concurrently in this binary would steal each other's driver context (CI
/// passes `--test-threads=1`; this keeps a plain `cargo test` honest too). Hold
/// the guard for as long as the driver built from that env is in use — hence an
/// async mutex, which is safe to hold across the driver's `.await` points.
pub async fn env_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().await
}

pub struct TmuxTestServer {
    dir: TempDir,
}

impl TmuxTestServer {
    pub fn start() -> Option<Self> {
        if Command::new("tmux").arg("-V").output().is_err() {
            eprintln!("SKIP: tmux not installed");
            return None;
        }
        let dir = TempDir::new().unwrap();
        let s = Self { dir };
        s.run(&["new-session", "-d", "-x", "180", "-y", "45", "-s", "it"]);
        Some(s)
    }
    pub fn sock(&self) -> String {
        self.dir.path().join("sock").to_string_lossy().into_owned()
    }
    pub fn run(&self, args: &[&str]) -> String {
        let out = Command::new("tmux")
            .arg("-S")
            .arg(self.sock())
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for TmuxTestServer {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .arg("-S")
            .arg(self.sock())
            .args(["kill-server"])
            .output();
    }
}
