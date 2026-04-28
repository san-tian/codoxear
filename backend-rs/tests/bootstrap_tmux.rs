use axum::body::Body;
use axum::http::{Request, StatusCode};
use codoxear_backend_rs::app_state::build_state_from_config;
use codoxear_backend_rs::models::BootstrapPayload;
use codoxear_backend_rs::routes::router;
use codoxear_backend_rs::runtime::RuntimeConfig;
use std::fs;
use std::path::PathBuf;
use tower::ServiceExt;

fn temp_app_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "codoxear-bootstrap-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(path.join("socks")).unwrap();
    path
}

#[tokio::test]
async fn bootstrap_exposes_tmux_metadata() {
    let app_dir = temp_app_dir("tmux");
    fs::write(app_dir.join("socks").join("sess-alpha.sock"), "").unwrap();
    fs::write(
        app_dir.join("socks").join("sess-alpha.json"),
        format!(
            r#"{{
          "session_id": "thread-alpha",
          "codex_pid": {},
          "broker_pid": {},
          "agent_backend": "codex",
          "transport": "tmux",
          "cwd": "/work/codoxear",
          "start_ts": 10.0,
          "tmux_session": "codoxear-dev",
          "tmux_window": "nova"
        }}"#,
            std::process::id(),
            std::process::id(),
        ),
    )
    .unwrap();

    let response = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap())
        .oneshot(
            Request::builder()
                .uri("/api/v1/bootstrap")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: BootstrapPayload = serde_json::from_slice(&body).unwrap();
    let session = payload
        .sessions
        .iter()
        .find(|session| session.id == "sess-alpha")
        .unwrap();

    assert_eq!(session.transport.as_deref(), Some("tmux"));
    assert_eq!(session.tmux_session.as_deref(), Some("codoxear-dev"));
    assert_eq!(session.tmux_window.as_deref(), Some("nova"));
}
