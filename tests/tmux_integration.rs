mod common;
use cc_uplink::drivers::tmux::control::ControlMode;
use cc_uplink::drivers::tmux::transport::{OneShotCli, TmuxTransport};

#[tokio::test]
async fn one_shot_cli_runs_against_private_server() {
    let Some(srv) = common::TmuxTestServer::start() else {
        return;
    };
    let t = OneShotCli {
        socket: Some(srv.sock()),
    };
    let out = t
        .run(&[
            "display-message".into(),
            "-p".into(),
            "ok-#{session_name}".into(),
        ])
        .await
        .unwrap();
    assert_eq!(out.trim(), "ok-it");
    assert!(t.events().is_none());
}

#[tokio::test]
async fn control_mode_attach_run_and_events() {
    let Some(srv) = common::TmuxTestServer::start() else {
        return;
    };
    let cm = ControlMode::attach(Some(srv.sock()), "it").await.unwrap();
    let out = cm
        .run(&[
            "display-message".into(),
            "-p".into(),
            "cm-#{session_name}".into(),
        ])
        .await
        .unwrap();
    assert_eq!(out.trim(), "cm-it");

    // events: make the pane print something and observe %output
    let mut rx = cm.events().unwrap();
    srv.run(&["send-keys", "-t", "it", "-l", "echo uplink-evt-marker"]);
    srv.run(&["send-keys", "-t", "it", "Enter"]);
    let mut seen = false;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await
        {
            if String::from_utf8_lossy(&ev.data).contains("uplink-evt-marker") {
                seen = true;
                break;
            }
        }
    }
    assert!(seen, "expected %output containing marker");
}
