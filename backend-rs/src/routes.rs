use crate::app_state::{epoch_now, AppState};
use crate::models::{
    ApiChangedFilesResponse, ApiDiagnosticsResponse, ApiFileSearchResponse, ApiGitDiffResponse,
    ApiGitFileVersionsResponse, ApiHarnessResponse, ApiQueueResponse, EventKind, LiveEvent,
    SendMessagePayload, TranscriptEvent,
};
use crate::runtime::{
    create_session, create_session_request_from_payload,
    default_file_search_limit, load_changed_files_response, load_diagnostics_response,
    load_file_blob, load_file_read_response, load_file_search_response, load_git_diff_response,
    load_git_file_versions_response, load_harness_response, load_messages_history,
    load_messages_live, load_messages_tail, load_queue_response, load_sessions_response,
    delete_queue_item, delete_session, edit_session, enqueue_session_message,
    inject_session_attachment, interrupt_session, move_queue_item, rename_session,
    set_harness_config,
    send_session_message, update_queue_item,
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
        .route("/api/v1/sessions", get(sessions).post(session_create))
        .route("/api/v1/sessions/:session_id/diagnostics", get(diagnostics))
        .route("/api/v1/sessions/:session_id/queue", get(queue))
        .route("/api/v1/sessions/:session_id/enqueue", post(session_enqueue))
        .route("/api/v1/sessions/:session_id/queue/delete", post(queue_delete))
        .route("/api/v1/sessions/:session_id/queue/update", post(queue_update))
        .route("/api/v1/sessions/:session_id/queue/move", post(queue_move))
        .route("/api/v1/sessions/:session_id/rename", post(session_rename))
        .route("/api/v1/sessions/:session_id/edit", post(session_edit))
        .route("/api/v1/sessions/:session_id/delete", post(session_delete))
        .route("/api/v1/sessions/:session_id/inject_file", post(session_inject_attachment))
        .route("/api/v1/sessions/:session_id/inject_image", post(session_inject_attachment))
        .route("/api/v1/sessions/:session_id/harness", get(harness).post(session_harness))
        .route("/api/v1/sessions/:session_id/git/changed_files", get(changed_files))
        .route("/api/v1/sessions/:session_id/git/diff", get(git_diff))
        .route("/api/v1/sessions/:session_id/git/file_versions", get(git_file_versions))
        .route("/api/v1/sessions/:session_id/file/read", get(file_read))
        .route("/api/v1/sessions/:session_id/file/search", get(file_search))
        .route("/api/v1/sessions/:session_id/file/blob", get(file_blob))
        .route("/api/v1/sessions/:session_id/messages/tail", get(messages_tail))
        .route("/api/v1/sessions/:session_id/messages/history", get(messages_history))
        .route("/api/v1/sessions/:session_id/messages/live", get(messages_live))
        .route("/api/v1/sessions/:session_id/send", post(session_send))
        .route("/api/v1/sessions/:session_id/interrupt", post(session_interrupt))
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

async fn session_create(
    State(state): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let request = create_session_request_from_payload(&payload).map_err(create_session_error_response)?;
    create_session(&state.config, request)
        .map(Json)
        .map_err(create_session_error_response)
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

async fn session_harness(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<ApiHarnessResponse>, (StatusCode, String)> {
    if !payload.is_object() {
        return Err((StatusCode::BAD_REQUEST, "invalid json body (expected object)".to_string()));
    }
    set_harness_config(
        &state.config,
        &session_id,
        payload.get("enabled"),
        payload.get("request"),
        payload.get("cooldown_minutes"),
        payload.get("remaining_injections"),
        payload.get("text").is_some(),
    )
    .map(Json)
    .map_err(route_error)
}

async fn changed_files(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<ApiChangedFilesResponse>, (StatusCode, String)> {
    load_changed_files_response(&state.config, &session_id)
        .map(Json)
        .map_err(git_route_error)
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
struct FileSearchQuery {
    q: Option<String>,
    limit: Option<String>,
}

#[derive(Deserialize)]
struct GitDiffQuery {
    path: String,
    staged: Option<String>,
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

#[derive(Deserialize)]
struct TextPayload {
    text: String,
}

#[derive(Deserialize)]
struct InjectAttachmentPayload {
    filename: String,
    data_b64: String,
    attachment_index: i64,
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

async fn session_send(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(payload): Json<TextPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if payload.text.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "text required".to_string()));
    }
    send_session_message(&state.config, &session_id, &payload.text)
        .map(Json)
        .map_err(route_error)
}

async fn session_interrupt(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    interrupt_session(&state.config, &session_id)
        .map(Json)
        .map_err(route_error)
}

async fn session_rename(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let Some(name) = payload.get("name").and_then(serde_json::Value::as_str) else {
        return Err((StatusCode::BAD_REQUEST, "name required".to_string()));
    };
    rename_session(&state.config, &session_id, name)
        .map(Json)
        .map_err(route_error)
}

async fn session_delete(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    delete_session(&state.config, &session_id)
        .map(Json)
        .map_err(route_error)
}

async fn session_edit(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let Some(name) = payload.get("name").and_then(serde_json::Value::as_str) else {
        return Err((StatusCode::BAD_REQUEST, "name required".to_string()));
    };
    edit_session(
        &state.config,
        &session_id,
        name,
        payload.get("priority_offset"),
        payload.get("snooze_until"),
        payload.get("dependency_session_id"),
    )
    .map(Json)
    .map_err(route_error)
}

async fn session_inject_attachment(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(payload): Json<InjectAttachmentPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inject_session_attachment(
        &state.config,
        &session_id,
        &payload.filename,
        &payload.data_b64,
        payload.attachment_index,
    )
    .map(Json)
    .map_err(route_error)
}

async fn session_enqueue(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let Some(text) = payload.get("text").and_then(serde_json::Value::as_str) else {
        return Err((StatusCode::BAD_REQUEST, "text required".to_string()));
    };
    if text.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "text required".to_string()));
    }
    enqueue_session_message(&state.config, &session_id, text)
        .map(Json)
        .map_err(queue_action_error)
}

async fn queue_delete(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let Some(item_id) = payload.get("id").and_then(serde_json::Value::as_str) else {
        return Err((StatusCode::BAD_REQUEST, "id required".to_string()));
    };
    if item_id.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "id required".to_string()));
    }
    delete_queue_item(&state.config, &session_id, item_id)
        .map(Json)
        .map_err(queue_action_error)
}

async fn queue_update(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let Some(item_id) = payload.get("id").and_then(serde_json::Value::as_str) else {
        return Err((StatusCode::BAD_REQUEST, "id required".to_string()));
    };
    if item_id.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "id required".to_string()));
    }
    let Some(text) = payload.get("text").and_then(serde_json::Value::as_str) else {
        return Err((StatusCode::BAD_REQUEST, "text required".to_string()));
    };
    if text.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "text required".to_string()));
    }
    update_queue_item(&state.config, &session_id, item_id, text)
        .map(Json)
        .map_err(queue_action_error)
}

async fn queue_move(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let Some(item_id) = payload.get("id").and_then(serde_json::Value::as_str) else {
        return Err((StatusCode::BAD_REQUEST, "id required".to_string()));
    };
    if item_id.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "id required".to_string()));
    }
    let Some(to_index) = payload.get("to_index").and_then(serde_json::Value::as_i64) else {
        return Err((StatusCode::BAD_REQUEST, "to_index required".to_string()));
    };
    move_queue_item(&state.config, &session_id, item_id, to_index)
        .map(Json)
        .map_err(queue_action_error)
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

async fn file_search(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<FileSearchQuery>,
) -> Result<Json<ApiFileSearchResponse>, (StatusCode, String)> {
    let raw_query = query.q.unwrap_or_default();
    if raw_query.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "q required".to_string()));
    }
    let default_limit = default_file_search_limit();
    let limit = match query.limit.as_deref() {
        None => default_limit,
        Some(raw) => {
            let trimmed = raw.trim();
            let parsed = if trimmed.is_empty() {
                default_limit as i64
            } else {
                trimmed
                    .parse::<i64>()
                    .map_err(|_| (StatusCode::BAD_REQUEST, "limit must be an integer".to_string()))?
            };
            if parsed < 1 {
                return Err((StatusCode::BAD_REQUEST, "limit must be >= 1".to_string()));
            }
            parsed as usize
        }
    };
    load_file_search_response(&state.config, &session_id, &raw_query, limit)
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

async fn git_diff(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<GitDiffQuery>,
) -> Result<Json<ApiGitDiffResponse>, (StatusCode, String)> {
    let staged = matches!(query.staged.as_deref(), Some("1"));
    load_git_diff_response(&state.config, &session_id, &query.path, staged)
        .map(Json)
        .map_err(git_route_error)
}

async fn git_file_versions(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<FilePathQuery>,
) -> Result<Json<ApiGitFileVersionsResponse>, (StatusCode, String)> {
    load_git_file_versions_response(&state.config, &session_id, &query.path)
        .map(Json)
        .map_err(git_route_error)
}

fn route_error(message: String) -> (StatusCode, String) {
    if message.starts_with("unknown session:") || message == "file not found" {
        return (StatusCode::NOT_FOUND, message);
    }
    if message == "permission denied" {
        return (StatusCode::FORBIDDEN, message);
    }
    if message == "path required"
        || message == "invalid json body (expected object)"
        || message == "unknown field: text (use request)"
        || message == "request must be a string"
        || message == "harness cooldown_minutes must be an integer"
        || message == "harness cooldown_minutes must be at least 1"
        || message == "harness remaining_injections must be an integer"
        || message == "harness remaining_injections must be at least 0"
        || message == "priority_offset must be a number"
        || message == "priority_offset must be finite"
        || message == "priority_offset must be within [-1, 1]"
        || message == "snooze_until must be a unix timestamp or null"
        || message == "snooze_until must be finite"
        || message == "dependency_session_id must be a string or null"
        || message == "session cannot depend on itself"
        || message == "dependency session not found"
        || message == "filename required"
        || message == "data_b64 required"
        || message == "invalid base64"
        || message == "attachment_index must be >= 1"
        || message == "q required"
        || message == "query required"
        || message == "limit must be an integer"
        || message == "limit must be >= 1"
        || message == "invalid path"
        || message == "path is outside git repo"
        || message == "session cwd is not a directory"
        || message == "path is not a file"
        || message == "file is not previewable inline"
        || message == "binary file not supported"
        || message.starts_with("file too large")
        || message.starts_with("git ls-files failed")
        || message.starts_with("invalid message cursor:")
    {
        return (StatusCode::BAD_REQUEST, message);
    }
    if message.starts_with("file too large") {
        return (StatusCode::PAYLOAD_TOO_LARGE, message);
    }
    if message == "session cwd not found" {
        return (StatusCode::NOT_FOUND, message);
    }
    (StatusCode::INTERNAL_SERVER_ERROR, message)
}

fn git_route_error(message: String) -> (StatusCode, String) {
    if message.contains("not a git repository") {
        return (StatusCode::CONFLICT, message);
    }
    route_error(message)
}

fn queue_action_error(message: String) -> (StatusCode, String) {
    if message.starts_with("unknown session:") {
        return (StatusCode::NOT_FOUND, message);
    }
    if message == "text required" || message == "id required" || message == "to_index required" {
        return (StatusCode::BAD_REQUEST, message);
    }
    (StatusCode::BAD_GATEWAY, message)
}

fn create_session_error_response(error: crate::runtime::CreateSessionError) -> (StatusCode, Json<serde_json::Value>) {
    let status = if error.is_bad_request() {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    let mut payload = json!({ "error": error.message() });
    if let Some(field) = error.field() {
        payload["field"] = json!(field);
    }
    (status, Json(payload))
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
    use std::env;
    use std::fs;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::thread;
    use std::time::{Duration, Instant};
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

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "codoxear-routes-extra-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: impl AsRef<str>) -> Self {
            let previous = env::var(key).ok();
            env::set_var(key, value.as_ref());
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => env::set_var(self.key, value),
                None => env::remove_var(self.key),
            }
        }
    }

    fn write_executable(path: &Path, script: &str) {
        fs::write(path, script).unwrap();
        #[allow(clippy::permissions_set_readonly_false)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).unwrap();
        }
    }

    fn kill_pid(pid: i64) {
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status();
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
    async fn session_create_route_spawns_web_broker() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("create-session");
        let repo_root = temp_dir("repo-root");
        let cwd = repo_root.join("workspace");
        let log_path = repo_root.join("spawn.log");
        let python_path = repo_root.join("fake-python.sh");
        write_executable(
            &python_path,
            "#!/usr/bin/env bash\nset -euo pipefail\n{\n  printf 'PWD=%s\\n' \"$PWD\"\n  printf 'ARGV=%s\\n' \"$*\"\n  printf 'OWNER=%s\\n' \"${CODEX_WEB_OWNER-}\"\n  printf 'BACKEND=%s\\n' \"${CODEX_WEB_AGENT_BACKEND-}\"\n  printf 'MODEL=%s\\n' \"${CODEX_WEB_MODEL-}\"\n  printf 'EFFORT=%s\\n' \"${CODEX_WEB_REASONING_EFFORT-}\"\n} >> \"${CODOXEAR_TEST_LOG}\"\nsleep 30\n",
        );
        let _repo_root = EnvGuard::set("CODOXEAR_REPO_ROOT", repo_root.display().to_string());
        let _python = EnvGuard::set("CODOXEAR_PYTHON_BIN", python_path.display().to_string());
        let _log = EnvGuard::set("CODOXEAR_TEST_LOG", log_path.display().to_string());
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"cwd":"{}","model":"gpt-5.4","reasoning_effort":"xhigh"}}"#,
                        cwd.display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let payload: Value = serde_json::from_slice(&body).unwrap();
        let broker_pid = payload["broker_pid"].as_i64().unwrap();

        thread::sleep(Duration::from_millis(150));
        let log = fs::read_to_string(&log_path).unwrap();
        assert!(cwd.is_dir());
        assert!(log.contains(&format!("PWD={}", repo_root.display())));
        assert!(log.contains(&format!("ARGV=-m codoxear.broker --cwd {} --", cwd.display())));
        assert!(log.contains("OWNER=web"));
        assert!(log.contains("BACKEND=codex"));
        assert!(log.contains("MODEL=gpt-5.4"));
        assert!(log.contains("EFFORT=xhigh"));

        kill_pid(broker_pid);
    }

    #[tokio::test]
    async fn session_create_route_creates_git_worktree_before_spawn() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("create-worktree");
        let repo_root = temp_dir("repo-root-worktree");
        let workspace = repo_root.join("repo");
        fs::create_dir_all(&workspace).unwrap();
        assert!(std::process::Command::new("git")
            .current_dir(&workspace)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .current_dir(&workspace)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .current_dir(&workspace)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap()
            .success());
        fs::write(workspace.join("README.md"), "base\n").unwrap();
        assert!(std::process::Command::new("git")
            .current_dir(&workspace)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .current_dir(&workspace)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success());

        let log_path = repo_root.join("worktree.log");
        let python_path = repo_root.join("fake-python.sh");
        write_executable(
            &python_path,
            "#!/usr/bin/env bash\nset -euo pipefail\nprintf 'ARGV=%s\\n' \"$*\" >> \"${CODOXEAR_TEST_LOG}\"\nsleep 30\n",
        );
        let _repo_root = EnvGuard::set("CODOXEAR_REPO_ROOT", repo_root.display().to_string());
        let _python = EnvGuard::set("CODOXEAR_PYTHON_BIN", python_path.display().to_string());
        let _log = EnvGuard::set("CODOXEAR_TEST_LOG", log_path.display().to_string());
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let branch = "feature/test-worktree";
        let expected_worktree = workspace.parent().unwrap().join("repo-feature-test-worktree");
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"cwd":"{}","worktree_branch":"{}"}}"#,
                        workspace.display(),
                        branch,
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let payload: Value = serde_json::from_slice(&body).unwrap();
        let broker_pid = payload["broker_pid"].as_i64().unwrap();

        thread::sleep(Duration::from_millis(150));
        let log = fs::read_to_string(&log_path).unwrap();
        assert!(expected_worktree.exists());
        assert!(log.contains(&format!("--cwd {} --", expected_worktree.display())));
        let branch_name = std::process::Command::new("git")
            .current_dir(&expected_worktree)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&branch_name.stdout).trim(), branch);

        kill_pid(broker_pid);
    }

    #[tokio::test]
    async fn session_create_route_rejects_missing_resume_session() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("create-resume-missing");
        let workspace = temp_dir("resume-missing");
        let repo_root = temp_dir("resume-missing-root");
        let _repo_root = EnvGuard::set("CODOXEAR_REPO_ROOT", repo_root.display().to_string());
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"cwd":"{}","resume_session_id":"missing"}}"#,
                        workspace.display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["error"], "resume session not found for cwd: missing");
    }

    #[tokio::test]
    async fn session_create_route_can_start_in_tmux() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("create-tmux");
        let repo_root = temp_dir("repo-root-tmux");
        let cwd = repo_root.join("workspace");
        let tmux_log = repo_root.join("tmux.log");
        let tmux_path = repo_root.join("fake-tmux.sh");
        write_executable(
            &tmux_path,
            "#!/usr/bin/env bash\nset -euo pipefail\ncmd=\"${1-}\"\nif [[ \"$cmd\" == \"-V\" ]]; then\n  echo 'tmux 3.4'\n  exit 0\nfi\nif [[ \"$cmd\" == \"has-session\" ]]; then\n  exit 1\nfi\nif [[ \"$cmd\" == \"new-session\" || \"$cmd\" == \"new-window\" ]]; then\n  shell_cmd=\"${@: -1}\"\n  printf 'SHELL=%s\\n' \"$shell_cmd\" >> \"${CODOXEAR_TEST_TMUX_LOG}\"\n  nonce=$(printf '%s' \"$shell_cmd\" | sed -n 's/.*CODEX_WEB_SPAWN_NONCE=\\([^ ]*\\).*/\\1/p')\n  printf '{\"spawn_nonce\":\"%s\",\"broker_pid\":7777}\\n' \"$nonce\" > \"${CODOXEAR_APP_DIR}/socks/spawn.json\"\n  printf '%%8\\n'\n  exit 0\nfi\nif [[ \"$cmd\" == \"capture-pane\" ]]; then\n  printf 'booting\\n'\n  exit 0\nfi\nexit 1\n",
        );
        let _app_dir = EnvGuard::set("CODOXEAR_APP_DIR", app_dir.display().to_string());
        let _repo_root = EnvGuard::set("CODOXEAR_REPO_ROOT", repo_root.display().to_string());
        let _tmux = EnvGuard::set("CODOXEAR_TMUX_BIN", tmux_path.display().to_string());
        let _tmux_log = EnvGuard::set("CODOXEAR_TEST_TMUX_LOG", tmux_log.display().to_string());
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"cwd":"{}","create_in_tmux":true}}"#,
                        cwd.display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["broker_pid"], 7777);
        assert_eq!(payload["tmux_session"], "codoxear");
        assert!(payload["tmux_window"].as_str().unwrap().starts_with("workspace-"));

        let log = fs::read_to_string(&tmux_log).unwrap();
        assert!(log.contains("CODEX_WEB_TRANSPORT=tmux"));
        assert!(log.contains("CODEX_WEB_TMUX_SESSION=codoxear"));
        assert!(log.contains("codoxear.broker"));
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
        assert!(std::process::Command::new("git")
            .current_dir(&repo_dir)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .current_dir(&repo_dir)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .current_dir(&repo_dir)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap()
            .success());
        fs::write(repo_dir.join("notes.txt"), "hello from rust\n").unwrap();
        fs::write(repo_dir.join("image.png"), b"\x89PNG\r\n\x1a\nrest").unwrap();
        assert!(std::process::Command::new("git")
            .current_dir(&repo_dir)
            .args(["add", "notes.txt", "image.png"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .current_dir(&repo_dir)
            .args(["commit", "-qm", "init"])
            .status()
            .unwrap()
            .success());
        fs::write(repo_dir.join("notes.txt"), "hello from rust\nwith unstaged line\n").unwrap();
        fs::write(repo_dir.join("staged.txt"), "staged only\n").unwrap();
        assert!(std::process::Command::new("git")
            .current_dir(&repo_dir)
            .args(["add", "staged.txt"])
            .status()
            .unwrap()
            .success());
        let sock_path = app_dir.join("socks").join("sid-support.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        let listener_thread = thread::spawn(move || {
            for _ in 0..10 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
                assert_eq!(line.trim(), "{\"cmd\":\"state\"}");
                stream
                    .write_all(
                        b"{\"busy\":true,\"queue_len\":0,\"token\":{\"context_window\":128000,\"tokens_in_context\":64000,\"percent_remaining\":50}}\n",
                    )
                    .unwrap();
            }
        });
        let log_path = app_dir.join("rollout-support.jsonl");
        fs::write(
            &log_path,
            r#"{"type":"session_meta","payload":{"model_provider":"crs","model":"gpt-5.5","reasoning_effort":"high"}}
{"type":"event_msg","payload":{"type":"user_message","message":"hello"},"ts":12.0}
{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"done"},"ts":14.0}
"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("socks").join("sid-support.json"),
            format!(
                r#"{{"session_id":"thread-support","codex_pid":{},"broker_pid":{},"agent_backend":"codex","owner":"web","cwd":"{}","log_path":"{}","start_ts":10.0,"updated_ts":20.0}}"#,
                std::process::id(),
                std::process::id(),
                repo_dir.display(),
                log_path.display(),
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
        assert_eq!(diagnostics_payload["busy"], false);
        assert_eq!(diagnostics_payload["broker_busy"], true);
        assert_eq!(diagnostics_payload["token"]["context_window"], 128000);

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
        assert_eq!(file_payload["text"], "hello from rust\nwith unstaged line\n");

        let search = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-support/file/search?q=notes&limit=10")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(search.status(), StatusCode::OK);
        let search_body = to_bytes(search.into_body(), usize::MAX).await.unwrap();
        let search_payload: Value = serde_json::from_slice(&search_body).unwrap();
        assert_eq!(search_payload["mode"], "git");
        assert_eq!(search_payload["matches"][0]["path"], "notes.txt");

        let changed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-support/git/changed_files")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(changed.status(), StatusCode::OK);
        let changed_body = to_bytes(changed.into_body(), usize::MAX).await.unwrap();
        let changed_payload: Value = serde_json::from_slice(&changed_body).unwrap();
        let entries = changed_payload["entries"].as_array().unwrap();
        let notes_entry = entries.iter().find(|entry| entry["path"] == "notes.txt").unwrap();
        assert_eq!(notes_entry["additions"], 1);
        let staged = changed_payload["staged"].as_array().unwrap();
        assert!(staged.iter().any(|entry| entry == "staged.txt"));

        let diff = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-support/git/diff?path=notes.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(diff.status(), StatusCode::OK);
        let diff_body = to_bytes(diff.into_body(), usize::MAX).await.unwrap();
        let diff_payload: Value = serde_json::from_slice(&diff_body).unwrap();
        assert_eq!(diff_payload["path"], "notes.txt");
        assert_eq!(diff_payload["staged"], false);
        assert!(diff_payload["diff"].as_str().unwrap().contains("+with unstaged line"));

        let versions = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-support/git/file_versions?path=notes.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(versions.status(), StatusCode::OK);
        let versions_body = to_bytes(versions.into_body(), usize::MAX).await.unwrap();
        let versions_payload: Value = serde_json::from_slice(&versions_body).unwrap();
        assert_eq!(versions_payload["path"], "notes.txt");
        assert_eq!(versions_payload["current_exists"], true);
        assert_eq!(versions_payload["current_size"], 35);
        assert_eq!(versions_payload["current_text"], "hello from rust\nwith unstaged line\n");
        assert_eq!(versions_payload["base_exists"], true);
        assert_eq!(versions_payload["base_text"], "hello from rust\n");

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
        listener_thread.join().unwrap();
    }

    #[tokio::test]
    async fn session_action_routes_forward_send_and_interrupt_to_broker() {
        let app_dir = temp_app_dir("actions");
        let sock_path = app_dir.join("socks").join("sid-action.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        let listener_thread = thread::spawn(move || {
            for _ in 0..5 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
                let payload: Value = serde_json::from_str(line.trim()).unwrap();
                match payload["cmd"].as_str().unwrap() {
                    "state" => {
                        stream
                            .write_all(b"{\"busy\":false,\"queue_len\":0}\n")
                            .unwrap();
                    }
                    "send" => {
                        assert_eq!(payload["text"], "ship it");
                        stream
                            .write_all(b"{\"queued\":false,\"queue_len\":0}\n")
                            .unwrap();
                    }
                    "keys" => {
                        assert_eq!(payload["seq"], "\\x1b");
                        stream.write_all(b"{\"accepted\":true}\n").unwrap();
                    }
                    other => panic!("unexpected broker command: {other}"),
                }
            }
        });
        fs::write(
            app_dir.join("socks").join("sid-action.json"),
            format!(
                r#"{{"session_id":"thread-action","codex_pid":{},"broker_pid":{},"cwd":"/repo","start_ts":11.0}}"#,
                std::process::id(),
                std::process::id(),
            ),
        )
        .unwrap();
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let send = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-action/send")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"text":"ship it"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(send.status(), StatusCode::OK);
        let send_body = to_bytes(send.into_body(), usize::MAX).await.unwrap();
        let send_payload: Value = serde_json::from_slice(&send_body).unwrap();
        assert_eq!(send_payload["queued"], false);
        assert_eq!(send_payload["queue_len"], 0);

        let interrupt = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-action/interrupt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(interrupt.status(), StatusCode::OK);
        let interrupt_body = to_bytes(interrupt.into_body(), usize::MAX).await.unwrap();
        let interrupt_payload: Value = serde_json::from_slice(&interrupt_body).unwrap();
        assert_eq!(interrupt_payload["ok"], true);
        assert_eq!(interrupt_payload["broker"]["accepted"], true);

        listener_thread.join().unwrap();
    }

    #[tokio::test]
    async fn session_rename_route_persists_cleaned_aliases() {
        let app_dir = temp_app_dir("rename");
        let repo_dir = app_dir.join("repo");
        fs::create_dir_all(&repo_dir).unwrap();
        fs::write(app_dir.join("socks").join("sid-rename.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-rename.json"),
            format!(
                r#"{{"session_id":"thread-rename","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":11.0}}"#,
                std::process::id(),
                std::process::id(),
                repo_dir.display(),
            ),
        )
        .unwrap();
        fs::write(app_dir.join("session_aliases.json"), r#"{"other":"keep"}"#).unwrap();
        let app = router(build_state_from_config(RuntimeConfig { app_dir: app_dir.clone() }).unwrap());

        let rename = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-rename/rename")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"   hello   rust   world   "}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rename.status(), StatusCode::OK);
        let rename_body = to_bytes(rename.into_body(), usize::MAX).await.unwrap();
        let rename_payload: Value = serde_json::from_slice(&rename_body).unwrap();
        assert_eq!(rename_payload["alias"], "hello rust world");

        let aliases: Value = serde_json::from_str(&fs::read_to_string(app_dir.join("session_aliases.json")).unwrap()).unwrap();
        assert_eq!(aliases["sid-rename"], "hello rust world");
        assert_eq!(aliases["other"], "keep");

        let clear = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-rename/rename")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"   "}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(clear.status(), StatusCode::OK);
        let clear_body = to_bytes(clear.into_body(), usize::MAX).await.unwrap();
        let clear_payload: Value = serde_json::from_slice(&clear_body).unwrap();
        assert_eq!(clear_payload["alias"], "");

        let cleared_aliases: Value = serde_json::from_str(&fs::read_to_string(app_dir.join("session_aliases.json")).unwrap()).unwrap();
        assert!(cleared_aliases.get("sid-rename").is_none());
        assert_eq!(cleared_aliases["other"], "keep");
    }

    #[tokio::test]
    async fn session_edit_route_persists_alias_and_sidebar_metadata() {
        let app_dir = temp_app_dir("edit");
        let repo_dir = app_dir.join("repo");
        let other_repo_dir = app_dir.join("repo-other");
        fs::create_dir_all(&repo_dir).unwrap();
        fs::create_dir_all(&other_repo_dir).unwrap();
        fs::write(app_dir.join("socks").join("sid-edit.sock"), "").unwrap();
        fs::write(app_dir.join("socks").join("sid-dependency.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-edit.json"),
            format!(
                r#"{{"session_id":"thread-edit","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":11.0}}"#,
                std::process::id(),
                std::process::id(),
                repo_dir.display(),
            ),
        )
        .unwrap();
        fs::write(
            app_dir.join("socks").join("sid-dependency.json"),
            format!(
                r#"{{"session_id":"thread-dependency","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":12.0}}"#,
                std::process::id(),
                std::process::id(),
                other_repo_dir.display(),
            ),
        )
        .unwrap();
        fs::write(app_dir.join("session_aliases.json"), r#"{"other":"keep"}"#).unwrap();
        fs::write(app_dir.join("session_sidebar.json"), r#"{"other":{"priority_offset":0.25}}"#).unwrap();

        let app = router(build_state_from_config(RuntimeConfig { app_dir: app_dir.clone() }).unwrap());

        let edit = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-edit/edit")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"name":"   hello   rust   edit   ","priority_offset":0.75,"snooze_until":1234,"dependency_session_id":"sid-dependency"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(edit.status(), StatusCode::OK);
        let edit_body = to_bytes(edit.into_body(), usize::MAX).await.unwrap();
        let edit_payload: Value = serde_json::from_slice(&edit_body).unwrap();
        assert_eq!(edit_payload["alias"], "hello rust edit");
        assert_eq!(edit_payload["priority_offset"], 0.75);
        assert_eq!(edit_payload["snooze_until"], 1234.0);
        assert_eq!(edit_payload["dependency_session_id"], "sid-dependency");

        let aliases: Value = serde_json::from_str(&fs::read_to_string(app_dir.join("session_aliases.json")).unwrap()).unwrap();
        assert_eq!(aliases["sid-edit"], "hello rust edit");
        assert_eq!(aliases["other"], "keep");

        let sidebar: Value = serde_json::from_str(&fs::read_to_string(app_dir.join("session_sidebar.json")).unwrap()).unwrap();
        assert_eq!(sidebar["sid-edit"]["priority_offset"], 0.75);
        assert_eq!(sidebar["sid-edit"]["snooze_until"], 1234.0);
        assert_eq!(sidebar["sid-edit"]["dependency_session_id"], "sid-dependency");
        assert_eq!(sidebar["other"]["priority_offset"], 0.25);
    }

    #[tokio::test]
    async fn session_inject_attachment_route_stages_file_and_sends_bracketed_paste() {
        let app_dir = temp_app_dir("inject-attachment");
        let repo_dir = app_dir.join("repo");
        fs::create_dir_all(&repo_dir).unwrap();
        let sock_path = app_dir.join("socks").join("sid-attach.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let listener_thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut saw_keys = false;
            while Instant::now() < deadline && !saw_keys {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(err) => panic!("accept failed: {err}"),
                };
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
                let payload: Value = serde_json::from_str(line.trim()).unwrap();
                match payload["cmd"].as_str().unwrap() {
                    "state" => {
                        stream.write_all(b"{\"busy\":false,\"queue_len\":0}\n").unwrap();
                    }
                    "keys" => {
                        let seq = payload["seq"].as_str().unwrap();
                        assert!(seq.starts_with("\u{1b}[200~Attachment 2: "));
                        assert!(seq.ends_with("\n\u{1b}[201~"));
                        stream.write_all(b"{\"accepted\":true}\n").unwrap();
                        saw_keys = true;
                    }
                    other => panic!("unexpected broker command: {other}"),
                }
            }
            assert!(saw_keys, "listener never observed injected keys");
        });
        fs::write(
            app_dir.join("socks").join("sid-attach.json"),
            format!(
                r#"{{"session_id":"thread-attach","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":11.0}}"#,
                std::process::id(),
                std::process::id(),
                repo_dir.display(),
            ),
        )
        .unwrap();
        let app = router(build_state_from_config(RuntimeConfig { app_dir: app_dir.clone() }).unwrap());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-attach/inject_file")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"filename":"../../payload.tar.gz","data_b64":"AAFwYXlsb2Fk","attachment_index":2}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let payload: Value = serde_json::from_slice(&body).unwrap();
        let staged_path = PathBuf::from(payload["path"].as_str().unwrap());
        assert_eq!(staged_path.parent().unwrap(), app_dir.join("uploads").join("sid-attach"));
        assert!(staged_path.file_name().unwrap().to_string_lossy().ends_with("payload.tar.gz"));
        assert_eq!(fs::read(&staged_path).unwrap(), b"\x00\x01payload");
        assert_eq!(payload["inject_text"], format!("Attachment 2: {}\n", staged_path.display()));
        assert_eq!(payload["broker"]["accepted"], true);

        listener_thread.join().unwrap();
    }

    #[tokio::test]
    async fn session_harness_route_persists_config_for_python_sweep_reload() {
        let app_dir = temp_app_dir("harness-write");
        let repo_dir = app_dir.join("repo");
        fs::create_dir_all(&repo_dir).unwrap();
        let sock_path = app_dir.join("socks").join("sid-harness.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        let listener_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let payload: Value = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(payload["cmd"], "state");
            stream.write_all(b"{\"busy\":false,\"queue_len\":0}\n").unwrap();
        });
        fs::write(
            app_dir.join("socks").join("sid-harness.json"),
            format!(
                r#"{{"session_id":"thread-harness","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":11.0}}"#,
                std::process::id(),
                std::process::id(),
                repo_dir.display(),
            ),
        )
        .unwrap();
        fs::write(
            app_dir.join("harness.json"),
            r#"{"sid-harness":{"enabled":false,"request":"old","cooldown_minutes":9,"remaining_injections":4}}"#,
        )
        .unwrap();
        let app = router(build_state_from_config(RuntimeConfig { app_dir: app_dir.clone() }).unwrap());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-harness/harness")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"enabled":true,"request":"keep going","cooldown_minutes":5,"remaining_injections":2}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["enabled"], true);
        assert_eq!(payload["request"], "keep going");
        assert_eq!(payload["cooldown_minutes"], 5.0);
        assert_eq!(payload["remaining_injections"], 2);

        let harness: Value = serde_json::from_str(&fs::read_to_string(app_dir.join("harness.json")).unwrap()).unwrap();
        assert_eq!(harness["sid-harness"]["enabled"], true);
        assert_eq!(harness["sid-harness"]["request"], "keep going");
        assert_eq!(harness["sid-harness"]["cooldown_minutes"], 5);
        assert_eq!(harness["sid-harness"]["remaining_injections"], 2);

        listener_thread.join().unwrap();
    }

    #[tokio::test]
    async fn session_delete_route_clears_persisted_session_state() {
        let app_dir = temp_app_dir("delete");
        let repo_dir = app_dir.join("repo");
        fs::create_dir_all(&repo_dir).unwrap();
        let sock_path = app_dir.join("socks").join("sid-delete.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        let listener_thread = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
                let payload: Value = serde_json::from_str(line.trim()).unwrap();
                match payload["cmd"].as_str().unwrap() {
                    "state" => {
                        stream.write_all(b"{\"busy\":false,\"queue_len\":0}\n").unwrap();
                    }
                    "shutdown" => {
                        stream.write_all(b"{\"ok\":true}\n").unwrap();
                    }
                    other => panic!("unexpected broker command: {other}"),
                }
            }
        });
        fs::write(
            app_dir.join("socks").join("sid-delete.json"),
            format!(
                r#"{{"session_id":"thread-delete","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":11.0}}"#,
                999_999,
                999_998,
                repo_dir.display(),
            ),
        )
        .unwrap();
        fs::write(
            app_dir.join("session_aliases.json"),
            r#"{"sid-delete":"Delete Me","other":"keep"}"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("session_sidebar.json"),
            format!(
                r#"{{"blocked":{{"priority_offset":0.0,"dependency_session_id":"sid-delete"}},"sid-delete":{{"priority_offset":0.5}},"other":{{"priority_offset":0.1}}}}"#,
            ),
        )
        .unwrap();
        fs::write(
            app_dir.join("harness.json"),
            r#"{"sid-delete":{"enabled":true,"request":"go"},"other":{"enabled":false}}"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("session_files.json"),
            format!(
                r#"{{"sid-delete":["legacy.txt"],"sid:sid-delete":["session.txt"],"cwd:{}":["cwd.txt"],"other":["keep.txt"]}}"#,
                repo_dir.display(),
            ),
        )
        .unwrap();
        fs::write(
            app_dir.join("session_queues.json"),
            r#"{"sid-delete":[{"id":"q1","text":"queued","created_ts":1.0}],"other":[{"id":"q2","text":"keep","created_ts":2.0}]}"#,
        )
        .unwrap();
        let app = router(build_state_from_config(RuntimeConfig { app_dir: app_dir.clone() }).unwrap());

        let delete = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-delete/delete")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete.status(), StatusCode::OK);
        let delete_body = to_bytes(delete.into_body(), usize::MAX).await.unwrap();
        let delete_payload: Value = serde_json::from_slice(&delete_body).unwrap();
        assert_eq!(delete_payload["ok"], true);

        let aliases: Value = serde_json::from_str(&fs::read_to_string(app_dir.join("session_aliases.json")).unwrap()).unwrap();
        assert!(aliases.get("sid-delete").is_none());
        assert_eq!(aliases["other"], "keep");

        let sidebar: Value = serde_json::from_str(&fs::read_to_string(app_dir.join("session_sidebar.json")).unwrap()).unwrap();
        assert!(sidebar.get("sid-delete").is_none());
        assert!(sidebar["blocked"].get("dependency_session_id").is_none());
        assert_eq!(sidebar["other"]["priority_offset"], 0.1);

        let harness: Value = serde_json::from_str(&fs::read_to_string(app_dir.join("harness.json")).unwrap()).unwrap();
        assert!(harness.get("sid-delete").is_none());
        assert_eq!(harness["other"]["enabled"], false);

        let files: Value = serde_json::from_str(&fs::read_to_string(app_dir.join("session_files.json")).unwrap()).unwrap();
        assert!(files.get("sid-delete").is_none());
        assert!(files.get("sid:sid-delete").is_none());
        assert!(files.get(&format!("cwd:{}", repo_dir.display())).is_none());
        assert_eq!(files["other"][0], "keep.txt");

        let queues: Value = serde_json::from_str(&fs::read_to_string(app_dir.join("session_queues.json")).unwrap()).unwrap();
        assert!(queues.get("sid-delete").is_none());
        assert_eq!(queues["other"][0]["text"], "keep");

        listener_thread.join().unwrap();
    }

    #[tokio::test]
    async fn queue_action_routes_mutate_queue_and_enqueue_immediately_when_idle() {
        let app_dir = temp_app_dir("queue-actions");
        let sock_path = app_dir.join("socks").join("sid-queue.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        let listener_thread = thread::spawn(move || {
            let mut state_calls = 0usize;
            for _ in 0..11 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
                let payload: Value = serde_json::from_str(line.trim()).unwrap();
                match payload["cmd"].as_str().unwrap() {
                    "state" => {
                        state_calls += 1;
                        let reply = if state_calls <= 3 {
                            b"{\"busy\":false,\"queue_len\":0}\n".as_slice()
                        } else {
                            b"{\"busy\":true,\"queue_len\":0}\n".as_slice()
                        };
                        stream.write_all(reply).unwrap();
                    }
                    "send" => {
                        assert_eq!(payload["text"], "ship now");
                        stream
                            .write_all(b"{\"queued\":false,\"queue_len\":0}\n")
                            .unwrap();
                    }
                    other => panic!("unexpected broker command: {other}"),
                }
            }
        });
        fs::write(
            app_dir.join("socks").join("sid-queue.json"),
            format!(
                r#"{{"session_id":"thread-queue","codex_pid":{},"broker_pid":{},"cwd":"/repo","start_ts":11.0}}"#,
                std::process::id(),
                std::process::id(),
            ),
        )
        .unwrap();
        let app = router(build_state_from_config(RuntimeConfig { app_dir: app_dir.clone() }).unwrap());

        let send_now = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-queue/enqueue")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"text":"ship now"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(send_now.status(), StatusCode::OK);
        let send_now_body = to_bytes(send_now.into_body(), usize::MAX).await.unwrap();
        let send_now_payload: Value = serde_json::from_slice(&send_now_body).unwrap();
        assert_eq!(send_now_payload["queued"], false);
        assert_eq!(send_now_payload["queue_len"], 0);

        let queue_after_send = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-queue/queue")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let queue_after_send_body = to_bytes(queue_after_send.into_body(), usize::MAX).await.unwrap();
        let queue_after_send_payload: Value = serde_json::from_slice(&queue_after_send_body).unwrap();
        assert_eq!(queue_after_send_payload["items"].as_array().unwrap().len(), 0);

        let first_queued = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-queue/enqueue")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"text":"queued one"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first_queued.status(), StatusCode::OK);
        let first_queued_body = to_bytes(first_queued.into_body(), usize::MAX).await.unwrap();
        let first_queued_payload: Value = serde_json::from_slice(&first_queued_body).unwrap();
        let first_id = first_queued_payload["item"]["id"].as_str().unwrap().to_string();
        assert_eq!(first_queued_payload["queued"], true);
        assert_eq!(first_queued_payload["queue_len"], 1);

        let second_queued = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-queue/enqueue")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"text":"queued two"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second_queued.status(), StatusCode::OK);
        let second_queued_body = to_bytes(second_queued.into_body(), usize::MAX).await.unwrap();
        let second_queued_payload: Value = serde_json::from_slice(&second_queued_body).unwrap();
        let second_id = second_queued_payload["item"]["id"].as_str().unwrap().to_string();
        assert_eq!(second_queued_payload["queue_len"], 2);

        let update = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-queue/queue/update")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"id":"{first_id}","text":"queued one edited"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update.status(), StatusCode::OK);
        let update_body = to_bytes(update.into_body(), usize::MAX).await.unwrap();
        let update_payload: Value = serde_json::from_slice(&update_body).unwrap();
        assert_eq!(update_payload["item"]["text"], "queued one edited");

        let move_item = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-queue/queue/move")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"id":"{second_id}","to_index":0}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(move_item.status(), StatusCode::OK);

        let delete = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-queue/queue/delete")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"id":"{first_id}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete.status(), StatusCode::OK);
        let delete_body = to_bytes(delete.into_body(), usize::MAX).await.unwrap();
        let delete_payload: Value = serde_json::from_slice(&delete_body).unwrap();
        assert_eq!(delete_payload["queue_len"], 1);

        let queue = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/sessions/sid-queue/queue")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(queue.status(), StatusCode::OK);
        let queue_body = to_bytes(queue.into_body(), usize::MAX).await.unwrap();
        let queue_payload: Value = serde_json::from_slice(&queue_body).unwrap();
        let items = queue_payload["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], second_id);
        assert_eq!(items[0]["text"], "queued two");

        listener_thread.join().unwrap();
    }

    #[tokio::test]
    async fn queue_action_routes_reject_mutating_sending_items() {
        let app_dir = temp_app_dir("queue-sending-guard");
        fs::write(app_dir.join("socks").join("sid-guard.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-guard.json"),
            format!(
                r#"{{"session_id":"thread-guard","codex_pid":{},"broker_pid":{},"cwd":"/repo","start_ts":11.0}}"#,
                std::process::id(),
                std::process::id(),
            ),
        )
        .unwrap();
        fs::write(
            app_dir.join("session_queues.json"),
            r#"{"sid-guard":[{"id":"sending","text":"first","created_ts":1.0,"sending":true},{"id":"queued","text":"second","created_ts":2.0}]}"#,
        )
        .unwrap();
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let delete = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-guard/queue/delete")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"id":"sending"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete.status(), StatusCode::BAD_GATEWAY);

        let update = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-guard/queue/update")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"id":"sending","text":"edited"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update.status(), StatusCode::BAD_GATEWAY);

        let move_item = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/sessions/sid-guard/queue/move")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"id":"queued","to_index":0}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(move_item.status(), StatusCode::BAD_GATEWAY);
    }
}
