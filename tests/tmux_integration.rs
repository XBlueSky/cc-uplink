mod common;
use cc_uplink::core::driver::Driver;
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

#[tokio::test]
async fn driver_channels_label_read() {
    let Some(srv) = common::TmuxTestServer::start() else {
        return;
    };
    // Run the driver against the private server: point $TMUX/$TMUX_PANE at it.
    // own_context needs TMUX_PANE; use the first pane of session "it".
    let pane = srv
        .run(&["list-panes", "-t", "it", "-F", "#{pane_id}"])
        .trim()
        .to_string();
    unsafe {
        std::env::set_var("TMUX", format!("{},0,0", srv.sock()));
        std::env::set_var("TMUX_PANE", &pane);
    }
    let d = cc_uplink::drivers::tmux::TmuxDriver::new(Default::default())
        .await
        .unwrap();
    let chans = d.channels().await.unwrap();
    assert!(chans.iter().any(|c| c.channel == format!("tmux:{pane}")));

    d.invoke(&pane, "label", serde_json::json!({"name":"selfpane"}))
        .await
        .unwrap();
    let resolved = d.resolve("selfpane").await.unwrap();
    assert_eq!(resolved, pane);

    srv.run(&["send-keys", "-t", "it", "-l", "echo read-marker"]);
    srv.run(&["send-keys", "-t", "it", "Enter"]);
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let out = d
        .invoke(&pane, "read", serde_json::json!({"lines": 20}))
        .await
        .unwrap();
    assert!(out["text"].as_str().unwrap().contains("read-marker"));
}
