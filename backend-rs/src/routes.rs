use crate::app_state::{epoch_now, AppState};
use crate::models::{
    ApiDiagnosticsResponse, ApiHarnessResponse, ApiQueueResponse, EventKind, LiveEvent, SendMessagePayload,
    TranscriptEvent,
};
use crate::runtime::{
    load_diagnostics_response, load_file_blob, load_file_read_response, load_harness_response,
    load_messages_history, load_messages_live, load_messages_tail, load_queue_response, load_sessions_response,
};
use axum::extract::{Path, Query, State};
use axum::body::Body;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::time::Duration;
use tokio::time::sleep;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/bootstrap", get(bootstrap))
        .route("/api/v1/sessions", get(sessions))
        .route("/api/v1/sessions/:session_id/diagnostics", get(diagnostics))
        .route("/api/v1/sessions/:session_id/queue", get(queue))
        .route("/api/v1/sessions/:session_id/harness", get(harness))
        .route("/api/v1/sessions/:session_id/file/read", get(file_read))
        .route("/api/v1/sessions/:session_id/file/blob", get(file_blob))
        .route("/api/v1/sessions/:session_id/messages/tail", get(messages_tail))
        .route("/api/v1/sessions/:session_id/messages/history", get(messages_history))
        .route("/api/v1/sessions/:session_id/messages/live", get(messages_live))
        .route("/api/v1/messages/send", post(send_message))
        .route("/api/v1/events/stream", get(events))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "service": "codoxear-backend-rs" }))
}

async fn bootstrap(State(state): State<AppState>) -> Json<crate::models::BootstrapPayload> {
    let store = state.store.read().await;
    Json(store.bootstrap())
}

async fn sessions(State(state): State<AppState>) -> Result<Json<crate::models::ApiSessionsResponse>, (StatusCode, String)> {
    load_sessions_response(&state.config)
        .map(Json)
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))
}

async fn diagnostics(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<ApiDiagnosticsResponse>, (StatusCode, String)> {
    load_diagnostics_response(&state.config, &session_id)
        .map(Json)
        .map_err(route_error)
}

async fn queue(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<ApiQueueResponse>, (StatusCode, String)> {
    load_queue_response(&state.config, &session_id)
        .map(Json)
        .map_err(route_error)
}

async fn harness(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<ApiHarnessResponse>, (StatusCode, String)> {
    load_harness_response(&state.config, &session_id)
        .map(Json)
        .map_err(route_error)
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct FilePathQuery {
    path: String,
}

#[derive(Deserialize)]
struct HistoryQuery {
    cursor: String,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct LiveQuery {
    cursor: String,
}

async fn messages_tail(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<LimitQuery>,
) -> Result<Json<crate::models::ApiMessagesTailResponse>, (StatusCode, String)> {
    load_messages_tail(&state.config, &session_id, query.limit.unwrap_or(120))
        .map(Json)
        .map_err(route_error)
}

async fn messages_history(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<crate::models::ApiMessagesHistoryResponse>, (StatusCode, String)> {
    load_messages_history(&state.config, &session_id, &query.cursor, query.limit.unwrap_or(60))
        .map(Json)
        .map_err(route_error)
}

async fn messages_live(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<LiveQuery>,
) -> Result<Json<crate::models::ApiMessagesLiveResponse>, (StatusCode, String)> {
    load_messages_live(&state.config, &session_id, &query.cursor)
        .map(Json)
        .map_err(route_error)
}

async fn file_read(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<FilePathQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    load_file_read_response(&state.config, &session_id, &query.path)
        .map(Json)
        .map_err(route_error)
}

async fn file_blob(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<FilePathQuery>,
) -> Result<Response, (StatusCode, String)> {
    let (raw, content_type) = load_file_blob(&state.config, &session_id, &query.path).map_err(route_error)?;
    let mut response = Response::new(Body::from(raw.clone()));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type).map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?,
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&raw.len().to_string()).map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?,
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(header::EXPIRES, HeaderValue::from_static("0"));
    Ok(response)
}

fn route_error(message: String) -> (StatusCode, String) {
    if message.starts_with("unknown session:") || message == "file not found" {
        return (StatusCode::NOT_FOUND, message);
    }
    if message == "permission denied" {
        return (StatusCode::FORBIDDEN, message);
    }
    if message == "path required"
        || message == "invalid path"
        || message == "path is not a file"
        || message == "file is not previewable inline"
        || message == "binary file not supported"
        || message.starts_with("file too large")
        || message.starts_with("invalid message cursor:")
    {
        return (StatusCode::BAD_REQUEST, message);
    }
    (StatusCode::INTERNAL_SERVER_ERROR, message)
}

async fn send_message(
    State(state): State<AppState>,
    Json(payload): Json<SendMessagePayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let now = epoch_now();
    let user_event = TranscriptEvent {
        id: format!("evt-user-{now:.3}"),
        kind: EventKind::User,
        title: Some("Queued from composer".into()),
        body: payload.text.clone(),
        meta: Some("sent to Rust preview backend".into()),
        created_at: now,
    };

    let session_snapshot = {
        let mut store = state.store.write().await;
        let detail = store
            .session_details
            .get_mut(&payload.session_id)
            .ok_or((StatusCode::NOT_FOUND, "unknown session".to_string()))?;
        detail.transcript.push(user_event.clone());

        let session = store
            .sessions
            .iter_mut()
            .find(|session| session.id == payload.session_id)
            .ok_or((StatusCode::NOT_FOUND, "unknown session".to_string()))?;
        session.last_line = payload.text.clone();
        session.status = "running".into();
        session.updated_at = now;
        session.unread = 0;
        session.clone()
    };

    let _ = state.tx.send(LiveEvent::MessageCreated {
        session_id: payload.session_id.clone(),
        event: user_event,
        session: session_snapshot.clone(),
    });

    let spawned_state = state.clone();
    let spawned_session_id = payload.session_id.clone();
    let spawned_text = payload.text.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(420)).await;
        let now = epoch_now();
        let assistant_event = TranscriptEvent {
            id: format!("evt-assistant-{now:.3}"),
            kind: EventKind::Assistant,
            title: Some("Preview assistant".into()),
            body: format!(
                "Captured the new message and rebroadcast it over SSE. Next step is to replace this simulated response with real broker/session plumbing.\n\nLast user request: {spawned_text}"
            ),
            meta: Some("synthetic assistant event".into()),
            created_at: now,
        };

        let session_snapshot = {
            let mut store = spawned_state.store.write().await;
            let Some(detail) = store.session_details.get_mut(&spawned_session_id) else {
                return;
            };
            detail.transcript.push(assistant_event.clone());
            let Some(session) = store.sessions.iter_mut().find(|session| session.id == spawned_session_id) else {
                return;
            };
            session.last_line = "Synthetic assistant response emitted from Rust preview backend.".into();
            session.status = "idle".into();
            session.updated_at = now;
            session.clone()
        };

        let _ = spawned_state.tx.send(LiveEvent::MessageCreated {
            session_id: spawned_session_id,
            event: assistant_event,
            session: session_snapshot,
        });
    });

    Ok(Json(json!({ "ok": true })))
}

async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.tx.subscribe();
    let connected = stream::once(async move {
        let event = LiveEvent::Connected {
            created_at: epoch_now(),
        };
        Ok(Event::default().data(serde_json::to_string(&event).unwrap()))
    });
    let broadcast_stream = BroadcastStream::new(receiver).filter_map(|event| {
        match event {
            Ok(payload) => Some(Ok(Event::default().data(serde_json::to_string(&payload).unwrap()))),
            Err(_) => None,
        }
    });

    Sse::new(connected.chain(broadcast_stream)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keep-alive"),
    )
}

#[cfg(test)]
mod tests {
    use super::router;
    use crate::app_state::{build_state, build_state_from_config};
    use crate::runtime::RuntimeConfig;
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Request, StatusCode};
    use serde_json::Value;
    use std::fs;
    use std::path::PathBuf;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_route_responds() {
      let app = router(build_state());
      let response = app
          .oneshot(Request::builder().uri("/api/v1/health").body(Body::empty()).unwrap())
          .await
          .unwrap();
      assert_eq!(response.status(), StatusCode::OK);
    }

    fn temp_app_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "codoxear-routes-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(path.join("socks")).unwrap();
        path
    }

    #[tokio::test]
    async fn sessions_route_returns_runtime_sessions() {
        let app_dir = temp_app_dir("sessions");
        fs::write(app_dir.join("socks").join("sid-route.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-route.json"),
            r#"{"session_id":"thread-route","codex_pid":1,"broker_pid":2,"agent_backend":"pi","cwd":"/repo","start_ts":11.0}"#,
        )
        .unwrap();
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["sessions"][0]["session_id"], "sid-route");
        assert_eq!(payload["sessions"][0]["thread_id"], "thread-route");
        assert_eq!(payload["sessions"][0]["agent_backend"], "pi");
    }

    #[tokio::test]
    async fn messages_tail_route_returns_runtime_events() {
        let app_dir = temp_app_dir("messages");
        let log_path = app_dir.join("rollout.jsonl");
        fs::write(
            &log_path,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"hello"},"ts":1.0}
{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}],"phase":"final_answer"},"ts":2.0}
"#,
        )
        .unwrap();
        fs::write(app_dir.join("socks").join("sid-tail.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-tail.json"),
            format!(
                r#"{{"session_id":"thread-tail","codex_pid":1,"broker_pid":2,"cwd":"/repo","log_path":"{}","start_ts":11.0}}"#,
                log_path.display()
            ),
        )
        .unwrap();
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-tail/messages/tail?limit=10")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["events"][0]["text"], "hello");
        assert_eq!(payload["events"][1]["message_class"], "final_response");
    }

    #[tokio::test]
    async fn session_support_routes_return_runtime_data() {
        let app_dir = temp_app_dir("support");
        let repo_dir = app_dir.join("repo");
        fs::create_dir_all(&repo_dir).unwrap();
        fs::write(repo_dir.join("notes.txt"), "hello from rust\n").unwrap();
        fs::write(repo_dir.join("image.png"), b"\x89PNG\r\n\x1a\nrest").unwrap();
        fs::write(app_dir.join("socks").join("sid-support.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-support.json"),
            format!(
                r#"{{"session_id":"thread-support","codex_pid":11,"broker_pid":22,"agent_backend":"codex","owner":"web","cwd":"{}","start_ts":10.0,"updated_ts":20.0}}"#,
                repo_dir.display()
            ),
        )
        .unwrap();
        fs::write(
            app_dir.join("session_queues.json"),
            r#"{"sid-support":[{"id":"q1","text":"queued work","created_ts":42.0,"sending":true}]}"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("harness.json"),
            r#"{"sid-support":{"enabled":true,"request":"keep going","cooldown_minutes":5,"remaining_injections":2}}"#,
        )
        .unwrap();
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let diagnostics = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-support/diagnostics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(diagnostics.status(), StatusCode::OK);
        let diagnostics_body = to_bytes(diagnostics.into_body(), usize::MAX).await.unwrap();
        let diagnostics_payload: Value = serde_json::from_slice(&diagnostics_body).unwrap();
        assert_eq!(diagnostics_payload["session_id"], "sid-support");
        assert_eq!(diagnostics_payload["queue_len"], 1);

        let queue = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-support/queue")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(queue.status(), StatusCode::OK);
        let queue_body = to_bytes(queue.into_body(), usize::MAX).await.unwrap();
        let queue_payload: Value = serde_json::from_slice(&queue_body).unwrap();
        assert_eq!(queue_payload["items"][0]["text"], "queued work");
        assert_eq!(queue_payload["items"][0]["sending"], true);

        let harness = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-support/harness")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(harness.status(), StatusCode::OK);
        let harness_body = to_bytes(harness.into_body(), usize::MAX).await.unwrap();
        let harness_payload: Value = serde_json::from_slice(&harness_body).unwrap();
        assert_eq!(harness_payload["request"], "keep going");

        let file = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-support/file/read?path=notes.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(file.status(), StatusCode::OK);
        let file_body = to_bytes(file.into_body(), usize::MAX).await.unwrap();
        let file_payload: Value = serde_json::from_slice(&file_body).unwrap();
        assert_eq!(file_payload["kind"], "text");
        assert_eq!(file_payload["text"], "hello from rust\n");

        let image = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-support/file/read?path=image.png")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(image.status(), StatusCode::OK);
        let image_body = to_bytes(image.into_body(), usize::MAX).await.unwrap();
        let image_payload: Value = serde_json::from_slice(&image_body).unwrap();
        assert_eq!(image_payload["kind"], "image");

        let blob = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-support/file/blob?path=image.png")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(blob.status(), StatusCode::OK);
        assert_eq!(blob.headers()[header::CONTENT_TYPE], "image/png");
    }
}
