use std::process::Command;
use tempfile::TempDir;

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
