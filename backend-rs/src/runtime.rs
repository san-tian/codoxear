use crate::models::{
    ApiBackendDefaults, ApiDiagnosticsResponse, ApiHarnessResponse, ApiMessagesHistoryResponse,
    ApiMessagesLiveResponse, ApiMessagesTailResponse, ApiNewSessionDefaults, ApiQueueItem,
    ApiQueueResponse, ApiSessionSummary, ApiSessionsResponse, SessionDetail, SessionSummary,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use toml::Value as TomlValue;

const APP_VERSION: &str = "0.1.0";
const CONTEXT_WINDOW_BASELINE_TOKENS: i64 = 12000;
const HARNESS_DEFAULT_IDLE_MINUTES: f64 = 15.0;
const HARNESS_DEFAULT_MAX_INJECTIONS: i64 = 1;
const FILE_READ_MAX_BYTES: u64 = 2 * 1024 * 1024;
const SIDEBAR_PRIORITY_HALF_LIFE_SECONDS: f64 = 8.0 * 3600.0;
const SIDEBAR_PRIORITY_LAMBDA: f64 = std::f64::consts::LN_2 / SIDEBAR_PRIORITY_HALF_LIFE_SECONDS;
const SUPPORTED_REASONING_EFFORTS: &[&str] = &["xhigh", "high", "medium", "low"];
const SUPPORTED_PI_REASONING_EFFORTS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh"];
const TEXTUAL_EXTENSIONS: &[&str] = &[
    "bash", "c", "cc", "cfg", "conf", "cpp", "css", "csv", "diff", "go", "h", "hpp", "htm",
    "html", "ini", "java", "js", "json", "jsonl", "log", "md", "markdown", "mdown", "mkd", "patch",
    "py", "rs", "scss", "sh", "sql", "svg", "toml", "ts", "tsx", "txt", "xml", "yaml", "yml", "zsh",
];
const TEXTUAL_FILENAMES: &[&str] = &["dockerfile", "license", "makefile", "readme"];

#[derive(Clone)]
pub struct RuntimeConfig {
    pub app_dir: PathBuf,
}

impl RuntimeConfig {
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            app_dir: default_app_dir()?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct SessionMeta {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    codex_pid: Option<i64>,
    #[serde(default)]
    broker_pid: Option<i64>,
    #[serde(default)]
    agent_backend: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    transport: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    log_path: Option<String>,
    #[serde(default)]
    start_ts: Option<f64>,
    #[serde(default)]
    updated_ts: Option<f64>,
    #[serde(default)]
    model_provider: Option<String>,
    #[serde(default)]
    preferred_auth_method: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    service_tier: Option<String>,
    #[serde(default)]
    tmux_session: Option<String>,
    #[serde(default)]
    tmux_window: Option<String>,
}

#[derive(Debug)]
struct BrokerState {
    busy: bool,
    token: Option<Value>,
}

pub fn default_app_dir() -> Result<PathBuf, String> {
    if let Ok(raw) = env::var("CODOXEAR_APP_DIR") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    let home = env::var("HOME").map_err(|_| "HOME is not set; cannot resolve Codoxear app dir".to_string())?;
    Ok(PathBuf::from(home).join(".local").join("share").join("codoxear"))
}

pub fn load_sessions_response(config: &RuntimeConfig) -> Result<ApiSessionsResponse, String> {
    let aliases = read_string_map(&config.app_dir.join("session_aliases.json"))?;
    let hidden = read_hidden_sessions(&config.app_dir.join("hidden_sessions.json"))?;
    let queues = read_array_map(&config.app_dir.join("session_queues.json"))?;
    let harness = read_object_map(&config.app_dir.join("harness.json"))?;
    let sidebar = read_object_map(&config.app_dir.join("session_sidebar.json"))?;
    let files = read_string_array_map(&config.app_dir.join("session_files.json"))?;
    let recent_cwds = read_recent_cwds(&config.app_dir.join("recent_cwds.json"))?;

    let mut sessions = Vec::new();
    let socks_dir = config.app_dir.join("socks");
    if socks_dir.exists() {
        let mut entries = fs::read_dir(&socks_dir)
            .map_err(|err| format!("read {}: {err}", socks_dir.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("read {}: {err}", socks_dir.display()))?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let sock_path = entry.path();
            if sock_path.extension().and_then(|ext| ext.to_str()) != Some("sock") {
                continue;
            }
            let session_id = sock_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| format!("invalid socket filename: {}", sock_path.display()))?
                .to_string();
            if hidden.contains(&session_id) {
                continue;
            }
            let meta_path = sock_path.with_extension("json");
            let meta: SessionMeta = read_json_file(&meta_path)?;
            if let Some(session) = session_from_meta(
                &session_id,
                &sock_path,
                meta,
                &aliases,
                &queues,
                &harness,
                &sidebar,
                &files,
            )? {
                sessions.push(session);
            }
        }
    }

    sessions.sort_by(|a, b| {
        b.final_priority
            .partial_cmp(&a.final_priority)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.updated_ts
                    .partial_cmp(&a.updated_ts)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                b.start_ts
                    .partial_cmp(&a.start_ts)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.session_id.cmp(&b.session_id))
    });

    Ok(ApiSessionsResponse {
        app_version: APP_VERSION.to_string(),
        sessions,
        recent_cwds,
        new_session_defaults: new_session_defaults()?,
        tmux_available: tmux_available(),
        tmux_session_name: Some("codoxear".to_string()),
    })
}

pub fn load_messages_tail(
    config: &RuntimeConfig,
    session_id: &str,
    limit: usize,
) -> Result<ApiMessagesTailResponse, String> {
    let session = find_session(config, session_id)?;
    let Some(log_path) = session.log_path.as_deref() else {
        return Ok(ApiMessagesTailResponse {
            thread_id: session.thread_id,
            log_path: None,
            live_cursor: None,
            history_cursor: None,
            events: vec![],
            has_older: false,
            busy: session.busy,
            queue_len: session.queue_len,
            token: session.token,
        });
    };
    let records = read_positioned_records(Path::new(log_path))?;
    let chat_records = chat_records(records);
    let take = limit.max(1).min(chat_records.len());
    let split = chat_records.len().saturating_sub(take);
    let selected = &chat_records[split..];
    let events = selected.iter().map(|record| record.event.clone()).collect::<Vec<_>>();
    let live_cursor = file_len(Path::new(log_path))?.to_string();
    let history_cursor = selected.first().map(|record| record.start.to_string());
    Ok(ApiMessagesTailResponse {
        thread_id: session.thread_id,
        log_path: Some(log_path.to_string()),
        live_cursor: Some(live_cursor),
        history_cursor,
        events,
        has_older: split > 0,
        busy: session.busy,
        queue_len: session.queue_len,
        token: session.token,
    })
}

pub fn load_messages_history(
    config: &RuntimeConfig,
    session_id: &str,
    before_cursor: &str,
    limit: usize,
) -> Result<ApiMessagesHistoryResponse, String> {
    let session = find_session(config, session_id)?;
    let Some(log_path) = session.log_path.as_deref() else {
        return Ok(ApiMessagesHistoryResponse {
            thread_id: session.thread_id,
            log_path: None,
            history_cursor: None,
            events: vec![],
            has_older: false,
            busy: session.busy,
            queue_len: session.queue_len,
            token: session.token,
        });
    };
    let before = parse_cursor(before_cursor)?;
    let records = chat_records(read_positioned_records(Path::new(log_path))?);
    let older = records
        .into_iter()
        .filter(|record| record.start < before)
        .collect::<Vec<_>>();
    let take = limit.max(1).min(older.len());
    let split = older.len().saturating_sub(take);
    let selected = &older[split..];
    Ok(ApiMessagesHistoryResponse {
        thread_id: session.thread_id,
        log_path: Some(log_path.to_string()),
        history_cursor: selected.first().map(|record| record.start.to_string()),
        events: selected.iter().map(|record| record.event.clone()).collect(),
        has_older: split > 0,
        busy: session.busy,
        queue_len: session.queue_len,
        token: session.token,
    })
}

pub fn load_messages_live(
    config: &RuntimeConfig,
    session_id: &str,
    after_cursor: &str,
) -> Result<ApiMessagesLiveResponse, String> {
    let session = find_session(config, session_id)?;
    let Some(log_path) = session.log_path.as_deref() else {
        return Ok(ApiMessagesLiveResponse {
            thread_id: session.thread_id,
            log_path: None,
            live_cursor: None,
            events: vec![],
            meta_delta: json!({"thinking": 0, "tool": 0, "system": 0}),
            turn_start: false,
            turn_end: false,
            turn_aborted: false,
            diag: json!({"pending_log": true}),
            busy: session.busy,
            queue_len: session.queue_len,
            token: session.token,
        });
    };
    let after = parse_cursor(after_cursor)?;
    let records = read_positioned_records(Path::new(log_path))?;
    let mut turn_start = false;
    let mut turn_end = false;
    let mut turn_aborted = false;
    let events = records
        .into_iter()
        .filter(|record| record.start >= after)
        .filter_map(|record| {
            update_turn_flags(&record.obj, &mut turn_start, &mut turn_end, &mut turn_aborted);
            chat_event_from_obj(&record.obj)
        })
        .collect::<Vec<_>>();
    Ok(ApiMessagesLiveResponse {
        thread_id: session.thread_id,
        log_path: Some(log_path.to_string()),
        live_cursor: Some(file_len(Path::new(log_path))?.to_string()),
        events,
        meta_delta: json!({"thinking": 0, "tool": 0, "system": 0}),
        turn_start,
        turn_end,
        turn_aborted,
        diag: json!({}),
        busy: session.busy,
        queue_len: session.queue_len,
        token: session.token,
    })
}

pub fn load_preview_bootstrap(config: &RuntimeConfig) -> Result<(Vec<SessionSummary>, HashMap<String, SessionDetail>, String), String> {
    let sessions_response = load_sessions_response(config)?;
    let selected = sessions_response
        .sessions
        .first()
        .map(|session| session.session_id.clone())
        .unwrap_or_default();
    let sessions = sessions_response
        .sessions
        .into_iter()
        .map(|session| SessionSummary {
            id: session.session_id.clone(),
            title: if session.alias.is_empty() {
                session.session_id.clone()
            } else {
                session.alias.clone()
            },
            backend: session.agent_backend,
            status: if session.busy { "running".into() } else { "idle".into() },
            workspace: session.cwd,
            transport: session.transport,
            tmux_session: session.tmux_session,
            tmux_window: session.tmux_window,
            last_line: String::new(),
            unread: 0,
            updated_at: session.updated_ts,
        })
        .collect();
    Ok((sessions, HashMap::new(), selected))
}

pub fn load_diagnostics_response(config: &RuntimeConfig, session_id: &str) -> Result<ApiDiagnosticsResponse, String> {
    let session = find_session(config, session_id)?;
    let now = epoch_now();
    let time_priority = priority_from_elapsed_seconds((now - session.updated_ts).max(0.0));
    let base_priority = clip01(time_priority + session.priority_offset);
    let blocked = session.dependency_session_id.is_some();
    let snoozed = session.snooze_until.map(|value| value > now).unwrap_or(false);
    let final_priority = if blocked || snoozed { 0.0 } else { base_priority };
    Ok(ApiDiagnosticsResponse {
        session_id: session.session_id,
        thread_id: session.thread_id,
        agent_backend: session.agent_backend,
        owned: session.owned,
        transport: session.transport,
        cwd: session.cwd.clone(),
        start_ts: session.start_ts,
        updated_ts: session.updated_ts,
        log_path: session.log_path,
        broker_pid: session.broker_pid,
        codex_pid: session.pid,
        busy: session.busy,
        broker_busy: session.busy,
        queue_len: session.queue_len,
        token: session.token,
        model_provider: session.model_provider.clone(),
        preferred_auth_method: session.preferred_auth_method.clone(),
        provider_choice: provider_choice_for_settings(
            session.model_provider.as_deref(),
            session.preferred_auth_method.as_deref(),
        ),
        model: session.model,
        reasoning_effort: session.reasoning_effort,
        service_tier: session.service_tier,
        tmux_session: session.tmux_session,
        tmux_window: session.tmux_window,
        git_branch: current_git_branch(Path::new(&session.cwd)),
        time_priority,
        base_priority,
        final_priority,
        priority_offset: session.priority_offset,
        snooze_until: session.snooze_until,
        dependency_session_id: session.dependency_session_id,
    })
}

pub fn load_queue_response(config: &RuntimeConfig, session_id: &str) -> Result<ApiQueueResponse, String> {
    let _ = find_session(config, session_id)?;
    let queues = read_array_map(&config.app_dir.join("session_queues.json"))?;
    let items = queues
        .get(session_id)
        .into_iter()
        .flat_map(|values| values.iter())
        .map(queue_item_from_value)
        .collect::<Vec<_>>();
    let queue = items.iter().map(|item| item.text.clone()).collect::<Vec<_>>();
    Ok(ApiQueueResponse {
        ok: true,
        items,
        queue,
    })
}

pub fn load_harness_response(config: &RuntimeConfig, session_id: &str) -> Result<ApiHarnessResponse, String> {
    let _ = find_session(config, session_id)?;
    let harness = read_object_map(&config.app_dir.join("harness.json"))?;
    let entry = harness.get(session_id);
    Ok(ApiHarnessResponse {
        ok: true,
        enabled: object_bool(entry, "enabled").unwrap_or(false),
        request: object_string(entry, "request").unwrap_or_default(),
        cooldown_minutes: object_number(entry, "cooldown_minutes").unwrap_or(HARNESS_DEFAULT_IDLE_MINUTES),
        remaining_injections: object_i64(entry, "remaining_injections").unwrap_or(HARNESS_DEFAULT_MAX_INJECTIONS),
    })
}

pub fn load_file_read_response(config: &RuntimeConfig, session_id: &str, raw_path: &str) -> Result<Value, String> {
    let session = find_session(config, session_id)?;
    let resolved = resolve_session_path(&session.cwd, raw_path)?;
    let rel = raw_path.trim().to_string();
    let abs = resolved.display().to_string();
    let metadata = fs::metadata(&resolved).map_err(map_io_error)?;
    if !metadata.is_file() {
        return Err("path is not a file".to_string());
    }
    let size = metadata.len();
    let prefix = read_prefix(&resolved, 4096)?;
    let (kind, content_type) = detect_file_kind(&resolved, &prefix);
    if kind == "image" {
        return Ok(json!({
            "ok": true,
            "kind": "image",
            "content_type": content_type,
            "path": abs,
            "rel": rel,
            "size": size,
            "image_url": format!("/api/v1/sessions/{session_id}/file/blob?path={}", url_encode(raw_path)),
        }));
    }
    if kind == "pdf" {
        return Ok(json!({
            "ok": true,
            "kind": "pdf",
            "content_type": content_type,
            "path": abs,
            "rel": rel,
            "size": size,
            "pdf_url": format!("/api/v1/sessions/{session_id}/file/blob?path={}", url_encode(raw_path)),
        }));
    }
    if size > FILE_READ_MAX_BYTES {
        return Ok(json!({
            "ok": true,
            "kind": "download_only",
            "path": abs,
            "rel": rel,
            "size": size,
            "reason": "too_large",
            "viewer_max_bytes": FILE_READ_MAX_BYTES,
        }));
    }
    let raw = fs::read(&resolved).map_err(map_io_error)?;
    if let Some((text, editable)) = decode_text_view(&resolved, &raw) {
        return Ok(json!({
            "ok": true,
            "kind": "text",
            "path": abs,
            "rel": rel,
            "size": size,
            "text": text,
            "editable": editable,
            "version": content_version(&raw),
        }));
    }
    Ok(json!({
        "ok": true,
        "kind": "download_only",
        "path": abs,
        "rel": rel,
        "size": size,
        "reason": "binary",
    }))
}

pub fn load_file_blob(config: &RuntimeConfig, session_id: &str, raw_path: &str) -> Result<(Vec<u8>, String), String> {
    let session = find_session(config, session_id)?;
    let resolved = resolve_session_path(&session.cwd, raw_path)?;
    let raw = fs::read(&resolved).map_err(map_io_error)?;
    let (_kind, content_type) = detect_file_kind(&resolved, &raw);
    match content_type {
        Some(value) if value.starts_with("image/") || value == "application/pdf" => Ok((raw, value.to_string())),
        _ => Err("file is not previewable inline".to_string()),
    }
}

fn session_from_meta(
    session_id: &str,
    sock_path: &Path,
    meta: SessionMeta,
    aliases: &HashMap<String, String>,
    queues: &HashMap<String, Vec<Value>>,
    harness: &HashMap<String, Value>,
    sidebar: &HashMap<String, Value>,
    files: &HashMap<String, Vec<String>>,
) -> Result<Option<ApiSessionSummary>, String> {
    let cwd = required_string(meta.cwd, "cwd", session_id)?;
    let start_ts = meta
        .start_ts
        .ok_or_else(|| format!("missing start_ts in metadata for session {session_id}"))?;
    let pid = meta
        .codex_pid
        .ok_or_else(|| format!("missing codex_pid in metadata for session {session_id}"))?;
    let broker_pid = meta
        .broker_pid
        .ok_or_else(|| format!("missing broker_pid in metadata for session {session_id}"))?;
    let agent_backend = normalize_backend(meta.agent_backend.as_deref())?;
    let transport = clean_optional(meta.transport).or_else(|| {
        if meta.tmux_session.as_deref().is_some() || meta.tmux_window.as_deref().is_some() {
            Some("tmux".to_string())
        } else {
            None
        }
    });
    let harness_entry = harness.get(session_id);
    let sidebar_entry = sidebar.get(session_id);
    let priority_offset = object_number(sidebar_entry, "priority_offset").unwrap_or(0.0).clamp(-1.0, 1.0);
    let blocked = object_string(sidebar_entry, "dependency_session_id").is_some();
    let snooze_until = object_number(sidebar_entry, "snooze_until");
    let snoozed = snooze_until.map(|value| value > epoch_now()).unwrap_or(false);
    let queue_len = queues.get(session_id).map(|items| items.len()).unwrap_or(0);
    let thread_id = clean_optional(meta.session_id).unwrap_or_else(|| session_id.to_string());
    let mut model_provider = clean_optional(meta.model_provider);
    let preferred_auth_method = clean_optional(meta.preferred_auth_method);
    let mut model = clean_optional(meta.model);
    let mut reasoning_effort = clean_optional(meta.reasoning_effort);
    let service_tier = clean_optional(meta.service_tier);
    let log_path = clean_optional(meta.log_path);
    let broker_state = match read_broker_state(sock_path) {
        Ok(state) => state,
        Err(_) if !pid_alive(pid) && !pid_alive(broker_pid) => return Ok(None),
        Err(_) => None,
    };
    if (model_provider.is_none() || model.is_none() || reasoning_effort.is_none()) && log_path.as_deref().is_some() {
        let log = Path::new(log_path.as_deref().unwrap_or_default());
        if log.exists() {
            let (log_provider, log_model, log_effort) = read_run_settings_from_log(log, &agent_backend);
            if model_provider.is_none() {
                model_provider = log_provider;
            }
            if model.is_none() {
                model = log_model;
            }
            if reasoning_effort.is_none() {
                reasoning_effort = log_effort;
            }
        }
    }
    let updated_ts = log_path
        .as_deref()
        .map(Path::new)
        .filter(|path| path.exists())
        .and_then(last_conversation_ts_from_log)
        .unwrap_or(start_ts);
    let time_priority = priority_from_elapsed_seconds((epoch_now() - updated_ts).max(0.0));
    let base_priority = clip01(time_priority + priority_offset);
    let final_priority = if blocked || snoozed { 0.0 } else { base_priority };
    let busy = log_path
        .as_deref()
        .map(Path::new)
        .filter(|path| path.exists())
        .and_then(compute_idle_from_log)
        .map(|idle| !idle)
        .unwrap_or(false);
    let token = broker_state
        .as_ref()
        .and_then(|state| state.token.clone())
        .or_else(|| log_path.as_deref().map(Path::new).filter(|path| path.exists()).and_then(latest_token_update_from_log));
    let last_assistant_ts = log_path
        .as_deref()
        .map(Path::new)
        .filter(|path| path.exists())
        .and_then(last_assistant_ts_from_log);
    Ok(Some(ApiSessionSummary {
        session_id: session_id.to_string(),
        thread_id: Some(thread_id),
        pid,
        broker_pid,
        agent_backend,
        owned: meta.owner.as_deref() == Some("web"),
        transport,
        cwd,
        start_ts,
        updated_ts,
        log_path: log_path.clone(),
        queue_len,
        busy,
        token,
        harness_enabled: object_bool(harness_entry, "enabled").unwrap_or(false),
        harness_cooldown_minutes: object_number(harness_entry, "cooldown_minutes").unwrap_or(HARNESS_DEFAULT_IDLE_MINUTES),
        harness_remaining_injections: object_i64(harness_entry, "remaining_injections").unwrap_or(HARNESS_DEFAULT_MAX_INJECTIONS),
        alias: aliases.get(session_id).cloned().unwrap_or_default(),
        files: files.get(session_id).cloned().unwrap_or_default(),
        git_branch: current_git_branch(Path::new(&cwd)),
        model_provider: model_provider.clone(),
        preferred_auth_method: preferred_auth_method.clone(),
        provider_choice: provider_choice_for_settings(model_provider.as_deref(), preferred_auth_method.as_deref()),
        model,
        reasoning_effort,
        service_tier,
        tmux_session: clean_optional(meta.tmux_session),
        tmux_window: clean_optional(meta.tmux_window),
        priority_offset,
        snooze_until,
        dependency_session_id: object_string(sidebar_entry, "dependency_session_id"),
        time_priority,
        base_priority,
        final_priority,
        blocked,
        snoozed,
        last_assistant_ts,
    }))
}

fn find_session(config: &RuntimeConfig, session_id: &str) -> Result<ApiSessionSummary, String> {
    load_sessions_response(config)?
        .sessions
        .into_iter()
        .find(|session| session.session_id == session_id)
        .ok_or_else(|| format!("unknown session: {session_id}"))
}

fn queue_item_from_value(value: &Value) -> ApiQueueItem {
    let created_ts = value
        .get("created_ts")
        .and_then(Value::as_f64)
        .filter(|ts| ts.is_finite() && *ts > 0.0)
        .unwrap_or_else(epoch_now);
    ApiQueueItem {
        id: value.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
        text: value.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
        created_ts,
        sending: value.get("sending").and_then(Value::as_bool).unwrap_or(false),
    }
}

fn map_io_error(err: std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::NotFound => "file not found".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        _ => err.to_string(),
    }
}

fn resolve_session_path(cwd: &str, raw_path: &str) -> Result<PathBuf, String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("path required".to_string());
    }
    if trimmed.contains('\0') {
        return Err("invalid path".to_string());
    }
    let candidate = expand_home(trimmed);
    let joined = if candidate.is_absolute() {
        candidate
    } else {
        expand_home(cwd).join(candidate)
    };
    Ok(fs::canonicalize(&joined).unwrap_or(joined))
}

fn expand_home(raw: &str) -> PathBuf {
    if raw == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(raw));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(raw)
}

fn home_dir() -> Option<PathBuf> {
    env::var("HOME").ok().map(PathBuf::from)
}

fn read_prefix(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;

    let mut file = fs::File::open(path).map_err(map_io_error)?;
    let mut buf = vec![0_u8; limit];
    let read = file.read(&mut buf).map_err(map_io_error)?;
    buf.truncate(read);
    Ok(buf)
}

fn detect_file_kind(path: &Path, raw: &[u8]) -> (&'static str, Option<&'static str>) {
    if path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.eq_ignore_ascii_case("svg")).unwrap_or(false) {
        return ("image", Some("image/svg+xml; charset=utf-8"));
    }
    if raw.starts_with(b"\x89PNG\r\n\x1a\n") {
        return ("image", Some("image/png"));
    }
    if raw.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return ("image", Some("image/jpeg"));
    }
    if raw.len() >= 12 && &raw[..4] == b"RIFF" && &raw[8..12] == b"WEBP" {
        return ("image", Some("image/webp"));
    }
    if path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.eq_ignore_ascii_case("pdf")).unwrap_or(false)
        || raw.starts_with(b"%PDF-")
    {
        return ("pdf", Some("application/pdf"));
    }
    ("text", None)
}

fn decode_text_view(path: &Path, raw: &[u8]) -> Option<(String, bool)> {
    if raw.contains(&0) {
        return None;
    }
    if let Ok(text) = std::str::from_utf8(raw) {
        return Some((text.to_string(), true));
    }
    if !path_looks_textual(path) && !looks_like_text_bytes(raw) {
        return None;
    }
    Some((String::from_utf8_lossy(raw).into_owned(), false))
}

fn content_version(raw: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn path_looks_textual(path: &Path) -> bool {
    let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or_default().to_ascii_lowercase();
    if TEXTUAL_EXTENSIONS.iter().any(|candidate| *candidate == ext) {
        return true;
    }
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_ascii_lowercase();
    TEXTUAL_FILENAMES.iter().any(|candidate| *candidate == name)
}

fn looks_like_text_bytes(raw: &[u8]) -> bool {
    raw.iter()
        .all(|byte| *byte >= 32 || matches!(*byte, 9 | 10 | 12 | 13 | 27))
}

fn url_encode(raw: &str) -> String {
    let mut out = String::new();
    for byte in raw.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~' | '/') {
            out.push(ch);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

fn current_git_branch(cwd: &Path) -> Option<String> {
    Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn provider_choice_for_settings(model_provider: Option<&str>, preferred_auth_method: Option<&str>) -> Option<String> {
    let provider = model_provider.unwrap_or("openai");
    if provider == "openai" {
        Some(if preferred_auth_method == Some("chatgpt") {
            "chatgpt".to_string()
        } else {
            "openai-api".to_string()
        })
    } else {
        Some(provider.to_string())
    }
}

fn priority_from_elapsed_seconds(elapsed_s: f64) -> f64 {
    if elapsed_s <= 0.0 {
        return 1.0;
    }
    clip01((-SIDEBAR_PRIORITY_LAMBDA * elapsed_s).exp())
}

fn clip01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

struct PositionedRecord {
    start: u64,
    obj: Value,
}

struct PositionedChatRecord {
    start: u64,
    event: Value,
}

fn read_positioned_records(path: &Path) -> Result<Vec<PositionedRecord>, String> {
    let raw = fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let mut records = Vec::new();
    let mut offset = 0_u64;
    for line in raw.split_inclusive('\n') {
        let start = offset;
        offset += line.len() as u64;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let obj = serde_json::from_str(trimmed).map_err(|err| format!("parse {} at byte {start}: {err}", path.display()))?;
        records.push(PositionedRecord { start, obj });
    }
    Ok(records)
}

fn file_len(path: &Path) -> Result<u64, String> {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|err| format!("stat {}: {err}", path.display()))
}

fn chat_records(records: Vec<PositionedRecord>) -> Vec<PositionedChatRecord> {
    records
        .into_iter()
        .filter_map(|record| {
            chat_event_from_obj(&record.obj).map(|event| PositionedChatRecord {
                start: record.start,
                event,
            })
        })
        .collect()
}

fn chat_event_from_obj(obj: &Value) -> Option<Value> {
    let typ = obj.get("type")?.as_str()?;
    match typ {
        "event_msg" => event_msg_chat_event(obj),
        "response_item" => response_item_chat_event(obj),
        "message" => pi_message_chat_event(obj),
        _ => None,
    }
}

fn event_msg_chat_event(obj: &Value) -> Option<Value> {
    let payload = obj.get("payload")?.as_object()?;
    let payload_type = payload.get("type")?.as_str()?;
    match payload_type {
        "user_message" => Some(json_text_event("user", payload.get("message")?.as_str()?, event_ts(obj), None)),
        "agent_message" => {
            let class = if payload.get("phase").and_then(Value::as_str) == Some("final_answer") {
                Some("final_response")
            } else {
                Some("narration")
            };
            Some(json_text_event("assistant", payload.get("message")?.as_str()?, event_ts(obj), class))
        }
        _ => None,
    }
}

fn response_item_chat_event(obj: &Value) -> Option<Value> {
    let payload = obj.get("payload")?.as_object()?;
    if payload.get("type")?.as_str()? != "message" {
        return None;
    }
    if payload.get("role")?.as_str()? != "assistant" {
        return None;
    }
    let text = output_text(payload.get("content")?)?;
    let class = if payload.get("phase").and_then(Value::as_str) == Some("final_answer")
        || payload.get("end_turn").and_then(Value::as_bool) == Some(true)
    {
        Some("final_response")
    } else {
        Some("narration")
    };
    Some(json_text_event("assistant", &text, event_ts(obj), class))
}

fn pi_message_chat_event(obj: &Value) -> Option<Value> {
    let message = obj.get("message")?.as_object()?;
    let role = message.get("role")?.as_str()?;
    match role {
        "user" => Some(json_text_event("user", &content_text(message.get("content")?)?, event_ts(obj), None)),
        "assistant" => {
            let text = content_text(message.get("content")?)?;
            let class = if message.get("stopReason").is_some() {
                Some("final_response")
            } else {
                Some("narration")
            };
            Some(json_text_event("assistant", &text, event_ts(obj), class))
        }
        _ => None,
    }
}

fn json_text_event(role: &str, text: &str, ts: Option<f64>, message_class: Option<&str>) -> Value {
    let mut event = json!({"role": role, "text": text});
    if let Some(ts_value) = ts {
        event["ts"] = json!(ts_value);
    }
    if let Some(class) = message_class {
        event["message_class"] = json!(class);
    }
    event
}

fn output_text(value: &Value) -> Option<String> {
    let parts = value.as_array()?;
    let text = parts
        .iter()
        .filter_map(|part| {
            if part.get("type").and_then(Value::as_str) == Some("output_text") {
                part.get("text").and_then(Value::as_str)
            } else {
                None
            }
        })
        .collect::<String>();
    if text.is_empty() { None } else { Some(text) }
}

fn content_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    let parts = value.as_array()?;
    let text = parts
        .iter()
        .filter_map(|part| {
            let kind = part.get("type").and_then(Value::as_str);
            if kind == Some("text") || kind == Some("output_text") || kind == Some("input_text") {
                part.get("text").and_then(Value::as_str)
            } else {
                None
            }
        })
        .collect::<String>();
    if text.is_empty() { None } else { Some(text) }
}

fn update_turn_flags(obj: &Value, turn_start: &mut bool, turn_end: &mut bool, turn_aborted: &mut bool) {
    if let Some(payload) = obj.get("payload").and_then(Value::as_object) {
        match payload.get("type").and_then(Value::as_str) {
            Some("user_message") => *turn_start = true,
            Some("task_complete") | Some("turn_complete") => *turn_end = true,
            Some("turn_aborted") => *turn_aborted = true,
            _ => {}
        }
    }
}

fn event_ts(obj: &Value) -> Option<f64> {
    obj.get("ts")
        .and_then(Value::as_f64)
        .or_else(|| obj.get("timestamp").and_then(Value::as_f64))
}

fn parse_cursor(raw: &str) -> Result<u64, String> {
    raw.trim()
        .parse::<u64>()
        .map_err(|_| format!("invalid message cursor: {raw:?}"))
}

fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let raw = fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    serde_json::from_str(&raw).map_err(|err| format!("parse {}: {err}", path.display()))
}

fn read_optional_value(path: &Path) -> Result<Option<Value>, String> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|err| format!("parse {}: {err}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("read {}: {err}", path.display())),
    }
}

fn read_string_map(path: &Path) -> Result<HashMap<String, String>, String> {
    let Some(Value::Object(object)) = read_optional_value(path)? else {
        return Ok(HashMap::new());
    };
    Ok(object
        .into_iter()
        .filter_map(|(key, value)| value.as_str().map(|text| (key, text.to_string())))
        .collect())
}

fn read_string_array_map(path: &Path) -> Result<HashMap<String, Vec<String>>, String> {
    let Some(Value::Object(object)) = read_optional_value(path)? else {
        return Ok(HashMap::new());
    };
    Ok(object
        .into_iter()
        .filter_map(|(key, value)| {
            let items = value
                .as_array()?
                .iter()
                .filter_map(|item| item.as_str().map(|text| text.to_string()))
                .collect::<Vec<_>>();
            Some((key, items))
        })
        .collect())
}

fn read_array_map(path: &Path) -> Result<HashMap<String, Vec<Value>>, String> {
    let Some(Value::Object(object)) = read_optional_value(path)? else {
        return Ok(HashMap::new());
    };
    Ok(object
        .into_iter()
        .filter_map(|(key, value)| value.as_array().map(|items| (key, items.clone())))
        .collect())
}

fn read_object_map(path: &Path) -> Result<HashMap<String, Value>, String> {
    let Some(Value::Object(object)) = read_optional_value(path)? else {
        return Ok(HashMap::new());
    };
    Ok(object.into_iter().collect())
}

fn read_hidden_sessions(path: &Path) -> Result<HashSet<String>, String> {
    let Some(Value::Array(items)) = read_optional_value(path)? else {
        return Ok(HashSet::new());
    };
    Ok(items
        .into_iter()
        .filter_map(|value| value.as_str().map(|text| text.to_string()))
        .collect())
}

fn read_recent_cwds(path: &Path) -> Result<Vec<String>, String> {
    let Some(Value::Object(object)) = read_optional_value(path)? else {
        return Ok(Vec::new());
    };
    let mut rows = object
        .into_iter()
        .filter_map(|(cwd, value)| value.as_f64().map(|ts| (cwd, ts)))
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(rows.into_iter().map(|(cwd, _)| cwd).collect())
}

fn required_string(value: Option<String>, field: &str, session_id: &str) -> Result<String, String> {
    let cleaned = clean_optional(value);
    cleaned.ok_or_else(|| format!("missing {field} in metadata for session {session_id}"))
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn normalize_backend(value: Option<&str>) -> Result<String, String> {
    let raw = value.unwrap_or("codex").trim().to_ascii_lowercase();
    match raw.as_str() {
        "" => Ok("codex".to_string()),
        "codex" | "pi" => Ok(raw),
        _ => Err(format!("agent_backend must be codex or pi, got {raw:?}")),
    }
}

fn object_bool(value: Option<&Value>, key: &str) -> Option<bool> {
    value?.as_object()?.get(key)?.as_bool()
}

fn object_number(value: Option<&Value>, key: &str) -> Option<f64> {
    value?.as_object()?.get(key)?.as_f64()
}

fn object_i64(value: Option<&Value>, key: &str) -> Option<i64> {
    value?.as_object()?.get(key)?.as_i64()
}

fn object_string(value: Option<&Value>, key: &str) -> Option<String> {
    value?.as_object()?.get(key)?.as_str().map(|text| text.to_string())
}

fn epoch_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn new_session_defaults() -> ApiNewSessionDefaults {
    let mut backends = HashMap::new();
    backends.insert(
        "codex".to_string(),
        ApiBackendDefaults {
            agent_backend: "codex".to_string(),
            model_provider: None,
            preferred_auth_method: None,
            provider_choice: None,
            provider_choices: vec![],
            model: None,
            models: vec![],
            reasoning_effort: "medium".to_string(),
            reasoning_efforts: vec!["low".to_string(), "medium".to_string(), "high".to_string()],
            service_tier: None,
            supports_fast: true,
        },
    );
    backends.insert(
        "pi".to_string(),
        ApiBackendDefaults {
            agent_backend: "pi".to_string(),
            model_provider: None,
            preferred_auth_method: None,
            provider_choice: None,
            provider_choices: vec![],
            model: None,
            models: vec![],
            reasoning_effort: "medium".to_string(),
            reasoning_efforts: vec!["low".to_string(), "medium".to_string(), "high".to_string()],
            service_tier: None,
            supports_fast: false,
        },
    );
    ApiNewSessionDefaults {
        default_backend: "codex".to_string(),
        backends,
    }
}

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{load_messages_history, load_messages_live, load_messages_tail, load_sessions_response, RuntimeConfig};
    use std::fs;
    use std::path::PathBuf;

    fn temp_app_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "codoxear-rs-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(path.join("socks")).unwrap();
        path
    }

    #[test]
    fn load_sessions_reads_socket_sidecar_metadata() {
        let app_dir = temp_app_dir("sessions");
        fs::write(app_dir.join("socks").join("sid-a.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-a.json"),
            r#"{
              "session_id": "thread-a",
              "codex_pid": 101,
              "broker_pid": 202,
              "agent_backend": "codex",
              "owner": "web",
              "transport": "tmux",
              "cwd": "/work/project",
              "log_path": "/logs/rollout-a.jsonl",
              "start_ts": 10.0,
              "updated_ts": 15.0,
              "model_provider": "crs",
              "model": "gpt-5.5",
              "reasoning_effort": "high",
              "tmux_session": "codoxear",
              "tmux_window": "1"
            }"#,
        )
        .unwrap();
        fs::write(app_dir.join("session_aliases.json"), r#"{"sid-a":"Alias A"}"#).unwrap();
        fs::write(app_dir.join("session_queues.json"), r#"{"sid-a":[{"id":"q1","text":"next"}]}"#).unwrap();
        fs::write(
            app_dir.join("harness.json"),
            r#"{"sid-a":{"enabled":true,"cooldown_minutes":3.5,"remaining_injections":2}}"#,
        )
        .unwrap();
        fs::write(app_dir.join("session_files.json"), r#"{"sid-a":["README.md"]}"#).unwrap();

        let response = load_sessions_response(&RuntimeConfig { app_dir }).unwrap();
        assert_eq!(response.sessions.len(), 1);
        let session = &response.sessions[0];
        assert_eq!(session.session_id, "sid-a");
        assert_eq!(session.thread_id.as_deref(), Some("thread-a"));
        assert_eq!(session.pid, 101);
        assert_eq!(session.broker_pid, 202);
        assert_eq!(session.agent_backend, "codex");
        assert!(session.owned);
        assert_eq!(session.transport.as_deref(), Some("tmux"));
        assert_eq!(session.alias, "Alias A");
        assert_eq!(session.queue_len, 1);
        assert!(session.harness_enabled);
        assert_eq!(session.files, vec!["README.md".to_string()]);
        assert_eq!(session.model_provider.as_deref(), Some("crs"));
        assert_eq!(session.tmux_session.as_deref(), Some("codoxear"));
    }

    #[test]
    fn load_sessions_skips_hidden_sessions() {
        let app_dir = temp_app_dir("hidden");
        fs::write(app_dir.join("socks").join("sid-hidden.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-hidden.json"),
            r#"{"codex_pid":1,"broker_pid":2,"cwd":"/work","start_ts":1.0}"#,
        )
        .unwrap();
        fs::write(app_dir.join("hidden_sessions.json"), r#"["sid-hidden"]"#).unwrap();

        let response = load_sessions_response(&RuntimeConfig { app_dir }).unwrap();
        assert!(response.sessions.is_empty());
    }

    #[test]
    fn message_pages_read_common_rollout_text_events() {
        let app_dir = temp_app_dir("messages");
        let log_path = app_dir.join("rollout.jsonl");
        fs::write(
            &log_path,
            [
                r#"{"type":"event_msg","payload":{"type":"user_message","message":"hello"},"ts":1.0}"#,
                r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"working"}]},"ts":2.0}"#,
                r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}],"phase":"final_answer"},"ts":3.0}"#,
            ]
            .join("\n")
                + "\n",
        )
        .unwrap();
        fs::write(app_dir.join("socks").join("sid-msg.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-msg.json"),
            format!(
                r#"{{"session_id":"thread-msg","codex_pid":1,"broker_pid":2,"cwd":"/work","log_path":"{}","start_ts":1.0}}"#,
                log_path.display()
            ),
        )
        .unwrap();
        let config = RuntimeConfig { app_dir };

        let tail = load_messages_tail(&config, "sid-msg", 2).unwrap();
        assert_eq!(tail.events.len(), 2);
        assert_eq!(tail.events[0]["text"], "working");
        assert_eq!(tail.events[1]["message_class"], "final_response");
        assert!(tail.has_older);

        let history = load_messages_history(&config, "sid-msg", tail.history_cursor.as_deref().unwrap(), 10).unwrap();
        assert_eq!(history.events.len(), 1);
        assert_eq!(history.events[0]["role"], "user");

        let live = load_messages_live(&config, "sid-msg", "0").unwrap();
        assert_eq!(live.events.len(), 3);
        assert_eq!(live.live_cursor, tail.live_cursor);
    }
}
