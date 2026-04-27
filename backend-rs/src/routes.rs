use crate::app_state::{epoch_now, AppState};
use crate::models::{
    ApiChangedFilesResponse, ApiDiagnosticsResponse, ApiFileSearchResponse, ApiGitDiffResponse,
    ApiGitFileVersionsResponse, ApiHarnessResponse, ApiQueueResponse, EventKind, LiveEvent,
    SendMessagePayload, TranscriptEvent,
};
use crate::runtime::{
    create_session, create_session_request_from_payload,
    default_file_search_limit, load_changed_files_response, load_diagnostics_response,
    load_audio_playlist_bytes, load_audio_segment_bytes, load_codex_config_response,
    load_notification_feed_response, load_notification_message_response,
    load_notification_subscriptions_response, load_cwd_suggestions_response,
    load_file_blob, load_file_read_response, load_file_search_response, load_git_diff_response,
    load_git_file_versions_response, load_harness_response, load_messages_history,
    load_messages_live, load_messages_tail, load_queue_response,
    load_legacy_static_file, load_nova_shell_file,
    load_voice_settings_response,
    load_resume_candidates_response, load_sessions_response, normalize_backend, resolve_dir_target,
    delete_queue_item, delete_session, edit_session, enqueue_session_message,
    inject_session_attachment, interrupt_session, move_queue_item, rename_session,
    save_codex_config_response, save_voice_settings_response,
    schedule_local_service_restart_response, set_harness_config,
    send_session_message, update_queue_item,
    toggle_notification_subscription_response, upsert_notification_subscription_response,
};
use axum::extract::{Path, Query, State};
use axum::body::{to_bytes, Body};
use axum::http::{header, HeaderMap, HeaderValue, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Redirect, Response};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use bytes::Bytes;
use futures_util::stream::{self, Stream};
use hmac::{Hmac, Mac};
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Sha256;
use std::convert::Infallible;
use std::env;
use std::fs;
use std::process;
use std::time::Duration;
use tokio::time::sleep;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(root_redirect))
        .route("/nova", get(nova_redirect))
        .route("/nova/", get(nova_redirect))
        .route("/nova/*path", get(nova_redirect_nested))
        .route("/nova-preview", get(nova_preview_index))
        .route("/nova-preview/", get(nova_preview_index))
        .route("/nova-preview/assets/*path", get(nova_preview_asset))
        .route("/nova-preview/*path", get(nova_preview_spa))
        .route("/service-worker.js", get(service_worker))
        .route("/manifest.webmanifest", get(legacy_manifest))
        .route("/favicon.ico", get(legacy_favicon))
        .route("/favicon.png", get(legacy_favicon_png))
        .merge(legacy_router(state.clone()))
        .route("/api/v1/health", get(health))
        .route("/api/v1/bootstrap", get(bootstrap))
        .route("/api/v1/me", get(me))
        .route("/api/v1/session_resume_candidates", get(session_resume_candidates))
        .route("/api/v1/cwd_suggestions", get(cwd_suggestions))
        .route("/api/v1/settings/codex_config", get(settings_codex_config).post(settings_codex_config_save))
        .route("/api/v1/settings/restart_service", post(settings_restart_service))
        .route("/api/v1/settings/voice", get(settings_voice).post(settings_voice_save))
        .route(
            "/api/v1/notifications/subscription",
            get(notification_subscriptions).post(notification_subscriptions_upsert),
        )
        .route(
            "/api/v1/notifications/subscription/toggle",
            post(notification_subscriptions_toggle),
        )
        .route("/api/v1/notifications/message", get(notification_message))
        .route("/api/v1/notifications/feed", get(notification_feed))
        .route("/api/v1/audio/live.m3u8", get(audio_playlist))
        .route("/api/v1/audio/segments/*path", get(audio_segment))
        .route("/api/v1/sessions", get(sessions).post(session_create))
        .route("/api/v1/login", post(login))
        .route("/api/v1/logout", post(logout))
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
        .nest("/api", public_api_router(state.clone()))
        .with_state(state)
}

fn legacy_router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/legacy", get(legacy_entry_proxy).post(legacy_entry_proxy))
        .route("/legacy/", get(legacy_entry_proxy).post(legacy_entry_proxy))
        .route("/legacy/*path", get(legacy_entry_proxy).post(legacy_entry_proxy))
        .route_layer(middleware::from_fn_with_state(
            state,
            require_public_api_auth,
        ))
}

fn public_api_router(state: AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/bootstrap", get(bootstrap))
        .route("/me", get(me))
        .route("/session_resume_candidates", get(session_resume_candidates))
        .route("/cwd_suggestions", get(cwd_suggestions))
        .route("/settings/codex_config", get(settings_codex_config).post(settings_codex_config_save))
        .route("/settings/restart_service", post(settings_restart_service))
        .route("/settings/voice", get(settings_voice).post(settings_voice_save))
        .route(
            "/notifications/subscription",
            get(notification_subscriptions).post(notification_subscriptions_upsert),
        )
        .route(
            "/notifications/subscription/toggle",
            post(notification_subscriptions_toggle),
        )
        .route("/notifications/message", get(notification_message))
        .route("/notifications/feed", get(notification_feed))
        .route("/audio/live.m3u8", get(audio_playlist))
        .route("/audio/listener", post(legacy_audio_listener_proxy))
        .route("/audio/segments/*path", get(audio_segment))
        .route("/sessions", get(sessions).post(session_create))
        .route("/sessions/:session_id/diagnostics", get(diagnostics))
        .route("/sessions/:session_id/queue", get(queue))
        .route("/sessions/:session_id/enqueue", post(session_enqueue))
        .route("/sessions/:session_id/queue/delete", post(queue_delete))
        .route("/sessions/:session_id/queue/update", post(queue_update))
        .route("/sessions/:session_id/queue/move", post(queue_move))
        .route("/sessions/:session_id/rename", post(session_rename))
        .route("/sessions/:session_id/edit", post(session_edit))
        .route("/sessions/:session_id/delete", post(session_delete))
        .route("/sessions/:session_id/inject_file", post(session_inject_attachment))
        .route("/sessions/:session_id/inject_image", post(session_inject_attachment))
        .route("/sessions/:session_id/harness", get(harness).post(session_harness))
        .route("/sessions/:session_id/git/changed_files", get(changed_files))
        .route("/sessions/:session_id/git/diff", get(git_diff))
        .route("/sessions/:session_id/git/file_versions", get(git_file_versions))
        .route("/sessions/:session_id/file/read", get(file_read))
        .route("/sessions/:session_id/file/search", get(file_search))
        .route("/sessions/:session_id/file/blob", get(file_blob))
        .route("/sessions/:session_id/messages/tail", get(messages_tail))
        .route("/sessions/:session_id/messages/history", get(messages_history))
        .route("/sessions/:session_id/messages/live", get(messages_live))
        .route("/sessions/:session_id/send", post(session_send))
        .route("/sessions/:session_id/interrupt", post(session_interrupt))
        .route("/messages/send", post(send_message))
        .route("/events/stream", get(events))
        .route_layer(middleware::from_fn_with_state(
            state,
            require_public_api_auth,
        ));
    Router::new()
        .route("/health", get(health))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .merge(protected)
}

async fn root_redirect() -> Redirect {
    Redirect::permanent("/nova-preview/")
}

async fn nova_redirect() -> Redirect {
    Redirect::permanent("/nova-preview/")
}

async fn nova_redirect_nested(Path(path): Path<String>) -> Redirect {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        Redirect::permanent("/nova-preview/")
    } else {
        Redirect::permanent(&format!("/nova-preview/{trimmed}"))
    }
}

async fn nova_preview_index() -> Result<Response, (StatusCode, String)> {
    static_file_response(load_nova_shell_file("index.html"), true)
}

async fn nova_preview_asset(Path(path): Path<String>) -> Result<Response, (StatusCode, String)> {
    static_file_response(load_nova_shell_file(&path), false)
}

async fn nova_preview_spa(Path(path): Path<String>) -> Result<Response, (StatusCode, String)> {
    let last = path.rsplit('/').next().unwrap_or_default();
    if last.contains('.') {
        return Err((StatusCode::NOT_FOUND, format!("static file not found: {path}")));
    }
    static_file_response(load_nova_shell_file("index.html"), true)
}

async fn service_worker() -> Result<Response, (StatusCode, String)> {
    static_file_response(load_legacy_static_file("service-worker.js"), true)
}

async fn legacy_entry_proxy(request: Request<Body>) -> Result<Response, (StatusCode, String)> {
    let request_path = rewrite_legacy_path(request.uri());
    proxy_legacy_request(request, Some(request_path)).await
}

async fn legacy_audio_listener_proxy(request: Request<Body>) -> Result<Response, (StatusCode, String)> {
    let request_path = rewrite_public_api_path(request.uri());
    proxy_legacy_request(request, Some(request_path)).await
}

async fn legacy_manifest() -> Result<Response, (StatusCode, String)> {
    static_file_response(load_legacy_static_file("manifest.webmanifest"), false)
}

async fn legacy_favicon() -> Result<Response, (StatusCode, String)> {
    static_file_response(load_legacy_static_file("favicon.png"), false)
}

async fn legacy_favicon_png() -> Result<Response, (StatusCode, String)> {
    static_file_response(load_legacy_static_file("favicon.png"), false)
}

fn static_file_response(
    file_result: Result<(Vec<u8>, String), String>,
    no_cache: bool,
) -> Result<Response, (StatusCode, String)> {
    let (raw, content_type) = file_result.map_err(|message| {
        if message.starts_with("read ") {
            (StatusCode::NOT_FOUND, message)
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, message)
        }
    })?;
    let mut response = Response::new(Body::from(raw.clone()));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type)
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?,
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&raw.len().to_string())
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?,
    );
    if no_cache {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
        headers.insert(header::EXPIRES, HeaderValue::from_static("0"));
    }
    Ok(response)
}

async fn proxy_legacy_request(
    request: Request<Body>,
    rewrite_path: Option<String>,
) -> Result<Response, (StatusCode, String)> {
    let target_base = legacy_backend_base();
    proxy_request_to_base(&target_base, request, rewrite_path).await
}

async fn proxy_request_to_base(
    target_base: &str,
    request: Request<Body>,
    rewrite_path: Option<String>,
) -> Result<Response, (StatusCode, String)> {
    let (parts, body) = request.into_parts();
    let request_path = rewrite_path.unwrap_or_else(|| request_path_with_query(&parts.uri));
    let url = format!("{target_base}{request_path}");
    let request_body = to_bytes(body, usize::MAX)
        .await
        .map_err(|err| (StatusCode::BAD_GATEWAY, format!("proxy body read error: {err}")))?;
    let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build_http();
    let mut upstream_request = Request::builder().method(parts.method).uri(&url);
    for (name, value) in &parts.headers {
        if should_skip_proxy_request_header(name.as_str()) {
            continue;
        }
        upstream_request = upstream_request.header(name, value);
    }
    let upstream_request = upstream_request
        .body(Full::new(request_body))
        .map_err(|err| (StatusCode::BAD_GATEWAY, format!("proxy request build error: {err}")))?;
    let upstream_response = tokio::time::timeout(Duration::from_secs(600), client.request(upstream_request))
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "proxy timeout".to_string()))?
        .map_err(|err| (StatusCode::BAD_GATEWAY, format!("proxy error: {err}")))?;
    let (upstream_parts, upstream_body) = upstream_response.into_parts();
    let status = StatusCode::from_u16(upstream_parts.status.as_u16())
        .map_err(|err| (StatusCode::BAD_GATEWAY, format!("invalid proxy status: {err}")))?;
    let upstream_body = tokio::time::timeout(Duration::from_secs(600), upstream_body.collect())
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "proxy timeout".to_string()))?
        .map_err(|err| (StatusCode::BAD_GATEWAY, format!("proxy error: {err}")))?
        .to_bytes();
    let mut response = Response::new(Body::from(upstream_body));
    *response.status_mut() = status;
    for (name, value) in &upstream_parts.headers {
        if should_skip_proxy_response_header(name.as_str()) {
            continue;
        }
        response.headers_mut().append(name, value.clone());
    }
    Ok(response)
}

fn legacy_backend_base() -> String {
    env::var("CODEX_WEB_NOVA_LEGACY_BASE")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:8744".to_string())
}

fn request_path_with_query(uri: &axum::http::Uri) -> String {
    match uri.query() {
        Some(query) => format!("{}?{query}", uri.path()),
        None => uri.path().to_string(),
    }
}

fn rewrite_legacy_path(uri: &axum::http::Uri) -> String {
    let rewritten = uri.path().strip_prefix("/legacy").unwrap_or(uri.path());
    let rewritten = if rewritten.is_empty() { "/" } else { rewritten };
    match uri.query() {
        Some(query) => format!("{rewritten}?{query}"),
        None => rewritten.to_string(),
    }
}

fn rewrite_public_api_path(uri: &axum::http::Uri) -> String {
    let path = if uri.path().starts_with("/api/") {
        uri.path().to_string()
    } else {
        format!("/api{}", uri.path())
    };
    match uri.query() {
        Some(query) => format!("{path}?{query}"),
        None => path,
    }
}

fn should_skip_proxy_request_header(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "host" | "content-length" | "connection")
}

fn should_skip_proxy_response_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "transfer-encoding" | "connection" | "server" | "date"
    )
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "service": "codoxear-backend-rs" }))
}

async fn bootstrap(State(state): State<AppState>) -> Json<crate::models::BootstrapPayload> {
    let store = state.store.read().await;
    Json(store.bootstrap())
}

async fn me() -> Json<Value> {
    Json(json!({ "ok": true, "server_pid": i64::from(process::id()) }))
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
struct ResumeCandidatesQuery {
    cwd: String,
    agent_backend: Option<String>,
}

#[derive(Deserialize)]
struct CwdSuggestionsQuery {
    q: Option<String>,
    limit: Option<String>,
}

#[derive(Deserialize)]
struct NotificationMessageQuery {
    message_id: Option<String>,
}

#[derive(Deserialize)]
struct NotificationFeedQuery {
    since: Option<String>,
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

async fn session_resume_candidates(
    State(state): State<AppState>,
    Query(query): Query<ResumeCandidatesQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let agent_backend = normalize_backend(query.agent_backend.as_deref())
        .map_err(|message| json_error(StatusCode::BAD_REQUEST, &message, None))?;
    let cwd = resolve_dir_target(&query.cwd)
        .map_err(|message| json_error(StatusCode::BAD_REQUEST, &message, Some("cwd")))?;
    load_resume_candidates_response(&state.config, &cwd, &agent_backend)
        .map(Json)
        .map_err(|message| json_error(StatusCode::INTERNAL_SERVER_ERROR, &message, None))
}

async fn cwd_suggestions(
    State(state): State<AppState>,
    Query(query): Query<CwdSuggestionsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let raw_query = query.q.unwrap_or_default();
    let limit = match query.limit.as_deref() {
        None => 12,
        Some(raw) => raw
            .trim()
            .parse::<usize>()
            .map_err(|_| json_error(StatusCode::BAD_REQUEST, "limit must be an integer", Some("limit")))?,
    };
    load_cwd_suggestions_response(&state.config, &raw_query, limit)
        .map(Json)
        .map_err(|message| json_error(StatusCode::BAD_REQUEST, &message, Some("cwd")))
}

async fn settings_codex_config() -> Result<Json<Value>, (StatusCode, String)> {
    load_codex_config_response()
        .map(Json)
        .map_err(settings_route_error)
}

async fn settings_codex_config_save(
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !payload.is_object() {
        return Err((StatusCode::BAD_REQUEST, "invalid json body (expected object)".to_string()));
    }
    let Some(text) = payload.get("text").and_then(Value::as_str) else {
        return Err((StatusCode::BAD_REQUEST, "text required".to_string()));
    };
    save_codex_config_response(text)
        .map(Json)
        .map_err(settings_route_error)
}

async fn settings_restart_service() -> Result<Json<Value>, (StatusCode, String)> {
    schedule_local_service_restart_response()
        .map(Json)
        .map_err(restart_route_error)
}

async fn settings_voice(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    load_voice_settings_response(&state.config)
        .map(Json)
        .map_err(voice_route_error)
}

async fn settings_voice_save(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !payload.is_object() {
        return Err((StatusCode::BAD_REQUEST, "invalid json body (expected object)".to_string()));
    }
    save_voice_settings_response(&state.config, &payload)
        .map(Json)
        .map_err(voice_route_error)
}

async fn notification_subscriptions(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    load_notification_subscriptions_response(&state.config)
        .map(Json)
        .map_err(notification_route_error)
}

async fn notification_subscriptions_upsert(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !payload.is_object() {
        return Err((StatusCode::BAD_REQUEST, "invalid json body (expected object)".to_string()));
    }
    upsert_notification_subscription_response(
        &state.config,
        payload.get("subscription").unwrap_or(&Value::Null),
        payload.get("user_agent").and_then(Value::as_str).unwrap_or_default(),
        payload.get("device_label").and_then(Value::as_str).unwrap_or_default(),
        payload.get("device_class").and_then(Value::as_str).unwrap_or_default(),
    )
    .map(Json)
    .map_err(notification_route_error)
}

async fn notification_subscriptions_toggle(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !payload.is_object() {
        return Err((StatusCode::BAD_REQUEST, "invalid json body (expected object)".to_string()));
    }
    let Some(endpoint) = payload.get("endpoint").and_then(Value::as_str) else {
        return Err((StatusCode::BAD_REQUEST, "endpoint required".to_string()));
    };
    let Some(enabled) = payload.get("enabled").and_then(Value::as_bool) else {
        return Err((StatusCode::BAD_REQUEST, "enabled must be a boolean".to_string()));
    };
    toggle_notification_subscription_response(&state.config, endpoint, enabled)
        .map(Json)
        .map_err(notification_route_error)
}

async fn notification_message(
    State(state): State<AppState>,
    Query(query): Query<NotificationMessageQuery>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let message_id = query.message_id.unwrap_or_default();
    if message_id.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message_id required".to_string()));
    }
    load_notification_message_response(&state.config, &message_id)
        .map(Json)
        .map_err(notification_message_route_error)
}

async fn notification_feed(
    State(state): State<AppState>,
    Query(query): Query<NotificationFeedQuery>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let since_ts = query
        .since
        .as_deref()
        .unwrap_or("0")
        .trim()
        .parse::<f64>()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid since".to_string()))?;
    load_notification_feed_response(&state.config, since_ts)
        .map(Json)
        .map_err(notification_message_route_error)
}

async fn audio_playlist(
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, String)> {
    audio_response(
        load_audio_playlist_bytes(&state.config).map_err(audio_route_error)?,
        "application/vnd.apple.mpegurl",
    )
}

async fn audio_segment(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    audio_response(
        load_audio_segment_bytes(&state.config, &path).map_err(audio_route_error)?,
        "video/mp2t",
    )
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let password = payload.get("password").and_then(Value::as_str);
    let same = is_same_password(password).map_err(|message| json_error(StatusCode::INTERNAL_SERVER_ERROR, &message, None))?;
    if !same {
        return Err(json_error(StatusCode::FORBIDDEN, "bad password", None));
    }
    let forwarded_proto = headers.get("X-Forwarded-Proto").and_then(|value| value.to_str().ok());
    let cookie = auth_cookie_header(&state.config.app_dir, forwarded_proto)
        .map_err(|message| json_error(StatusCode::INTERNAL_SERVER_ERROR, &message, None))?;
    json_response_with_cookie(json!({ "ok": true }), &cookie)
        .map_err(|message| json_error(StatusCode::INTERNAL_SERVER_ERROR, &message, None))
}

async fn logout() -> Result<Response, (StatusCode, Json<Value>)> {
    let cookie = logout_cookie_header().map_err(|message| json_error(StatusCode::INTERNAL_SERVER_ERROR, &message, None))?;
    json_response_with_cookie(json!({ "ok": true }), &cookie)
        .map_err(|message| json_error(StatusCode::INTERNAL_SERVER_ERROR, &message, None))
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

fn settings_route_error(message: String) -> (StatusCode, String) {
    if message == "text required" || message == "invalid json body (expected object)" || message.starts_with("invalid TOML:") {
        return (StatusCode::BAD_REQUEST, message);
    }
    (StatusCode::INTERNAL_SERVER_ERROR, message)
}

fn restart_route_error(message: String) -> (StatusCode, String) {
    if message.starts_with("missing ") || message.contains(" is not executable") {
        return (StatusCode::INTERNAL_SERVER_ERROR, message);
    }
    (StatusCode::INTERNAL_SERVER_ERROR, message)
}

fn notification_route_error(message: String) -> (StatusCode, String) {
    if message == "unknown subscription" {
        return (StatusCode::NOT_FOUND, message);
    }
    if message == "invalid json body (expected object)"
        || message == "subscription must be an object"
        || message == "subscription endpoint required"
        || message == "subscription keys required"
        || message == "subscription keys.p256dh and keys.auth required"
        || message == "endpoint required"
        || message == "enabled must be a boolean"
    {
        return (StatusCode::BAD_REQUEST, message);
    }
    (StatusCode::INTERNAL_SERVER_ERROR, message)
}

fn voice_route_error(message: String) -> (StatusCode, String) {
    if message == "invalid json body (expected object)" || message == "tts_base_url must start with http:// or https://" {
        return (StatusCode::BAD_REQUEST, message);
    }
    (StatusCode::INTERNAL_SERVER_ERROR, message)
}

fn notification_message_route_error(message: String) -> (StatusCode, String) {
    if message == "message_id required" || message == "invalid since" {
        return (StatusCode::BAD_REQUEST, message);
    }
    if message == "unknown message" {
        return (StatusCode::NOT_FOUND, message);
    }
    (StatusCode::INTERNAL_SERVER_ERROR, message)
}

fn audio_route_error(message: String) -> (StatusCode, String) {
    if message == "unknown audio segment" {
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

fn json_error(status: StatusCode, message: &str, field: Option<&str>) -> (StatusCode, Json<Value>) {
    let mut payload = json!({ "error": message });
    if let Some(field_name) = field {
        payload["field"] = json!(field_name);
    }
    (status, Json(payload))
}

fn json_response_with_cookie(payload: Value, set_cookie: &str) -> Result<Response, String> {
    let raw = serde_json::to_vec(&payload).map_err(|err| err.to_string())?;
    let mut response = Response::new(Body::from(raw.clone()));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&raw.len().to_string()).map_err(|err| err.to_string())?,
    );
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(set_cookie).map_err(|err| err.to_string())?,
    );
    Ok(response)
}

fn json_response(status: StatusCode, payload: Value) -> Response {
    let raw = serde_json::to_vec(&payload).unwrap_or_else(|_| b"{\"error\":\"internal server error\"}".to_vec());
    let mut response = Response::new(Body::from(raw.clone()));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    if let Ok(value) = HeaderValue::from_str(&raw.len().to_string()) {
        headers.insert(header::CONTENT_LENGTH, value);
    }
    response
}

fn audio_response(raw: Vec<u8>, content_type: &'static str) -> Result<Response, (StatusCode, String)> {
    let mut response = Response::new(Body::from(raw.clone()));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(header::EXPIRES, HeaderValue::from_static("0"));
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&raw.len().to_string())
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?,
    );
    Ok(response)
}

async fn require_public_api_auth(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    match request_is_authenticated(request.headers(), &state.config.app_dir) {
        Ok(true) => next.run(request).await,
        Ok(false) => json_response(StatusCode::UNAUTHORIZED, json!({ "error": "unauthorized" })),
        Err(message) => json_response(StatusCode::INTERNAL_SERVER_ERROR, json!({ "error": message })),
    }
}

fn is_same_password(raw: Option<&str>) -> Result<bool, String> {
    let Some(password) = raw else {
        return Ok(false);
    };
    let expected = env::var("CODEX_WEB_PASSWORD")
        .map_err(|_| "CODEX_WEB_PASSWORD is required (set it in .env)".to_string())?
        .trim()
        .to_string();
    if expected.is_empty() {
        return Err("CODEX_WEB_PASSWORD is required (set it in .env)".to_string());
    }
    Ok(password == expected)
}

fn request_is_authenticated(headers: &HeaderMap, app_dir: &std::path::Path) -> Result<bool, String> {
    let Some(cookie_header) = headers.get(header::COOKIE).and_then(|value| value.to_str().ok()) else {
        return Ok(false);
    };
    let Some(token) = cookie_value(cookie_header, cookie_name()) else {
        return Ok(false);
    };
    verify_auth_cookie(&token, app_dir)
}

fn cookie_value(raw: &str, name: &str) -> Option<String> {
    raw.split(';').find_map(|part| {
        let trimmed = part.trim();
        let (key, value) = trimmed.split_once('=')?;
        if key.trim() == name {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

fn verify_auth_cookie(value: &str, app_dir: &std::path::Path) -> Result<bool, String> {
    let Some((payload_b64, sig_b64)) = value.split_once('.') else {
        return Ok(false);
    };
    let Ok(raw_payload) = URL_SAFE_NO_PAD.decode(payload_b64.as_bytes()) else {
        return Ok(false);
    };
    let Ok(signature) = URL_SAFE_NO_PAD.decode(sig_b64.as_bytes()) else {
        return Ok(false);
    };
    let secret = load_or_create_hmac_secret(app_dir)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(&secret).map_err(|err| err.to_string())?;
    mac.update(&raw_payload);
    if mac.verify_slice(&signature).is_err() {
        return Ok(false);
    }
    let Ok(payload) = serde_json::from_slice::<Value>(&raw_payload) else {
        return Ok(false);
    };
    let Some(exp) = payload.get("exp").and_then(Value::as_i64) else {
        return Ok(false);
    };
    Ok(exp > epoch_now() as i64)
}

fn auth_cookie_header(app_dir: &std::path::Path, forwarded_proto: Option<&str>) -> Result<String, String> {
    let exp = epoch_now() as i64 + cookie_ttl_seconds();
    let raw = serde_json::to_vec(&json!({ "exp": exp })).map_err(|err| err.to_string())?;
    let secret = load_or_create_hmac_secret(app_dir)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(&secret).map_err(|err| err.to_string())?;
    mac.update(&raw);
    let sig = mac.finalize().into_bytes();
    let mut attrs = vec![
        format!("{}={}.{}", cookie_name(), URL_SAFE_NO_PAD.encode(raw), URL_SAFE_NO_PAD.encode(sig)),
        format!("Path={}", cookie_path()?),
        "HttpOnly".to_string(),
        "SameSite=Strict".to_string(),
        format!("Max-Age={}", cookie_ttl_seconds()),
    ];
    let proto = forwarded_proto.unwrap_or_default().to_ascii_lowercase();
    if cookie_secure() || proto == "https" {
        attrs.push("Secure".to_string());
    }
    Ok(attrs.join("; "))
}

fn logout_cookie_header() -> Result<String, String> {
    Ok(format!(
        "{}=deleted; Path={}; Max-Age=0; HttpOnly; SameSite=Strict",
        cookie_name(),
        cookie_path()?
    ))
}

fn load_or_create_hmac_secret(app_dir: &std::path::Path) -> Result<Vec<u8>, String> {
    let path = app_dir.join("hmac_secret");
    fs::create_dir_all(app_dir).map_err(|err| format!("create {}: {err}", app_dir.display()))?;
    if path.exists() {
        let bytes = fs::read(&path).map_err(|err| format!("read {}: {err}", path.display()))?;
        if bytes.len() < 32 {
            return Err(format!("invalid hmac secret (too short): {}", path.display()));
        }
        return Ok(bytes.into_iter().take(64).collect());
    }
    let mut secret = vec![0_u8; 64];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut secret))
        .map_err(|err| format!("read /dev/urandom: {err}"))?;
    fs::write(&path, &secret).map_err(|err| format!("write {}: {err}", path.display()))?;
    #[allow(clippy::permissions_set_readonly_false)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&path)
            .map_err(|err| format!("stat {}: {err}", path.display()))?
            .permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&path, permissions).map_err(|err| format!("chmod {}: {err}", path.display()))?;
    }
    Ok(secret)
}

fn cookie_name() -> &'static str {
    "codoxear_auth"
}

fn cookie_ttl_seconds() -> i64 {
    env::var("CODEX_WEB_COOKIE_TTL_SECONDS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(30 * 24 * 3600)
}

fn cookie_secure() -> bool {
    env::var("CODEX_WEB_COOKIE_SECURE").ok().as_deref() == Some("1")
}

fn cookie_path() -> Result<String, String> {
    let prefix = normalize_url_prefix(env::var("CODEX_WEB_URL_PREFIX").ok().as_deref())?;
    Ok(if prefix.is_empty() {
        "/".to_string()
    } else {
        format!("{prefix}/")
    })
}

fn normalize_url_prefix(raw: Option<&str>) -> Result<String, String> {
    let Some(value) = raw else {
        return Ok(String::new());
    };
    let mut prefix = value.trim().to_string();
    if prefix.is_empty() || prefix == "/" {
        return Ok(String::new());
    }
    if prefix.contains("://") {
        return Err("CODEX_WEB_URL_PREFIX must be a path prefix (not a URL)".to_string());
    }
    if prefix.contains('?') || prefix.contains('#') {
        return Err("CODEX_WEB_URL_PREFIX must not include '?' or '#'".to_string());
    }
    if !prefix.starts_with('/') {
        return Err("CODEX_WEB_URL_PREFIX must start with '/'".to_string());
    }
    while prefix.len() > 1 && prefix.ends_with('/') {
        prefix.pop();
    }
    if prefix == "/" {
        return Ok(String::new());
    }
    Ok(prefix)
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
    use super::{json_response, router};
    use crate::app_state::{build_state, build_state_from_config};
    use crate::runtime::RuntimeConfig;
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Request, StatusCode};
    use axum::response::Response;
    use axum::routing::{get, post};
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

    #[tokio::test]
    async fn root_and_nova_routes_redirect_to_nova_preview() {
        let _guard = env_lock().lock().unwrap();
        let app = router(build_state());
        for path in ["/", "/nova", "/nova/extra/path"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
            let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap();
            assert!(location.starts_with("/nova-preview/"));
        }
    }

    #[tokio::test]
    async fn nova_preview_index_route_serves_html() {
        let _guard = env_lock().lock().unwrap();
        let app = router(build_state());
        let response = app
            .oneshot(Request::builder().uri("/nova-preview/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::CONTENT_TYPE).unwrap(), "text/html; charset=utf-8");
        assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(std::str::from_utf8(&body).unwrap().contains("<html"));
    }

    #[tokio::test]
    async fn service_worker_route_serves_javascript() {
        let _guard = env_lock().lock().unwrap();
        let app = router(build_state());
        let response = app
            .oneshot(Request::builder().uri("/service-worker.js").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::CONTENT_TYPE).unwrap(), "text/javascript; charset=utf-8");
        assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = std::str::from_utf8(&body).unwrap();
        assert!(text.contains("self.addEventListener") || text.contains("addEventListener("));
    }

    #[tokio::test]
    async fn me_route_returns_server_pid() {
        let app = router(build_state_from_config(RuntimeConfig {
            app_dir: temp_app_dir("me"),
        })
        .unwrap());
        let response = app
            .oneshot(Request::builder().uri("/api/v1/me").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["ok"], true);
        assert!(payload["server_pid"].as_i64().unwrap() > 0);
    }

    #[tokio::test]
    async fn login_and_logout_routes_manage_auth_cookie() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("auth-cookie");
        let _password = EnvGuard::set("CODEX_WEB_PASSWORD", "topsecret");
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let login = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"password":"topsecret"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::OK);
        let login_cookie = login.headers().get(header::SET_COOKIE).unwrap().to_str().unwrap().to_string();
        assert!(login_cookie.starts_with("codoxear_auth="));
        assert!(login_cookie.contains("HttpOnly"));
        assert!(login_cookie.contains("SameSite=Strict"));
        let login_body = to_bytes(login.into_body(), usize::MAX).await.unwrap();
        let login_payload: Value = serde_json::from_slice(&login_body).unwrap();
        assert_eq!(login_payload["ok"], true);

        let logout = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/logout")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logout.status(), StatusCode::OK);
        let logout_cookie = logout.headers().get(header::SET_COOKIE).unwrap().to_str().unwrap();
        assert!(logout_cookie.contains("codoxear_auth=deleted"));
        assert!(logout_cookie.contains("Max-Age=0"));
    }

    #[tokio::test]
    async fn public_api_routes_require_auth_and_accept_rust_cookie() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("public-api-auth");
        let codex_home = temp_dir("public-api-auth-home");
        let _password = EnvGuard::set("CODEX_WEB_PASSWORD", "topsecret");
        let _codex_home = EnvGuard::set("CODEX_HOME", codex_home.display().to_string());
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let unauthorized = app
            .clone()
            .oneshot(Request::builder().uri("/api/me").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        let unauthorized_body = to_bytes(unauthorized.into_body(), usize::MAX).await.unwrap();
        let unauthorized_payload: Value = serde_json::from_slice(&unauthorized_body).unwrap();
        assert_eq!(unauthorized_payload["error"], "unauthorized");

        let login = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"password":"topsecret"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::OK);
        let cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let me = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/me")
                    .header(header::COOKIE, cookie.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::OK);
        let me_body = to_bytes(me.into_body(), usize::MAX).await.unwrap();
        let me_payload: Value = serde_json::from_slice(&me_body).unwrap();
        assert_eq!(me_payload["ok"], true);

        let sessions = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(sessions.status(), StatusCode::OK);
        let sessions_body = to_bytes(sessions.into_body(), usize::MAX).await.unwrap();
        let sessions_payload: Value = serde_json::from_slice(&sessions_body).unwrap();
        assert_eq!(sessions_payload["sessions"], Value::Array(vec![]));
        assert!(sessions_payload.get("new_session_defaults").is_some());
    }

    #[tokio::test]
    async fn public_voice_and_audio_routes_use_rust_runtime_except_listener_proxy() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("legacy-api-proxy");
        fs::create_dir_all(app_dir.join("audio").join("segments")).unwrap();
        fs::write(
            app_dir.join("voice_settings.json"),
            r#"{
  "tts_enabled_for_narration": true,
  "tts_enabled_for_final_response": false,
  "tts_base_url": "https://tts.example",
  "tts_api_key": "secret-key",
  "summarization_model": "gpt-4.1-mini",
  "tts_model": "gpt-4o-mini-tts"
}
"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("voice_runtime.json"),
            r#"{
  "audio": {
    "queue_depth": 2,
    "active_listener_count": 1,
    "segment_count": 3,
    "last_error": "",
    "media_sequence": 9
  },
  "updated_ts": 12.5
}
"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("push_subscriptions.json"),
            r#"[
  {
    "subscription": {
      "endpoint": "https://push.example/mobile-1",
      "keys": {"p256dh": "abc", "auth": "def"}
    },
    "device_class": "mobile",
    "notifications_enabled": true
  },
  {
    "subscription": {
      "endpoint": "https://push.example/desktop-1",
      "keys": {"p256dh": "ghi", "auth": "jkl"}
    },
    "device_class": "desktop",
    "notifications_enabled": true
  }
]
"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("voice_delivery_ledger.json"),
            r#"{
  "msg-final": {
    "session_id": "sid-1",
    "session_display_name": "Repo",
    "message_class": "final_response",
    "summary_status": "sent",
    "push_status": "sent",
    "notification_text": "  final    summary  ",
    "updated_ts": 44.0
  },
  "msg-narration": {
    "session_id": "sid-1",
    "session_display_name": "Repo",
    "message_class": "narration",
    "summary_status": "sent",
    "push_status": "skipped",
    "notification_text": "narration summary",
    "updated_ts": 45.0
  }
}
"#,
        )
        .unwrap();
        fs::write(app_dir.join("audio").join("live.m3u8"), b"#EXTM3U\n# rust playlist\n").unwrap();
        fs::write(app_dir.join("audio").join("segments").join("clip-a.ts"), b"segment:clip-a").unwrap();
        let _password = EnvGuard::set("CODEX_WEB_PASSWORD", "topsecret");
        let _legacy_base = EnvGuard::set("CODEX_WEB_NOVA_LEGACY_BASE", spawn_mock_legacy_backend());
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());
        let cookie = login_cookie(&app).await;

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/settings/voice")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let settings = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/settings/voice?tab=tts")
                    .header(header::COOKIE, cookie.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(settings.status(), StatusCode::OK);
        let settings_body = to_bytes(settings.into_body(), usize::MAX).await.unwrap();
        let settings_payload: Value = serde_json::from_slice(&settings_body).unwrap();
        assert_eq!(settings_payload["tts_enabled_for_narration"], true);
        assert_eq!(settings_payload["tts_base_url"], "https://tts.example");
        assert_eq!(settings_payload["audio"]["queue_depth"], 2);
        assert_eq!(settings_payload["audio"]["active_listener_count"], 1);
        assert_eq!(settings_payload["notifications"]["enabled_devices"], 1);
        assert_eq!(settings_payload["notifications"]["total_devices"], 1);

        let message = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/notifications/message?message_id=msg-final")
                    .header(header::COOKIE, cookie.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(message.status(), StatusCode::OK);
        let message_body = to_bytes(message.into_body(), usize::MAX).await.unwrap();
        let message_payload: Value = serde_json::from_slice(&message_body).unwrap();
        assert_eq!(message_payload["message_id"], "msg-final");
        assert_eq!(message_payload["message_class"], "final_response");
        assert_eq!(message_payload["notification_text"], "final summary");

        let feed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/notifications/feed?since=0")
                    .header(header::COOKIE, cookie.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(feed.status(), StatusCode::OK);
        let feed_body = to_bytes(feed.into_body(), usize::MAX).await.unwrap();
        let feed_payload: Value = serde_json::from_slice(&feed_body).unwrap();
        assert_eq!(feed_payload["items"].as_array().unwrap().len(), 1);
        assert_eq!(feed_payload["items"][0]["message_id"], "msg-final");

        let listener = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/audio/listener")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, cookie.clone())
                    .body(Body::from(r#"{"client_id":"listener-a","enabled":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(listener.status(), StatusCode::OK);
        let listener_body = to_bytes(listener.into_body(), usize::MAX).await.unwrap();
        let listener_payload: Value = serde_json::from_slice(&listener_body).unwrap();
        assert_eq!(listener_payload["path"], "/api/audio/listener");
        assert!(listener_payload["body"]
            .as_str()
            .unwrap()
            .contains("listener-a"));

        let playlist = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/audio/live.m3u8")
                    .header(header::COOKIE, cookie.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(playlist.status(), StatusCode::OK);
        assert_eq!(
            playlist.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/vnd.apple.mpegurl"
        );
        let playlist_body = to_bytes(playlist.into_body(), usize::MAX).await.unwrap();
        let playlist_text = std::str::from_utf8(&playlist_body).unwrap();
        assert!(playlist_text.contains("#EXTM3U"));
        assert!(playlist_text.contains("rust playlist"));

        let segment = app
            .oneshot(
                Request::builder()
                    .uri("/api/audio/segments/clip-a.ts")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(segment.status(), StatusCode::OK);
        let segment_body = to_bytes(segment.into_body(), usize::MAX).await.unwrap();
        assert_eq!(std::str::from_utf8(&segment_body).unwrap(), "segment:clip-a");
    }

    #[tokio::test]
    async fn legacy_prefix_routes_strip_prefix_before_proxying() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("legacy-prefix-proxy");
        let _password = EnvGuard::set("CODEX_WEB_PASSWORD", "topsecret");
        let _legacy_base = EnvGuard::set("CODEX_WEB_NOVA_LEGACY_BASE", spawn_mock_legacy_backend());
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());
        let cookie = login_cookie(&app).await;

        let unauthorized = app
            .clone()
            .oneshot(Request::builder().uri("/legacy").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let nested = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/legacy/files/view?tab=diff")
                    .header(header::COOKIE, cookie.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(nested.status(), StatusCode::OK);
        let nested_body = to_bytes(nested.into_body(), usize::MAX).await.unwrap();
        let nested_payload: Value = serde_json::from_slice(&nested_body).unwrap();
        assert_eq!(nested_payload["path"], "/files/view");
        assert_eq!(nested_payload["query"], "tab=diff");

        let root = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/legacy?source=rust")
                    .header(header::CONTENT_TYPE, "text/plain")
                    .header(header::COOKIE, cookie)
                    .body(Body::from("legacy-body"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(root.status(), StatusCode::OK);
        let root_body = to_bytes(root.into_body(), usize::MAX).await.unwrap();
        let root_payload: Value = serde_json::from_slice(&root_body).unwrap();
        assert_eq!(root_payload["path"], "/");
        assert_eq!(root_payload["query"], "source=rust");
        assert_eq!(root_payload["body"], "legacy-body");
    }

    #[tokio::test]
    async fn session_resume_candidates_route_returns_alias_and_last_user_message() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("resume-candidates");
        let codex_home = temp_dir("resume-candidates-home");
        let workspace = temp_dir("resume-candidates-workspace");
        let sessions_dir = codex_home.join("sessions").join("2026").join("04").join("26");
        fs::create_dir_all(&sessions_dir).unwrap();
        let log_path = sessions_dir.join("rollout-2026-04-26T01-00-00-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa.jsonl");
        fs::write(
            &log_path,
            format!(
                concat!(
                    "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"resume-a\",\"cwd\":\"{}\",\"timestamp\":\"2026-04-26T01:00:00Z\",\"source\":\"cli\"}}}}\n",
                    "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"# AGENTS.md instructions for /repo\\n...\"}}]}}}}\n",
                    "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"latest prompt from resume route\"}}]}}}}\n"
                ),
                workspace.display()
            ),
        )
        .unwrap();
        fs::write(app_dir.join("session_aliases.json"), r#"{"resume-a":"Alias A"}"#).unwrap();
        let _codex_home = EnvGuard::set("CODEX_HOME", codex_home.display().to_string());
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/session_resume_candidates?cwd={}&agent_backend=codex",
                        workspace.display()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["exists"], true);
        assert_eq!(payload["sessions"][0]["session_id"], "resume-a");
        assert_eq!(payload["sessions"][0]["alias"], "Alias A");
        assert_eq!(payload["sessions"][0]["last_user_message"], "latest prompt from resume route");
    }

    #[tokio::test]
    async fn cwd_suggestions_route_returns_directory_matches() {
        let app_dir = temp_app_dir("cwd-suggestions");
        let root = temp_dir("cwd-suggestion-root");
        let alpha = root.join("alpha");
        let alpine = root.join("alpine");
        fs::create_dir_all(&alpha).unwrap();
        fs::create_dir_all(&alpine).unwrap();
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/cwd_suggestions?q={}&limit=5",
                        root.join("al").display()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        let suggestions = payload["suggestions"].as_array().unwrap();
        let values = suggestions
            .iter()
            .filter_map(|entry| entry.get("value").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(values.iter().any(|value| *value == alpha.display().to_string()));
        assert!(values.iter().any(|value| *value == alpine.display().to_string()));
    }

    #[tokio::test]
    async fn settings_codex_config_routes_round_trip_toml() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("settings-codex-config");
        let codex_home = temp_dir("settings-codex-home");
        let _codex_home = EnvGuard::set("CODEX_HOME", codex_home.display().to_string());
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let get_missing = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/settings/codex_config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_missing.status(), StatusCode::OK);
        let get_missing_body = to_bytes(get_missing.into_body(), usize::MAX).await.unwrap();
        let get_missing_payload: Value = serde_json::from_slice(&get_missing_body).unwrap();
        assert_eq!(get_missing_payload["exists"], false);
        assert_eq!(get_missing_payload["text"], "");

        let save = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/settings/codex_config")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"text":"model = \"gpt-5.4\"\n"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(save.status(), StatusCode::OK);
        let save_body = to_bytes(save.into_body(), usize::MAX).await.unwrap();
        let save_payload: Value = serde_json::from_slice(&save_body).unwrap();
        assert_eq!(save_payload["exists"], true);
        assert_eq!(save_payload["text"], "model = \"gpt-5.4\"\n");
        assert_eq!(
            fs::read_to_string(codex_home.join("config.toml")).unwrap(),
            "model = \"gpt-5.4\"\n"
        );

        let invalid = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/settings/codex_config")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"text":"model = [\n"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn settings_restart_service_route_schedules_local_daemon_restart() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("settings-restart-service");
        let repo_root = temp_dir("settings-restart-repo");
        let script_path = repo_root.join("scripts").join("codoxear-local");
        fs::create_dir_all(script_path.parent().unwrap()).unwrap();
        write_executable(
            &script_path,
            "#!/usr/bin/env bash\nset -euo pipefail\nsleep 30\n",
        );
        let _repo_root = EnvGuard::set("CODOXEAR_REPO_ROOT", repo_root.display().to_string());
        let app = router(build_state_from_config(RuntimeConfig { app_dir }).unwrap());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/settings/restart_service")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["scheduled"], true);
        assert_eq!(payload["script"], script_path.display().to_string());
        let restart_pid = payload["restart_pid"].as_i64().unwrap();
        assert!(restart_pid > 0);
        kill_pid(restart_pid);
    }

    #[tokio::test]
    async fn notification_subscription_routes_manage_records() {
        let app_dir = temp_app_dir("notification-subscriptions");
        let app = router(build_state_from_config(RuntimeConfig { app_dir: app_dir.clone() }).unwrap());

        let initial = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/notifications/subscription")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(initial.status(), StatusCode::OK);
        let initial_body = to_bytes(initial.into_body(), usize::MAX).await.unwrap();
        let initial_payload: Value = serde_json::from_slice(&initial_body).unwrap();
        assert_eq!(initial_payload["ok"], true);
        assert!(initial_payload["vapid_public_key"].as_str().unwrap().len() > 10);
        assert_eq!(initial_payload["subscriptions"].as_array().unwrap().len(), 0);
        assert!(app_dir.join("webpush_vapid_private.pem").exists());

        let endpoint = "https://push.example.test/sub/abc";
        let upsert = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/notifications/subscription")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"subscription":{{"endpoint":"{endpoint}","keys":{{"p256dh":"p-key","auth":"a-key"}}}},"user_agent":"Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) Mobile","device_label":"current-device","device_class":""}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(upsert.status(), StatusCode::OK);
        let upsert_body = to_bytes(upsert.into_body(), usize::MAX).await.unwrap();
        let upsert_payload: Value = serde_json::from_slice(&upsert_body).unwrap();
        let subscriptions = upsert_payload["subscriptions"].as_array().unwrap();
        assert_eq!(subscriptions.len(), 1);
        assert_eq!(subscriptions[0]["endpoint"], endpoint);
        assert_eq!(subscriptions[0]["notifications_enabled"], true);
        assert_eq!(subscriptions[0]["device_class"], "mobile");

        let toggle = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/notifications/subscription/toggle")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"endpoint":"{endpoint}","enabled":false}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(toggle.status(), StatusCode::OK);
        let toggle_body = to_bytes(toggle.into_body(), usize::MAX).await.unwrap();
        let toggle_payload: Value = serde_json::from_slice(&toggle_body).unwrap();
        assert_eq!(toggle_payload["subscriptions"][0]["notifications_enabled"], false);
        let saved = fs::read_to_string(app_dir.join("push_subscriptions.json")).unwrap();
        assert!(saved.contains(endpoint));
        assert!(saved.contains("\"notifications_enabled\": false"));
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

    async fn login_cookie(app: &axum::Router) -> String {
        let login = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"password":"topsecret"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::OK);
        login
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    fn spawn_mock_legacy_backend() -> String {
        async fn echo_request(request: Request<Body>) -> Response {
            let (parts, body) = request.into_parts();
            let body = to_bytes(body, usize::MAX).await.unwrap();
            json_response(
                StatusCode::OK,
                serde_json::json!({
                    "method": parts.method.as_str(),
                    "path": parts.uri.path(),
                    "query": parts.uri.query().unwrap_or_default(),
                    "cookie": parts
                        .headers
                        .get(header::COOKIE)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default(),
                    "body": String::from_utf8_lossy(&body).to_string(),
                }),
            )
        }

        async fn playlist_request(request: Request<Body>) -> Response {
            let cookie = request
                .headers()
                .get(header::COOKIE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            let raw = format!("#EXTM3U\n# cookie:{cookie}\n");
            let mut response = Response::new(Body::from(raw));
            *response.status_mut() = StatusCode::OK;
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/vnd.apple.mpegurl"),
            );
            response
        }

        async fn segment_request(request: Request<Body>) -> Response {
            let segment = request
                .uri()
                .path()
                .split("/api/audio/segments/")
                .nth(1)
                .unwrap_or_default();
            Response::new(Body::from(format!("segment:{segment}")))
        }

        let router = axum::Router::new()
            .route("/api/settings/voice", get(echo_request).post(echo_request))
            .route("/api/notifications/message", get(echo_request))
            .route("/api/notifications/feed", get(echo_request))
            .route("/api/audio/live.m3u8", get(playlist_request))
            .route("/api/audio/listener", post(echo_request))
            .route("/api/audio/segments/*path", get(segment_request))
            .route("/", get(echo_request).post(echo_request))
            .route("/*path", get(echo_request).post(echo_request));

        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = std_listener.local_addr().unwrap();
        std_listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{addr}")
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
