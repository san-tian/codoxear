use axum::body::Body;
use axum::http::{Request, StatusCode};
use codoxear_backend_rs::app_state::build_state;
use codoxear_backend_rs::models::BootstrapPayload;
use codoxear_backend_rs::routes::router;
use tower::ServiceExt;

#[tokio::test]
async fn bootstrap_exposes_tmux_metadata() {
    let response = router(build_state())
        .oneshot(Request::builder().uri("/api/v1/bootstrap").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
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
