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

#[tokio::test]
async fn send_delivers_and_verifies() {
    let Some(srv) = common::TmuxTestServer::start() else {
        return;
    };
    let own = srv
        .run(&["list-panes", "-t", "it", "-F", "#{pane_id}"])
        .trim()
        .to_string();
    unsafe {
        std::env::set_var("TMUX", format!("{},0,0", srv.sock()));
        std::env::set_var("TMUX_PANE", &own);
    }
    // second pane runs `cat` — echoes what it receives
    srv.run(&["split-window", "-t", "it", "-d", "cat"]);
    let panes = srv.run(&["list-panes", "-t", "it", "-F", "#{pane_id}"]);
    let target = panes
        .lines()
        .find(|p| p.trim() != own)
        .unwrap()
        .trim()
        .to_string();

    let d = cc_uplink::drivers::tmux::TmuxDriver::new(Default::default())
        .await
        .unwrap();
    let r = d
        .send(
            &target,
            cc_uplink::core::driver::SendRequest {
                message: "ping from claude".into(),
                reply_hint: cc_uplink::core::driver::ReplyHint::None,
            },
        )
        .await
        .unwrap();
    assert!(r.delivered);
    assert_eq!(r.correlation_id.len(), 8);

    // sending to own pane is rejected
    let e = d
        .send(
            &own,
            cc_uplink::core::driver::SendRequest {
                message: "loop".into(),
                reply_hint: cc_uplink::core::driver::ReplyHint::None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(e.kind, cc_uplink::error::ErrorKind::Rejected));

    // multiline is invalid
    let e = d
        .send(
            &target,
            cc_uplink::core::driver::SendRequest {
                message: "a\nb".into(),
                reply_hint: cc_uplink::core::driver::ReplyHint::None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(e.kind, cc_uplink::error::ErrorKind::Invalid));
}

#[tokio::test]
async fn keys_requires_recent_read() {
    let Some(srv) = common::TmuxTestServer::start() else {
        return;
    };
    let own = srv
        .run(&["list-panes", "-t", "it", "-F", "#{pane_id}"])
        .trim()
        .to_string();
    unsafe {
        std::env::set_var("TMUX", format!("{},0,0", srv.sock()));
        std::env::set_var("TMUX_PANE", &own);
    }
    srv.run(&["split-window", "-t", "it", "-d", "cat"]);
    let panes = srv.run(&["list-panes", "-t", "it", "-F", "#{pane_id}"]);
    let target = panes
        .lines()
        .find(|p| p.trim() != own)
        .unwrap()
        .trim()
        .to_string();
    let d = cc_uplink::drivers::tmux::TmuxDriver::new(Default::default())
        .await
        .unwrap();

    let e = d
        .invoke(&target, "keys", serde_json::json!({"keys":["Enter"]}))
        .await
        .unwrap_err();
    assert!(matches!(e.kind, cc_uplink::error::ErrorKind::Rejected));

    d.invoke(&target, "read", serde_json::json!({"lines":5}))
        .await
        .unwrap();
    d.invoke(&target, "keys", serde_json::json!({"keys":["Enter"]}))
        .await
        .unwrap();
}

#[tokio::test]
async fn ask_returns_transcript_delta() {
    let Some(srv) = common::TmuxTestServer::start() else {
        return;
    };
    let own = srv
        .run(&["list-panes", "-t", "it", "-F", "#{pane_id}"])
        .trim()
        .to_string();
    unsafe {
        std::env::set_var("TMUX", format!("{},0,0", srv.sock()));
        std::env::set_var("TMUX_PANE", &own);
    }
    // target pane: a shell that will execute what we send after Enter
    srv.run(&["split-window", "-t", "it", "-d", "sh"]);
    let panes = srv.run(&["list-panes", "-t", "it", "-F", "#{pane_id}"]);
    let target = panes
        .lines()
        .find(|p| p.trim() != own)
        .unwrap()
        .trim()
        .to_string();
    let d = cc_uplink::drivers::tmux::TmuxDriver::new(Default::default())
        .await
        .unwrap();

    // 'ask' a shell: the injected envelope is not a valid command (sh prints error),
    // but the transcript delta must contain both the envelope and the shell's reaction.
    let out = d
        .invoke(
            &target,
            "ask",
            serde_json::json!({
                "message": "echo uplink-ask-answer", "quiet_ms": 800, "timeout_ms": 15000
            }),
        )
        .await
        .unwrap();
    let t = out["transcript"].as_str().unwrap();
    assert!(
        t.contains("uplink-ask-answer") || t.contains("[uplink"),
        "transcript should contain the exchange, got: {t}"
    );
    assert!(out["receipt"]["delivered"].as_bool().unwrap());
}

#[tokio::test]
async fn recv_sees_inbound_reply_envelope() {
    let Some(srv) = common::TmuxTestServer::start() else {
        return;
    };
    let own = srv
        .run(&["list-panes", "-t", "it", "-F", "#{pane_id}"])
        .trim()
        .to_string();
    // Replace whatever interactive shell the test runner's default session
    // command is (its prompt may emit shell-decoration escapes, e.g. the
    // screen/tmux window-title sequence `ESC k <cmd> ESC \`) with a plain
    // `sh`, so the pane's output is the deterministic, undecorated text this
    // test asserts on.
    srv.run(&["respawn-pane", "-k", "-t", &own, "sh"]);
    unsafe {
        std::env::set_var("TMUX", format!("{},0,0", srv.sock()));
        std::env::set_var("TMUX_PANE", &own);
    }
    let d = cc_uplink::drivers::tmux::TmuxDriver::new(Default::default())
        .await
        .unwrap();
    // peer-style reply: typed into our pane, then Enter → shell echoes the line into pane output
    srv.run(&[
        "send-keys",
        "-t",
        &own,
        "-l",
        "echo '[reply id:cafe0001] done'",
    ]);
    srv.run(&["send-keys", "-t", &own, "Enter"]);
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let batch = cc_uplink::core::driver::Driver::recv(&*d, None)
        .await
        .unwrap();
    assert!(
        batch
            .items
            .iter()
            .any(|i| i.id.as_deref() == Some("cafe0001"))
    );
}
