use crate::models::{
    ApiBackendDefaults, ApiChangedFileEntry, ApiChangedFilesResponse, ApiDiagnosticsResponse,
    ApiFileSearchMatch, ApiFileSearchResponse, ApiGitDiffResponse, ApiGitFileVersionsResponse,
    ApiHarnessResponse, ApiMessagesHistoryResponse, ApiMessagesLiveResponse,
    ApiMessagesTailResponse, ApiNewSessionDefaults, ApiQueueItem, ApiQueueResponse,
    ApiSessionSummary, ApiSessionsResponse, SessionDetail, SessionSummary,
};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::cmp::Reverse;
use std::collections::{hash_map::DefaultHasher, BinaryHeap, HashMap, HashSet};
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Component;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::task;
use tokio::time::{self, MissedTickBehavior};
use toml::Value as TomlValue;

const APP_VERSION: &str = "0.1.0";
const CONTEXT_WINDOW_BASELINE_TOKENS: i64 = 12000;
const DEFAULT_SUMMARIZATION_MODEL: &str = "gpt-4.1-mini";
const DEFAULT_TTS_MODEL: &str = "gpt-4o-mini-tts";
const DEFAULT_TTS_BASE_URL: &str = "https://api.openai.com/v1";
const LISTENER_TTL_SECONDS: f64 = 45.0;
const HARNESS_DEFAULT_IDLE_MINUTES: f64 = 5.0;
const HARNESS_DEFAULT_MAX_INJECTIONS: i64 = 10;
const STATIC_ASSET_VERSION_PLACEHOLDER: &str = "__CODOXEAR_ASSET_VERSION__";
const STATIC_ATTACH_MAX_BYTES_PLACEHOLDER: &str = "__CODOXEAR_ATTACH_MAX_BYTES__";
const STATIC_ASSET_VERSION_FILES: &[&str] = &["app.js", "app.css"];
const ATTACH_UPLOAD_MAX_BYTES: usize = 16 * 1024 * 1024;
const FILE_READ_MAX_BYTES: u64 = 2 * 1024 * 1024;
const FILE_SEARCH_LIMIT: usize = 120;
const FILE_SEARCH_TIMEOUT_SECONDS: f64 = 0.75;
const FILE_SEARCH_MAX_CANDIDATES: usize = 200000;
const GIT_DIFF_MAX_BYTES: usize = 800 * 1024;
const GIT_DIFF_TIMEOUT_SECONDS: f64 = 4.0;
const GIT_CHANGED_FILES_MAX: usize = 400;
const LOCAL_SERVICE_RESTART_DELAY_SECONDS: f64 = 0.75;
const TERMINAL_INTERRUPT_SEQ: &str = "\\x03";
const DEFAULT_HARNESS_SWEEP_SECONDS: f64 = 2.5;
const DEFAULT_QUEUE_SWEEP_SECONDS: f64 = 1.0;
const DEFAULT_VOICE_SCAN_SECONDS: f64 = 1.0;
const DEFAULT_QUEUE_IDLE_GRACE_SECONDS: f64 = 10.0;
const DEFAULT_HARNESS_MAX_SCAN_BYTES: usize = 8 * 1024 * 1024;
const SIDEBAR_PRIORITY_HALF_LIFE_SECONDS: f64 = 8.0 * 3600.0;
const SIDEBAR_PRIORITY_LAMBDA: f64 = std::f64::consts::LN_2 / SIDEBAR_PRIORITY_HALF_LIFE_SECONDS;
const SUPPORTED_REASONING_EFFORTS: &[&str] = &["xhigh", "high", "medium", "low"];
const SUPPORTED_PI_REASONING_EFFORTS: &[&str] =
    &["off", "minimal", "low", "medium", "high", "xhigh"];
const BUILTIN_PI_PROVIDER_CHOICES: &[&str] = &[
    "anthropic",
    "openai-codex",
    "github-copilot",
    "google-gemini-cli",
    "google-antigravity",
];
const TMUX_META_WAIT_SECONDS: f64 = 10.0;
const HARNESS_PROMPT_PREFIX: &str = r#"Unattended-mode instructions (optimize for 8+ hours, minimal turns, minimal repetition, maximal progress)

- Maintain four internal sections:
  1. Deliverables
     - The concrete outputs the agent owes the user by the end of the task.
     - Stable unless the user changes the request.
  2. Completed
     - Verified facts already established while producing the Deliverables.
  3. Next actions
     - Ordered concrete steps from the current state toward the Deliverables.
  4. Parked user decisions
     - Decisions or inputs that only the user can provide.

- Working rules:
  - Keep these sections internal. Surface them only when yielding is necessary.
  - Default to continuing in the same turn.
  - Before each action, reason until the approach, failure modes, and verification path are clear.
  - Exploration should happen through reading, tracing, inspection, and reasoning.
  - Avoid trial and error.
  - Resolve crashes, bugs, and design mistakes yourself unless a true user decision is required.
  - Use the strongest available verification.
  - Do not repeat the same command, edit, or analysis without a concrete new reason.

- Yield only when:
  - all Deliverables are finished and supported by Completed;
  - the only remaining gap is a Parked user decision;
  - or the next step is irreversible or high-risk and needs explicit user confirmation.

- End-of-turn gate (only when yielding is necessary):
  - Run a clean-room adversarial review via a dedicated subagent.
  - Give it: user intent, Deliverables, Completed, remaining Next actions, Parked user decisions, constraints, and changed artifacts.
  - Apply findings before yielding, or surface the exact remaining user decision or risk.
"#;
static QUEUE_ITEM_COUNTER: AtomicU64 = AtomicU64::new(0);
const ASK_USER_TOOL_NAMES: &[&str] = &["ask_user", "AskUserQuestion"];
const EXTENSION_DISPLAY_KEY: &str = "codoxear_display";
const EXTENSION_DISPLAY_TOOL_NAMES: &[&str] = &["codoxear_display", "codoxear.display"];
const FILE_LIST_IGNORED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".mypy_cache",
    ".svn",
    "__pycache__",
    "build",
    "dist",
    "node_modules",
    "venv",
    ".venv",
];
const TEXTUAL_EXTENSIONS: &[&str] = &[
    "bash", "c", "cc", "cfg", "conf", "cpp", "css", "csv", "diff", "go", "h", "hpp", "htm", "html",
    "ini", "java", "js", "json", "jsonl", "log", "md", "markdown", "mdown", "mkd", "patch", "py",
    "rs", "scss", "sh", "sql", "svg", "toml", "ts", "tsx", "txt", "xml", "yaml", "yml", "zsh",
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

#[derive(Debug)]
pub enum FileWriteError {
    BadRequest(String),
    NotFound(String),
    Forbidden(String),
    Conflict(Value),
    Internal(String),
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
    workspace_cwd: Option<String>,
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
    #[serde(default)]
    resume_session_id: Option<String>,
}

#[derive(Debug)]
struct BrokerState {
    busy: bool,
    _queue_len: usize,
    token: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct CreateSessionRequest {
    pub cwd: String,
    pub workspace_cwd: Option<String>,
    pub args: Vec<String>,
    pub agent_backend: String,
    pub resume_session_id: Option<String>,
    pub worktree_branch: Option<String>,
    pub model_provider: Option<String>,
    pub preferred_auth_method: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub create_in_tmux: bool,
}

#[derive(Debug, Clone)]
pub enum CreateSessionError {
    BadRequest {
        message: String,
        field: Option<String>,
    },
    Internal(String),
}

impl CreateSessionError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest {
            message: message.into(),
            field: None,
        }
    }

    pub fn bad_request_with_field(message: impl Into<String>, field: impl Into<String>) -> Self {
        Self::BadRequest {
            message: message.into(),
            field: Some(field.into()),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest { message, .. } => message,
            Self::Internal(message) => message,
        }
    }

    pub fn field(&self) -> Option<&str> {
        match self {
            Self::BadRequest { field, .. } => field.as_deref(),
            Self::Internal(_) => None,
        }
    }

    pub fn is_bad_request(&self) -> bool {
        matches!(self, Self::BadRequest { .. })
    }
}

pub fn default_app_dir() -> Result<PathBuf, String> {
    if let Ok(raw) = env::var("CODOXEAR_APP_DIR") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    let home = env::var("HOME")
        .map_err(|_| "HOME is not set; cannot resolve Codoxear app dir".to_string())?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("codoxear"))
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

pub fn load_resume_candidates_response(
    config: &RuntimeConfig,
    cwd: &Path,
    agent_backend: &str,
) -> Result<Value, String> {
    let info = describe_session_cwd(cwd);
    let mut payload = info.as_object().cloned().unwrap_or_default();
    let aliases = read_string_map(&config.app_dir.join("session_aliases.json"))?;
    let sessions = if info.get("exists").and_then(Value::as_bool) == Some(true) {
        list_resume_candidates_for_cwd(cwd, agent_backend, 12, &aliases)
    } else {
        Vec::new()
    };
    payload.insert("ok".to_string(), Value::Bool(true));
    payload.insert("sessions".to_string(), Value::Array(sessions));
    Ok(Value::Object(payload))
}

pub fn load_cwd_suggestions_response(
    config: &RuntimeConfig,
    raw: &str,
    limit: usize,
) -> Result<Value, String> {
    let recent_cwds = read_recent_cwds(&config.app_dir.join("recent_cwds.json"))?;
    list_directory_suggestions(raw, &recent_cwds, limit)
}

pub fn repo_root_dir() -> Result<PathBuf, String> {
    repo_root()
}

pub fn load_nova_shell_file(path: &str) -> Result<(Vec<u8>, String), String> {
    let repo_root = repo_root()?;
    let normalized = normalize_nova_shell_path(path)?;
    let dist_dir = repo_root.join("frontend").join("dist");
    let target = dist_dir.join(&normalized);
    let raw = fs::read(&target).map_err(|err| format!("read {}: {err}", target.display()))?;
    Ok((raw, content_type_for_static_path(&normalized).to_string()))
}

pub fn load_legacy_static_file(path: &str) -> Result<(Vec<u8>, String), String> {
    let repo_root = repo_root()?;
    let normalized = normalize_legacy_static_path(path)?;
    let static_dir = repo_root.join("codoxear").join("static");
    let target = static_dir.join(&normalized);
    let raw = read_legacy_static_bytes(&static_dir, &target)?;
    Ok((raw, content_type_for_static_path(&normalized).to_string()))
}

fn read_legacy_static_bytes(static_dir: &Path, path: &Path) -> Result<Vec<u8>, String> {
    let data = fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    if path.extension().and_then(|ext| ext.to_str()) != Some("html") {
        return Ok(data);
    }
    let mut text =
        String::from_utf8(data).map_err(|err| format!("decode {}: {err}", path.display()))?;
    text = text.replace(
        STATIC_ASSET_VERSION_PLACEHOLDER,
        &legacy_static_asset_version(static_dir)?,
    );
    text = text.replace(
        STATIC_ATTACH_MAX_BYTES_PLACEHOLDER,
        &ATTACH_UPLOAD_MAX_BYTES.to_string(),
    );
    Ok(text.into_bytes())
}

fn legacy_static_asset_version(static_dir: &Path) -> Result<String, String> {
    let base = fs::canonicalize(static_dir).unwrap_or_else(|_| static_dir.to_path_buf());
    let mut digest = Sha256::new();
    for rel in STATIC_ASSET_VERSION_FILES {
        let path = base.join(rel);
        if !path.starts_with(&base) {
            return Err(format!(
                "static asset escaped static dir: {}",
                path.display()
            ));
        }
        if !path.is_file() {
            continue;
        }
        digest.update(rel.as_bytes());
        digest.update([0]);
        digest.update(fs::read(&path).map_err(|err| format!("read {}: {err}", path.display()))?);
        digest.update([0]);
    }
    let hex = format!("{:x}", digest.finalize());
    Ok(hex.chars().take(12).collect())
}

pub fn load_codex_config_response() -> Result<Value, String> {
    let path = codex_config_path();
    let exists = path.exists();
    let text = if exists {
        fs::read_to_string(&path).map_err(|err| format!("read {}: {err}", path.display()))?
    } else {
        String::new()
    };
    Ok(json!({
        "ok": true,
        "path": path.display().to_string(),
        "exists": exists,
        "text": text,
    }))
}

pub fn save_codex_config_response(text: &str) -> Result<Value, String> {
    text.parse::<TomlValue>()
        .map_err(|err| format!("invalid TOML: {err}"))?;
    let path = codex_config_path();
    write_text_file_atomic(&path, text)?;
    load_codex_config_response()
}

pub fn schedule_local_service_restart_response() -> Result<Value, String> {
    let repo_root = repo_root()?;
    let script = repo_root.join("scripts").join("codoxear-local");
    if !script.exists() {
        return Err(format!("missing {}", script.display()));
    }
    let metadata =
        fs::metadata(&script).map_err(|err| format!("stat {}: {err}", script.display()))?;
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!("{} is not executable", script.display()));
    }
    let command = format!(
        "sleep {:.2}; exec {} restart",
        LOCAL_SERVICE_RESTART_DELAY_SECONDS,
        shell_quote(&script.display().to_string())
    );
    let mut child = Command::new("/bin/bash");
    child
        .arg("-lc")
        .arg(command)
        .current_dir(&repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    child.process_group(0);
    let proc = child
        .spawn()
        .map_err(|err| format!("spawn restart shell: {err}"))?;
    Ok(json!({
        "ok": true,
        "scheduled": true,
        "restart_pid": proc.id(),
        "script": script.display().to_string(),
        "server_pid": process::id(),
    }))
}

pub fn load_notification_subscriptions_response(config: &RuntimeConfig) -> Result<Value, String> {
    let records = read_notification_subscription_records(&push_subscriptions_path(config))?;
    let vapid_public_key = ensure_vapid_public_key(config)?;
    Ok(notification_subscriptions_snapshot_value(
        &records,
        &vapid_public_key,
    ))
}

pub fn upsert_notification_subscription_response(
    config: &RuntimeConfig,
    subscription: &Value,
    user_agent: &str,
    device_label: &str,
    device_class: &str,
) -> Result<Value, String> {
    let path = push_subscriptions_path(config);
    let mut records = read_notification_subscription_records(&path)?;
    let cleaned = clean_notification_subscription(subscription)?;
    let now_ts = epoch_now();
    let endpoint = cleaned
        .get("endpoint")
        .and_then(Value::as_str)
        .ok_or_else(|| "subscription endpoint required".to_string())?;
    let record_id = notification_subscription_id(endpoint);
    let previous = records.get(&record_id).cloned();
    let created_ts = previous
        .as_ref()
        .and_then(|value| value.get("created_ts"))
        .and_then(Value::as_f64)
        .unwrap_or(now_ts);
    let last_success_ts = previous
        .as_ref()
        .and_then(|value| value.get("last_success_ts"))
        .and_then(Value::as_f64);
    let last_failure_ts = previous
        .as_ref()
        .and_then(|value| value.get("last_failure_ts"))
        .and_then(Value::as_f64);
    let last_error = previous
        .as_ref()
        .and_then(|value| value.get("last_error"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let user_agent_clean = user_agent.trim().to_string();
    let device_class_clean = clean_notification_device_class(device_class, &user_agent_clean);
    let record = json!({
        "id": record_id,
        "subscription": cleaned,
        "notifications_enabled": true,
        "created_ts": created_ts,
        "updated_ts": now_ts,
        "last_success_ts": last_success_ts,
        "last_failure_ts": last_failure_ts,
        "last_error": last_error,
        "user_agent": user_agent_clean,
        "device_label": device_label.trim(),
        "device_class": device_class_clean,
    });
    records.insert(record_id, record);
    write_notification_subscription_records(&path, &records)?;
    load_notification_subscriptions_response(config)
}

pub fn toggle_notification_subscription_response(
    config: &RuntimeConfig,
    endpoint: &str,
    enabled: bool,
) -> Result<Value, String> {
    let endpoint_clean = endpoint.trim();
    if endpoint_clean.is_empty() {
        return Err("endpoint required".to_string());
    }
    let path = push_subscriptions_path(config);
    let mut records = read_notification_subscription_records(&path)?;
    let record_id = notification_subscription_id(endpoint_clean);
    let Some(record) = records.get_mut(&record_id) else {
        return Err("unknown subscription".to_string());
    };
    if record
        .get("subscription")
        .and_then(Value::as_object)
        .and_then(|item| item.get("endpoint"))
        .and_then(Value::as_str)
        != Some(endpoint_clean)
    {
        return Err("unknown subscription".to_string());
    }
    let object = record
        .as_object_mut()
        .ok_or_else(|| "unknown subscription".to_string())?;
    object.insert("notifications_enabled".to_string(), Value::Bool(enabled));
    object.insert("updated_ts".to_string(), json!(epoch_now()));
    write_notification_subscription_records(&path, &records)?;
    load_notification_subscriptions_response(config)
}

pub fn update_audio_listener_heartbeat_response(
    config: &RuntimeConfig,
    client_id: &str,
    enabled: bool,
) -> Result<Value, String> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        return Err("client_id required".to_string());
    }
    let now_ts = epoch_now();
    let path = voice_listeners_path(config);
    let mut listeners = read_voice_listener_records(&path, now_ts)?;
    if enabled {
        listeners.insert(client_id.to_string(), now_ts);
    } else {
        listeners.remove(client_id);
    }
    write_voice_listener_records(&path, &listeners)?;
    Ok(json!({
        "ok": true,
        "active_listener_count": listeners.len(),
    }))
}

pub fn load_voice_settings_response(config: &RuntimeConfig) -> Result<Value, String> {
    let settings = read_clean_voice_settings(&voice_settings_path(config))?;
    let audio = read_voice_runtime_audio_snapshot(&voice_runtime_path(config))?;
    let records = read_notification_subscription_records(&push_subscriptions_path(config))?;
    let vapid_public_key = ensure_vapid_public_key(config)?;
    let active_listener_count = current_voice_listener_count(
        config,
        audio
            .get("active_listener_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    )?;
    let (enabled_devices, total_devices) = notification_mobile_device_counts(&records);
    Ok(json!({
        "ok": true,
        "tts_enabled_for_narration": settings.get("tts_enabled_for_narration").and_then(Value::as_bool).unwrap_or(false),
        "tts_enabled_for_final_response": settings.get("tts_enabled_for_final_response").and_then(Value::as_bool).unwrap_or(false),
        "tts_base_url": settings.get("tts_base_url").and_then(Value::as_str).unwrap_or(DEFAULT_TTS_BASE_URL),
        "tts_api_key": settings.get("tts_api_key").and_then(Value::as_str).unwrap_or_default(),
        "summarization_model": settings.get("summarization_model").and_then(Value::as_str).unwrap_or(DEFAULT_SUMMARIZATION_MODEL),
        "tts_model": settings.get("tts_model").and_then(Value::as_str).unwrap_or(DEFAULT_TTS_MODEL),
        "audio": {
            "queue_depth": audio.get("queue_depth").and_then(Value::as_i64).unwrap_or(0),
            "active_listener_count": active_listener_count,
            "stream_url": "/api/audio/live.m3u8",
            "segment_count": audio.get("segment_count").and_then(Value::as_i64).unwrap_or(0),
            "last_error": audio.get("last_error").and_then(Value::as_str).unwrap_or_default(),
            "media_sequence": audio.get("media_sequence").and_then(Value::as_i64).unwrap_or(1),
        },
        "notifications": {
            "enabled_devices": enabled_devices,
            "total_devices": total_devices,
            "vapid_public_key": vapid_public_key,
        },
    }))
}

pub fn save_voice_settings_response(
    config: &RuntimeConfig,
    payload: &Value,
) -> Result<Value, String> {
    let settings = clean_voice_settings_value(payload)?;
    let text = serde_json::to_string_pretty(&settings)
        .map_err(|err| format!("serialize {}: {err}", voice_settings_path(config).display()))?;
    let text = format!("{text}\n");
    write_text_file_atomic(&voice_settings_path(config), &text)?;
    load_voice_settings_response(config)
}

pub fn load_notification_message_response(
    config: &RuntimeConfig,
    message_id: &str,
) -> Result<Value, String> {
    let message_id = message_id.trim();
    if message_id.is_empty() {
        return Err("message_id required".to_string());
    }
    let ledger = read_voice_delivery_ledger(&voice_delivery_ledger_path(config))?;
    let Some(row) = ledger.get(message_id).and_then(Value::as_object) else {
        return Err("unknown message".to_string());
    };
    Ok(json!({
        "ok": true,
        "message_id": message_id,
        "message_class": string_field(row, "message_class"),
        "summary_status": string_field(row, "summary_status"),
        "push_status": string_field(row, "push_status"),
        "notification_text": compact_text(string_field(row, "notification_text")),
    }))
}

pub fn load_notification_feed_response(
    config: &RuntimeConfig,
    since_ts: f64,
) -> Result<Value, String> {
    let ledger = read_voice_delivery_ledger(&voice_delivery_ledger_path(config))?;
    let mut items = ledger
        .iter()
        .filter_map(|(message_id, value)| {
            let row = value.as_object()?;
            if string_field(row, "message_class") != "final_response" {
                return None;
            }
            let updated_ts = row.get("updated_ts").and_then(Value::as_f64).unwrap_or(0.0);
            if updated_ts <= since_ts {
                return None;
            }
            let summary_status = string_field(row, "summary_status");
            if summary_status != "sent" && summary_status != "skipped" && summary_status != "error" {
                return None;
            }
            let notification_text = compact_text(string_field(row, "notification_text"));
            if notification_text.is_empty() {
                return None;
            }
            Some(json!({
                "message_id": message_id,
                "session_id": string_field(row, "session_id"),
                "session_display_name": default_session_display_name(string_field(row, "session_display_name")),
                "notification_text": notification_text,
                "updated_ts": updated_ts,
            }))
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        let a_ts = a.get("updated_ts").and_then(Value::as_f64).unwrap_or(0.0);
        let b_ts = b.get("updated_ts").and_then(Value::as_f64).unwrap_or(0.0);
        a_ts.partial_cmp(&b_ts)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.get("message_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .cmp(
                        b.get("message_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    )
            })
    });
    Ok(json!({ "ok": true, "items": items }))
}

pub fn load_audio_playlist_bytes(config: &RuntimeConfig) -> Result<Vec<u8>, String> {
    let path = audio_playlist_path(config);
    match fs::read(&path) {
        Ok(raw) => Ok(raw),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(b"#EXTM3U\n".to_vec()),
        Err(err) => Err(format!("read {}: {err}", path.display())),
    }
}

pub fn load_audio_segment_bytes(
    config: &RuntimeConfig,
    segment_name: &str,
) -> Result<Vec<u8>, String> {
    let name = Path::new(segment_name)
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "unknown audio segment".to_string())?;
    if name != segment_name || !name.ends_with(".ts") {
        return Err("unknown audio segment".to_string());
    }
    let segments_dir = audio_segments_dir(config);
    let base = segments_dir
        .canonicalize()
        .unwrap_or_else(|_| segments_dir.clone());
    let candidate = segments_dir.join(name);
    let resolved = candidate
        .canonicalize()
        .map_err(|_| "unknown audio segment".to_string())?;
    if !resolved.starts_with(&base) {
        return Err("unknown audio segment".to_string());
    }
    fs::read(&resolved).map_err(|_| "unknown audio segment".to_string())
}

fn describe_session_cwd(cwd: &Path) -> Value {
    let exists = cwd.exists();
    let repo_root = exists.then(|| git_repo_root(cwd)).flatten();
    let git_branch = if exists {
        current_git_branch(cwd).unwrap_or_default()
    } else {
        String::new()
    };
    json!({
        "cwd": cwd.display().to_string(),
        "exists": exists,
        "will_create": !exists,
        "git_repo": repo_root.is_some(),
        "git_root": repo_root.map(|path| path.display().to_string()).unwrap_or_default(),
        "git_branch": git_branch,
    })
}

fn normalize_nova_shell_path(path: &str) -> Result<String, String> {
    let trimmed = path.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return Err("empty path".to_string());
    }
    if trimmed
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err("invalid static path".to_string());
    }
    Ok(trimmed.to_string())
}

fn normalize_legacy_static_path(path: &str) -> Result<String, String> {
    normalize_nova_shell_path(path)
}

fn content_type_for_static_path(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "css" => "text/css; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "ico" => "image/x-icon",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "mjs" => "text/javascript; charset=utf-8",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "txt" => "text/plain; charset=utf-8",
        "webmanifest" => "application/manifest+json; charset=utf-8",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn list_resume_candidates_for_cwd(
    cwd: &Path,
    agent_backend: &str,
    limit: usize,
    aliases: &HashMap<String, String>,
) -> Vec<Value> {
    let cwd_text = cwd.display().to_string();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for log_path in iter_session_logs_for_backend(agent_backend) {
        let Some(mut row) = resume_candidate_from_log(&log_path, agent_backend) else {
            continue;
        };
        let session_id = row
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let row_cwd = row.get("cwd").and_then(Value::as_str).unwrap_or_default();
        if session_id.is_empty() || row_cwd != cwd_text || !seen.insert(session_id.to_string()) {
            continue;
        }
        let alias = aliases.get(session_id).cloned().unwrap_or_default();
        let preview = last_user_message_preview_from_log(&log_path, 256 * 1024);
        if let Some(object) = row.as_object_mut() {
            object.insert("alias".to_string(), Value::String(alias));
            object.insert("last_user_message".to_string(), Value::String(preview));
        }
        out.push(row);
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn resume_candidate_from_log(log_path: &Path, agent_backend: &str) -> Option<Value> {
    let payload = read_session_log_header(log_path, agent_backend)?;
    if agent_backend == "codex" && is_subagent_session_payload(&payload) {
        return None;
    }
    let session_id = payload.get("id").and_then(Value::as_str)?;
    let cwd = payload.get("cwd").and_then(Value::as_str)?;
    let mut out = serde_json::Map::new();
    out.insert(
        "session_id".to_string(),
        Value::String(session_id.to_string()),
    );
    out.insert("cwd".to_string(), Value::String(cwd.to_string()));
    out.insert(
        "log_path".to_string(),
        Value::String(log_path.display().to_string()),
    );
    out.insert("updated_ts".to_string(), json!(file_mtime(log_path)));
    out.insert(
        "timestamp".to_string(),
        payload.get("timestamp").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "git_branch".to_string(),
        payload
            .get("git")
            .and_then(Value::as_object)
            .and_then(|git| git.get("branch"))
            .and_then(Value::as_str)
            .map(|text| Value::String(text.to_string()))
            .unwrap_or_else(|| Value::String(String::new())),
    );
    out.insert(
        "agent_backend".to_string(),
        Value::String(agent_backend.to_string()),
    );
    Some(Value::Object(out))
}

fn last_user_message_preview_from_log(log_path: &Path, max_scan_bytes: usize) -> String {
    let size = match fs::metadata(log_path) {
        Ok(meta) => meta.len() as usize,
        Err(_) => return String::new(),
    };
    let start = size.saturating_sub(max_scan_bytes.max(1));
    let mut preview = String::new();
    for obj in read_jsonl_slice(log_path, start as u64, max_scan_bytes).unwrap_or_default() {
        let text = if obj.get("type").and_then(Value::as_str) == Some("message") {
            pi_user_text_value(&obj).unwrap_or_default()
        } else if obj.get("type").and_then(Value::as_str) == Some("response_item") {
            let Some(payload) = obj.get("payload").and_then(Value::as_object) else {
                continue;
            };
            if payload.get("type").and_then(Value::as_str) != Some("message")
                || payload.get("role").and_then(Value::as_str) != Some("user")
            {
                continue;
            }
            user_message_text(payload)
        } else {
            continue;
        };
        if text.trim().is_empty() || is_scaffold_user_text(&text) {
            continue;
        }
        preview = resume_preview_from_text(&text, 120);
    }
    if !preview.is_empty() {
        return preview;
    }
    for obj in read_jsonl_slice(log_path, 0, max_scan_bytes).unwrap_or_default() {
        let text = if obj.get("type").and_then(Value::as_str) == Some("message") {
            pi_user_text_value(&obj).unwrap_or_default()
        } else if obj.get("type").and_then(Value::as_str) == Some("response_item") {
            let Some(payload) = obj.get("payload").and_then(Value::as_object) else {
                continue;
            };
            if payload.get("type").and_then(Value::as_str) != Some("message")
                || payload.get("role").and_then(Value::as_str) != Some("user")
            {
                continue;
            }
            user_message_text(payload)
        } else {
            continue;
        };
        if text.trim().is_empty() || is_scaffold_user_text(&text) {
            continue;
        }
        return resume_preview_from_text(&text, 120);
    }
    String::new()
}

fn read_jsonl_slice(path: &Path, start: u64, max_bytes: usize) -> Result<Vec<Value>, String> {
    let mut file = fs::File::open(path).map_err(map_io_error)?;
    file.seek(SeekFrom::Start(start)).map_err(map_io_error)?;
    let mut raw = vec![0_u8; max_bytes];
    let read = file.read(&mut raw).map_err(map_io_error)?;
    raw.truncate(read);
    if start > 0 {
        if let Some(idx) = raw.iter().position(|byte| *byte == b'\n') {
            raw.drain(..=idx);
        } else {
            raw.clear();
        }
    }
    let mut out = Vec::new();
    for line in raw.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        out.push(value);
    }
    Ok(out)
}

fn user_message_text(payload: &Map<String, Value>) -> String {
    payload
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let kind = item.get("type").and_then(Value::as_str);
                    if matches!(
                        kind,
                        Some("input_text") | Some("output_text") | Some("text")
                    ) {
                        item.get("text")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|text| !text.is_empty())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn is_scaffold_user_text(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with("# AGENTS.md instructions") || trimmed.starts_with("<environment_context>")
}

fn resume_preview_from_text(text: &str, max_chars: usize) -> String {
    let compact = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let compact = compact.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    let mut head = compact
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    if let Some(cut) = head.rfind(' ') {
        if cut >= max_chars.saturating_mul(3) / 5 {
            head.truncate(cut);
        }
    }
    format!("{}...", head.trim_end())
}

fn list_directory_suggestions(
    raw: &str,
    recent_cwds: &[String],
    limit: usize,
) -> Result<Value, String> {
    let query = raw.trim().to_string();
    let cap = limit.clamp(1, 48);
    let mut suggestions = Vec::new();
    let mut seen = HashSet::new();

    let mut resolved_query = String::new();
    if query.is_empty() {
        for recent in recent_cwds {
            push_cwd_suggestion(
                &mut suggestions,
                &mut seen,
                cap,
                PathBuf::from(recent),
                "recent",
            );
        }
        if suggestions.len() < cap {
            if let Some(home) = home_dir().map(|path| fs::canonicalize(&path).unwrap_or(path)) {
                for child in
                    iter_matching_directories(&home, "", cap.saturating_sub(suggestions.len()))
                {
                    push_cwd_suggestion(&mut suggestions, &mut seen, cap, child, "directory");
                }
            }
        }
    } else {
        let expanded = expand_home(&query);
        let joined = if expanded.is_absolute() {
            expanded
        } else {
            env::current_dir().map_err(map_io_error)?.join(expanded)
        };
        let target = fs::canonicalize(&joined).unwrap_or(joined);
        resolved_query = target.display().to_string();
        let mut search_root = if query.ends_with('/') || target.is_dir() {
            Some(target.clone())
        } else {
            Some(
                target
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| target.clone()),
            )
        };
        let mut prefix = if query.ends_with('/') || target.is_dir() {
            String::new()
        } else {
            target
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string()
        };
        while let Some(root) = search_root.clone() {
            if root.exists() && root.is_dir() {
                for child in iter_matching_directories(&root, &prefix, cap) {
                    push_cwd_suggestion(&mut suggestions, &mut seen, cap, child, "directory");
                }
                break;
            }
            let next_prefix = root
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .map(|name| name.to_string());
            let parent = root.parent().map(Path::to_path_buf);
            match parent {
                Some(parent_path) if parent_path != root => {
                    if let Some(next) = next_prefix {
                        prefix = next;
                    }
                    search_root = Some(parent_path);
                }
                _ => {
                    search_root = None;
                }
            }
        }
    }

    if suggestions.len() < cap {
        for recent in recent_cwds {
            let Some(row) = cwd_suggestion_entry(Path::new(recent), "recent") else {
                continue;
            };
            let value = row.get("value").and_then(Value::as_str).unwrap_or_default();
            if !resolved_query.is_empty() && !value.starts_with(&resolved_query) {
                continue;
            }
            if !seen.insert(value.to_string()) {
                continue;
            }
            suggestions.push(row);
            if suggestions.len() >= cap {
                break;
            }
        }
    }

    Ok(json!({
        "ok": true,
        "query": query,
        "suggestions": suggestions,
    }))
}

fn push_cwd_suggestion(
    suggestions: &mut Vec<Value>,
    seen: &mut HashSet<String>,
    cap: usize,
    path: PathBuf,
    kind: &str,
) {
    if suggestions.len() >= cap {
        return;
    }
    let Some(row) = cwd_suggestion_entry(&path, kind) else {
        return;
    };
    let value = row
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !seen.insert(value) {
        return;
    }
    suggestions.push(row);
}

fn iter_matching_directories(base_dir: &Path, prefix: &str, limit: usize) -> Vec<PathBuf> {
    let wanted = prefix.trim().to_ascii_lowercase();
    let mut rows = Vec::new();
    let Ok(entries) = fs::read_dir(base_dir) else {
        return rows;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().trim().to_string();
        if !wanted.is_empty() && !name.to_ascii_lowercase().starts_with(&wanted) {
            continue;
        }
        rows.push(fs::canonicalize(entry.path()).unwrap_or_else(|_| entry.path()));
    }
    rows.sort_by(|left, right| {
        left.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .cmp(
                &right
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase(),
            )
            .then_with(|| left.display().to_string().cmp(&right.display().to_string()))
    });
    rows.truncate(limit);
    rows
}

fn cwd_suggestion_entry(path: &Path, kind: &str) -> Option<Value> {
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !resolved.is_dir() {
        return None;
    }
    let value = resolved.display().to_string();
    let label = resolved
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .unwrap_or_else(|| value.clone());
    Some(json!({
        "value": value,
        "label": label,
        "kind": kind,
    }))
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
    let events = selected
        .iter()
        .map(|record| record.event.clone())
        .collect::<Vec<_>>();
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
    let selected = records
        .into_iter()
        .filter(|record| record.start >= after)
        .map(|record| record.obj)
        .collect::<Vec<_>>();
    let extracted = extract_chat_events(&selected);
    Ok(ApiMessagesLiveResponse {
        thread_id: session.thread_id,
        log_path: Some(log_path.to_string()),
        live_cursor: Some(file_len(Path::new(log_path))?.to_string()),
        events: extracted.events,
        meta_delta: extracted.meta,
        turn_start: extracted.turn_start,
        turn_end: extracted.turn_end,
        turn_aborted: extracted.turn_aborted,
        diag: extracted.diag,
        busy: session.busy,
        queue_len: session.queue_len,
        token: session.token,
    })
}

pub fn load_preview_bootstrap(
    config: &RuntimeConfig,
) -> Result<(Vec<SessionSummary>, HashMap<String, SessionDetail>, String), String> {
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
            status: if session.busy {
                "running".into()
            } else {
                "idle".into()
            },
            workspace: session
                .workspace_cwd
                .clone()
                .unwrap_or_else(|| session.cwd.clone()),
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

pub fn load_diagnostics_response(
    config: &RuntimeConfig,
    session_id: &str,
) -> Result<ApiDiagnosticsResponse, String> {
    let session = find_session(config, session_id)?;
    let broker_state = read_live_broker_state(config, &session)?;
    let broker_busy = broker_state.busy;
    let busy = session
        .log_path
        .as_deref()
        .map(Path::new)
        .filter(|path| path.exists())
        .map(|_| session.busy)
        .unwrap_or(broker_busy);
    let now = epoch_now();
    let time_priority = priority_from_elapsed_seconds((now - session.updated_ts).max(0.0));
    let base_priority = clip01(time_priority + session.priority_offset);
    let blocked = session.dependency_session_id.is_some();
    let snoozed = session
        .snooze_until
        .map(|value| value > now)
        .unwrap_or(false);
    let final_priority = if blocked || snoozed {
        0.0
    } else {
        base_priority
    };
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
        busy,
        broker_busy,
        queue_len: session.queue_len,
        token: broker_state.token.or(session.token),
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

pub fn load_queue_response(
    config: &RuntimeConfig,
    session_id: &str,
) -> Result<ApiQueueResponse, String> {
    let _ = find_session(config, session_id)?;
    let queues = read_array_map(&config.app_dir.join("session_queues.json"))?;
    let items = normalized_queue_values(queues.get(session_id).map(Vec::as_slice).unwrap_or(&[]))
        .iter()
        .map(queue_item_from_value)
        .collect::<Vec<_>>();
    let queue = items
        .iter()
        .map(|item| item.text.clone())
        .collect::<Vec<_>>();
    Ok(ApiQueueResponse {
        ok: true,
        items,
        queue,
    })
}

pub fn enqueue_session_message(
    config: &RuntimeConfig,
    session_id: &str,
    text: &str,
) -> Result<Value, String> {
    if text.trim().is_empty() {
        return Err("text required".to_string());
    }
    let session = find_session(config, session_id)?;
    let queue_path = config.app_dir.join("session_queues.json");
    let mut queues = read_array_map(&queue_path)?;
    let mut items =
        normalized_queue_values(queues.get(session_id).map(Vec::as_slice).unwrap_or(&[]));
    let item = new_queue_item_value(text, None);
    let item_id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    items.push(item.clone());
    let queue_len = items.len();
    set_normalized_queue_values(&mut queues, session_id, items.clone());
    write_queue_map(&queue_path, &queues)?;
    if queue_len != 1 {
        return Ok(json!({"queued": true, "queue_len": queue_len, "item": item}));
    }
    let ready = match queue_remote_ready(config, &session) {
        Ok(ready) => ready,
        Err(_) => false,
    };
    if !ready {
        return Ok(json!({"queued": true, "queue_len": queue_len, "item": item}));
    }
    if let Some(first) = items.first_mut() {
        first["sending"] = Value::Bool(true);
    }
    set_normalized_queue_values(&mut queues, session_id, items.clone());
    write_queue_map(&queue_path, &queues)?;

    let send_result = broker_request_for_session(
        config,
        &session,
        &json!({"cmd": "send", "text": text}),
        Duration::from_secs_f64(3.0),
    )
    .and_then(|response| {
        if !response.is_object() || response.get("queue_len").and_then(Value::as_u64).is_none() {
            return Err("invalid broker send response".to_string());
        }
        Ok(response)
    });

    match send_result {
        Ok(response) => {
            let mut remaining =
                normalized_queue_values(queues.get(session_id).map(Vec::as_slice).unwrap_or(&[]));
            if let Some(index) = remaining
                .iter()
                .position(|entry| entry.get("id").and_then(Value::as_str) == Some(item_id.as_str()))
            {
                remaining.remove(index);
            }
            set_normalized_queue_values(&mut queues, session_id, remaining);
            write_queue_map(&queue_path, &queues)?;
            Ok(response)
        }
        Err(_) => {
            let mut reverted =
                normalized_queue_values(queues.get(session_id).map(Vec::as_slice).unwrap_or(&[]));
            if let Some(entry) = reverted
                .iter_mut()
                .find(|entry| entry.get("id").and_then(Value::as_str) == Some(item_id.as_str()))
            {
                entry.as_object_mut().map(|object| object.remove("sending"));
            }
            set_normalized_queue_values(&mut queues, session_id, reverted);
            write_queue_map(&queue_path, &queues)?;
            Ok(json!({"queued": true, "queue_len": queue_len, "item": item}))
        }
    }
}

pub fn delete_queue_item(
    config: &RuntimeConfig,
    session_id: &str,
    item_id: &str,
) -> Result<Value, String> {
    let item_id = item_id.trim();
    if item_id.is_empty() {
        return Err("id required".to_string());
    }
    let _ = find_session(config, session_id)?;
    let queue_path = config.app_dir.join("session_queues.json");
    let mut queues = read_array_map(&queue_path)?;
    let mut items =
        normalized_queue_values(queues.get(session_id).map(Vec::as_slice).unwrap_or(&[]));
    let Some(index) = items
        .iter()
        .position(|entry| entry.get("id").and_then(Value::as_str) == Some(item_id))
    else {
        return Err("item not found".to_string());
    };
    if items[index].get("sending").and_then(Value::as_bool) == Some(true) {
        return Err("item is already sending".to_string());
    }
    items.remove(index);
    let queue_len = items.len();
    set_normalized_queue_values(&mut queues, session_id, items);
    write_queue_map(&queue_path, &queues)?;
    Ok(json!({"ok": true, "queue_len": queue_len}))
}

pub fn update_queue_item(
    config: &RuntimeConfig,
    session_id: &str,
    item_id: &str,
    text: &str,
) -> Result<Value, String> {
    let item_id = item_id.trim();
    if item_id.is_empty() {
        return Err("id required".to_string());
    }
    if text.trim().is_empty() {
        return Err("text required".to_string());
    }
    let _ = find_session(config, session_id)?;
    let queue_path = config.app_dir.join("session_queues.json");
    let mut queues = read_array_map(&queue_path)?;
    let mut items =
        normalized_queue_values(queues.get(session_id).map(Vec::as_slice).unwrap_or(&[]));
    let Some(index) = items
        .iter()
        .position(|entry| entry.get("id").and_then(Value::as_str) == Some(item_id))
    else {
        return Err("item not found".to_string());
    };
    if items[index].get("sending").and_then(Value::as_bool) == Some(true) {
        return Err("item is already sending".to_string());
    }
    items[index]["text"] = Value::String(text.to_string());
    let item = queue_item_from_value(&items[index]);
    let queue_len = items.len();
    set_normalized_queue_values(&mut queues, session_id, items);
    write_queue_map(&queue_path, &queues)?;
    Ok(json!({"ok": true, "queue_len": queue_len, "item": item}))
}

pub fn move_queue_item(
    config: &RuntimeConfig,
    session_id: &str,
    item_id: &str,
    to_index: i64,
) -> Result<Value, String> {
    let item_id = item_id.trim();
    if item_id.is_empty() {
        return Err("id required".to_string());
    }
    let _ = find_session(config, session_id)?;
    let queue_path = config.app_dir.join("session_queues.json");
    let mut queues = read_array_map(&queue_path)?;
    let mut items =
        normalized_queue_values(queues.get(session_id).map(Vec::as_slice).unwrap_or(&[]));
    let Some(index) = items
        .iter()
        .position(|entry| entry.get("id").and_then(Value::as_str) == Some(item_id))
    else {
        return Err("item not found".to_string());
    };
    if items[index].get("sending").and_then(Value::as_bool) == Some(true) {
        return Err("item is already sending".to_string());
    }
    let min_index = if items
        .iter()
        .any(|entry| entry.get("sending").and_then(Value::as_bool) == Some(true))
    {
        1usize
    } else {
        0usize
    };
    if to_index < min_index as i64 || to_index >= items.len() as i64 {
        return Err("to_index out of range".to_string());
    }
    let item = items.remove(index);
    items.insert(to_index as usize, item);
    let queue_len = items.len();
    set_normalized_queue_values(&mut queues, session_id, items);
    write_queue_map(&queue_path, &queues)?;
    Ok(json!({"ok": true, "queue_len": queue_len}))
}

pub fn rust_queue_sweep_enabled() -> bool {
    env::var("CODOXEAR_ENABLE_QUEUE_SWEEP")
        .ok()
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

pub fn rust_harness_sweep_enabled() -> bool {
    env::var("CODOXEAR_ENABLE_HARNESS_SWEEP")
        .ok()
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

pub fn rust_voice_scan_enabled() -> bool {
    env::var("CODOXEAR_ENABLE_VOICE_SCAN")
        .ok()
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

pub fn spawn_harness_sweep_worker(config: RuntimeConfig) {
    tokio::spawn(async move {
        let mut last_injected: HashMap<String, f64> = HashMap::new();
        let mut last_injected_scope: HashMap<String, f64> = HashMap::new();
        let mut interval = time::interval(harness_sweep_interval());
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let sweep_config = config.clone();
            let prior_last_injected = std::mem::take(&mut last_injected);
            let prior_last_injected_scope = std::mem::take(&mut last_injected_scope);
            match task::spawn_blocking(move || {
                let mut next_last_injected = prior_last_injected;
                let mut next_last_injected_scope = prior_last_injected_scope;
                let result = run_harness_sweep_once(
                    &sweep_config,
                    &mut next_last_injected,
                    &mut next_last_injected_scope,
                    epoch_now(),
                );
                (next_last_injected, next_last_injected_scope, result)
            })
            .await
            {
                Ok((next_last_injected, next_last_injected_scope, Ok(_did_send))) => {
                    last_injected = next_last_injected;
                    last_injected_scope = next_last_injected_scope;
                }
                Ok((next_last_injected, next_last_injected_scope, Err(err))) => {
                    last_injected = next_last_injected;
                    last_injected_scope = next_last_injected_scope;
                    tracing::warn!("rust harness sweep failed: {err}");
                }
                Err(err) => {
                    tracing::warn!("rust harness sweep task join failed: {err}");
                }
            }
        }
    });
}

pub fn spawn_queue_sweep_worker(config: RuntimeConfig) {
    tokio::spawn(async move {
        let mut idle_since: HashMap<String, f64> = HashMap::new();
        let mut interval = time::interval(queue_sweep_interval());
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let sweep_config = config.clone();
            let prior_idle_since = std::mem::take(&mut idle_since);
            match task::spawn_blocking(move || {
                let mut next_idle_since = prior_idle_since;
                let result = run_queue_sweep_once(&sweep_config, &mut next_idle_since, epoch_now());
                (next_idle_since, result)
            })
            .await
            {
                Ok((next_idle_since, Ok(_did_send))) => {
                    idle_since = next_idle_since;
                }
                Ok((next_idle_since, Err(err))) => {
                    idle_since = next_idle_since;
                    tracing::warn!("rust queue sweep failed: {err}");
                }
                Err(err) => {
                    tracing::warn!("rust queue sweep task join failed: {err}");
                }
            }
        }
    });
}

pub fn spawn_voice_scan_worker(config: RuntimeConfig) {
    tokio::spawn(async move {
        let mut offsets: HashMap<String, u64> = HashMap::new();
        let mut interval = time::interval(voice_scan_interval());
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let scan_config = config.clone();
            let prior_offsets = std::mem::take(&mut offsets);
            match task::spawn_blocking(move || {
                let mut next_offsets = prior_offsets;
                let result = run_voice_scan_once(&scan_config, &mut next_offsets);
                (next_offsets, result)
            })
            .await
            {
                Ok((next_offsets, Ok(_delivered))) => {
                    offsets = next_offsets;
                }
                Ok((next_offsets, Err(err))) => {
                    offsets = next_offsets;
                    tracing::warn!("rust voice scan failed: {err}");
                }
                Err(err) => {
                    tracing::warn!("rust voice scan task join failed: {err}");
                }
            }
        }
    });
}

fn run_queue_sweep_once(
    config: &RuntimeConfig,
    idle_since: &mut HashMap<String, f64>,
    now_ts: f64,
) -> Result<bool, String> {
    let queue_path = config.app_dir.join("session_queues.json");
    let mut queues = read_array_map(&queue_path)?;
    let sessions = load_sessions_response(config)?;
    let session_by_id = sessions
        .sessions
        .into_iter()
        .map(|session| (session.session_id.clone(), session))
        .collect::<HashMap<_, _>>();

    let mut dirty = false;
    let stale_ids = queues
        .keys()
        .filter(|session_id| !session_by_id.contains_key(*session_id))
        .cloned()
        .collect::<Vec<_>>();
    for session_id in stale_ids {
        queues.remove(&session_id);
        idle_since.remove(&session_id);
        dirty = true;
    }

    let mut session_ids = queues.keys().cloned().collect::<Vec<_>>();
    session_ids.sort();

    for session_id in session_ids {
        let Some(session) = session_by_id.get(&session_id) else {
            continue;
        };
        let mut items =
            normalized_queue_values(queues.get(&session_id).map(Vec::as_slice).unwrap_or(&[]));
        if items.is_empty() {
            queues.remove(&session_id);
            idle_since.remove(&session_id);
            dirty = true;
            continue;
        }
        if items
            .first()
            .and_then(|value| value.get("sending"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            idle_since.remove(&session_id);
            continue;
        }
        let ready = match queue_remote_ready(config, session) {
            Ok(ready) => ready,
            Err(_) => {
                idle_since.remove(&session_id);
                continue;
            }
        };
        if !ready {
            idle_since.remove(&session_id);
            continue;
        }

        let start_ts = idle_since.entry(session_id.clone()).or_insert(now_ts);
        if (now_ts - *start_ts) < queue_idle_grace_seconds() {
            continue;
        }
        idle_since.remove(&session_id);

        let item_id = items
            .first()
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let text = items
            .first()
            .and_then(|value| value.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if let Some(first) = items.first_mut() {
            first["sending"] = Value::Bool(true);
        }
        set_normalized_queue_values(&mut queues, &session_id, items.clone());
        write_queue_map(&queue_path, &queues)?;

        let send_result = broker_request_for_session(
            config,
            session,
            &json!({"cmd": "send", "text": text}),
            Duration::from_secs_f64(3.0),
        )
        .and_then(|response| {
            if !response.is_object() || response.get("queue_len").and_then(Value::as_u64).is_none()
            {
                return Err("invalid broker send response".to_string());
            }
            Ok(response)
        });

        match send_result {
            Ok(_) => {
                let mut remaining = normalized_queue_values(
                    queues.get(&session_id).map(Vec::as_slice).unwrap_or(&[]),
                );
                if let Some(index) = remaining.iter().position(|entry| {
                    entry.get("id").and_then(Value::as_str) == Some(item_id.as_str())
                }) {
                    remaining.remove(index);
                }
                set_normalized_queue_values(&mut queues, &session_id, remaining);
                write_queue_map(&queue_path, &queues)?;
                return Ok(true);
            }
            Err(_) => {
                let mut reverted = normalized_queue_values(
                    queues.get(&session_id).map(Vec::as_slice).unwrap_or(&[]),
                );
                if let Some(entry) = reverted
                    .iter_mut()
                    .find(|entry| entry.get("id").and_then(Value::as_str) == Some(item_id.as_str()))
                {
                    if let Some(object) = entry.as_object_mut() {
                        object.remove("sending");
                    }
                }
                set_normalized_queue_values(&mut queues, &session_id, reverted);
                write_queue_map(&queue_path, &queues)?;
            }
        }
    }

    if dirty {
        write_queue_map(&queue_path, &queues)?;
    }
    Ok(false)
}

fn run_voice_scan_once(
    config: &RuntimeConfig,
    offsets: &mut HashMap<String, u64>,
) -> Result<usize, String> {
    let sessions = load_sessions_response(config)?;
    let ledger_path = voice_delivery_ledger_path(config);
    let mut ledger = read_voice_delivery_ledger(&ledger_path)?;
    let voice_settings = read_clean_voice_settings(&voice_settings_path(config))?;
    let subscriptions = read_notification_subscription_records(&push_subscriptions_path(config))?;
    let mut seen_keys = HashSet::new();
    let mut delivered = 0usize;
    let mut ledger_dirty = false;

    for session in sessions.sessions {
        let Some(log_path_raw) = session.log_path.as_deref() else {
            continue;
        };
        let log_path = Path::new(log_path_raw);
        if !log_path.exists() {
            continue;
        }
        let key = format!("{}|{}", session.session_id, log_path.display());
        seen_keys.insert(key.clone());
        let size = file_len(log_path).unwrap_or(0);
        let offset = offsets.entry(key.clone()).or_insert(size);
        if size < *offset {
            *offset = 0;
        }
        let mut loops = 0usize;
        while *offset < size && loops < 16 {
            let (objs, new_offset) = read_jsonl_values_from_offset(log_path, *offset, 256 * 1024)?;
            if new_offset <= *offset {
                break;
            }
            if session_resume_id(config, &session.session_id)?.is_none() {
                let messages = extract_voice_delivery_messages(&objs);
                for message in messages {
                    if ledger.contains_key(&message.message_id) {
                        continue;
                    }
                    if maybe_record_voice_delivery_locally(
                        &mut ledger,
                        &session,
                        &message,
                        &voice_settings,
                        &subscriptions,
                    ) {
                        ledger_dirty = true;
                        delivered += 1;
                    } else if write_voice_inbox_message(config, &session, &message)? {
                        delivered += 1;
                    }
                }
            }
            *offset = new_offset;
            loops += 1;
        }
    }

    offsets.retain(|key, _| seen_keys.contains(key));
    if ledger_dirty {
        write_voice_delivery_ledger(&ledger_path, &ledger)?;
    }
    Ok(delivered)
}

#[derive(Clone, Debug)]
struct VoiceDeliveryMessage {
    message_id: String,
    message_class: String,
    text: String,
    ts: Option<f64>,
}

fn extract_voice_delivery_messages(objs: &[Value]) -> Vec<VoiceDeliveryMessage> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut last_text_key: Option<(String, String)> = None;

    for obj in objs {
        let (message_class, text) = match obj.get("type").and_then(Value::as_str) {
            Some("message") => {
                let Some(value) = pi_assistant_text_value(obj) else {
                    continue;
                };
                let message_class = if pi_assistant_is_final_turn_end(obj) {
                    "final_response"
                } else {
                    "narration"
                };
                (message_class, value)
            }
            Some("event_msg") => {
                let Some(payload) = obj.get("payload").and_then(Value::as_object) else {
                    continue;
                };
                if payload.get("type").and_then(Value::as_str) != Some("agent_message") {
                    continue;
                }
                let Some(message) = payload.get("message").and_then(Value::as_str) else {
                    continue;
                };
                if message.trim().is_empty() {
                    continue;
                }
                let message_class =
                    if payload.get("phase").and_then(Value::as_str) == Some("final_answer") {
                        "final_response"
                    } else {
                        "narration"
                    };
                (message_class, message.to_string())
            }
            Some("response_item") => {
                let Some(payload) = obj.get("payload").and_then(Value::as_object) else {
                    continue;
                };
                if payload.get("type").and_then(Value::as_str) != Some("message")
                    || payload.get("role").and_then(Value::as_str) != Some("assistant")
                {
                    continue;
                }
                let Some(value) = payload.get("content").and_then(output_text) else {
                    continue;
                };
                if value.trim().is_empty() {
                    continue;
                }
                let message_class = if payload.get("phase").and_then(Value::as_str)
                    == Some("final_answer")
                    || payload.get("end_turn").and_then(Value::as_bool) == Some(true)
                {
                    "final_response"
                } else {
                    "narration"
                };
                (message_class, value)
            }
            _ => continue,
        };
        let text = strip_oai_mem_citation_tail(&text);
        if text.trim().is_empty() {
            continue;
        }
        let normalized_text = compact_text(&text);
        let text_key = (message_class.to_string(), normalized_text);
        if last_text_key.as_ref() == Some(&text_key) {
            continue;
        }
        let ts = event_ts(obj);
        let message_id = voice_text_message_id(message_class, &text, ts);
        if !seen.insert(message_id.clone()) {
            continue;
        }
        last_text_key = Some(text_key);
        out.push(VoiceDeliveryMessage {
            message_id,
            message_class: message_class.to_string(),
            text,
            ts,
        });
    }
    out
}

fn read_jsonl_values_from_offset(
    path: &Path,
    offset: u64,
    max_bytes: usize,
) -> Result<(Vec<Value>, u64), String> {
    let file = fs::File::open(path).map_err(|err| format!("open {}: {err}", path.display()))?;
    let mut reader = BufReader::new(file);
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|err| format!("seek {}: {err}", path.display()))?;
    let mut values = Vec::new();
    let mut cursor = offset;
    let mut consumed = 0usize;
    loop {
        if consumed >= max_bytes {
            break;
        }
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .map_err(|err| format!("read {}: {err}", path.display()))?;
        if n == 0 {
            break;
        }
        consumed += n;
        let start = cursor;
        cursor += n as u64;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value = serde_json::from_str(trimmed)
            .map_err(|err| format!("parse {} at byte {start}: {err}", path.display()))?;
        values.push(value);
    }
    Ok((values, cursor))
}

fn voice_text_message_id(message_class: &str, text: &str, ts: Option<f64>) -> String {
    let normalized_text = compact_text(text);
    let class_json = serde_json::to_string(message_class).unwrap_or_else(|_| "\"\"".to_string());
    let text_json = serde_json::to_string(&normalized_text).unwrap_or_else(|_| "\"\"".to_string());
    let ts_json = ts
        .filter(|value| value.is_finite())
        .map(|value| python_round_to_i64(value * 1000.0).to_string())
        .unwrap_or_else(|| "null".to_string());
    let payload =
        format!("{{\"class\": {class_json}, \"text\": {text_json}, \"ts_ms\": {ts_json}}}");
    let digest = Sha256::digest(payload.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn python_round_to_i64(value: f64) -> i64 {
    let floor = value.floor();
    let frac = value - floor;
    if frac < 0.5 {
        floor as i64
    } else if frac > 0.5 {
        floor as i64 + 1
    } else {
        let base = floor as i64;
        if base % 2 == 0 {
            base
        } else {
            base + 1
        }
    }
}

fn strip_oai_mem_citation_tail(text: &str) -> String {
    let Some(start) = text.rfind("<oai-mem-citation>") else {
        return text.to_string();
    };
    let tail = &text[start..];
    let Some(end_rel) = tail.rfind("</oai-mem-citation>") else {
        return text.to_string();
    };
    let after = &tail[end_rel + "</oai-mem-citation>".len()..];
    if after.trim().is_empty() {
        text[..start].to_string()
    } else {
        text.to_string()
    }
}

fn session_resume_id(config: &RuntimeConfig, session_id: &str) -> Result<Option<String>, String> {
    let meta_path = config
        .app_dir
        .join("socks")
        .join(format!("{session_id}.json"));
    let raw = match fs::read_to_string(&meta_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("read {}: {err}", meta_path.display())),
    };
    let meta: SessionMeta = serde_json::from_str(&raw)
        .map_err(|err| format!("parse {}: {err}", meta_path.display()))?;
    Ok(clean_optional(meta.resume_session_id))
}

fn write_voice_inbox_message(
    config: &RuntimeConfig,
    session: &ApiSessionSummary,
    message: &VoiceDeliveryMessage,
) -> Result<bool, String> {
    let session_slug = safe_filename(&session.session_id, "session");
    let short_id = message.message_id.chars().take(16).collect::<String>();
    let path = config
        .app_dir
        .join("voice_inbox")
        .join(format!("{session_slug}-{short_id}.json"));
    if path.exists() {
        return Ok(false);
    }
    let payload = json!({
        "message_id": message.message_id,
        "session_id": session.session_id,
        "session_display_name": default_session_display_name(session.alias.clone()),
        "message_class": message.message_class,
        "text": message.text,
        "ts": message.ts,
    });
    write_json_value(&path, &payload)?;
    Ok(true)
}

fn maybe_record_voice_delivery_locally(
    ledger: &mut HashMap<String, Value>,
    session: &ApiSessionSummary,
    message: &VoiceDeliveryMessage,
    settings: &Value,
    subscriptions: &HashMap<String, Value>,
) -> bool {
    let session_display_name = default_session_display_name(session.alias.clone());
    let source_text = compact_text(&message.text);
    if source_text.is_empty() {
        return false;
    }
    let now_ts = epoch_now();
    let narration_enabled = settings
        .get("tts_enabled_for_narration")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let final_tts_enabled = settings
        .get("tts_enabled_for_final_response")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let api_key_present = settings
        .get("tts_api_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some();
    let mobile_push_enabled = subscriptions.values().any(|record| {
        record
            .get("notifications_enabled")
            .map(json_truthy)
            .unwrap_or(false)
            && record.get("device_class").and_then(Value::as_str) == Some("mobile")
    });

    let mut row = json!({
        "message_id": message.message_id,
        "session_id": session.session_id,
        "session_display_name": session_display_name,
        "message_class": message.message_class,
        "preview_text": clip_text(&message.text, 160),
        "notification_text": "",
        "summary_text": "",
        "summary_status": "skipped",
        "narrated_status": "skipped",
        "push_status": "skipped",
        "voice": "",
        "created_ts": now_ts,
        "updated_ts": now_ts,
        "last_error": "",
    });

    match message.message_class.as_str() {
        "narration" if !narration_enabled => {
            ledger.insert(message.message_id.clone(), row);
            true
        }
        "final_response" if !api_key_present && !mobile_push_enabled => {
            row["notification_text"] = json!(clip_text(&source_text, 120));
            if final_tts_enabled {
                row["narrated_status"] = json!("error");
                row["last_error"] = json!("tts_api_key is required");
            }
            ledger.insert(message.message_id.clone(), row);
            true
        }
        _ => false,
    }
}

fn write_voice_delivery_ledger(path: &Path, ledger: &HashMap<String, Value>) -> Result<(), String> {
    let mut ids = ledger.keys().cloned().collect::<Vec<_>>();
    ids.sort();
    let mut object = serde_json::Map::new();
    for id in ids {
        let Some(value) = ledger.get(&id) else {
            continue;
        };
        object.insert(id, value.clone());
    }
    write_json_value(path, &Value::Object(object))
}

fn run_harness_sweep_once(
    config: &RuntimeConfig,
    last_injected: &mut HashMap<String, f64>,
    last_injected_scope: &mut HashMap<String, f64>,
    now_ts: f64,
) -> Result<bool, String> {
    let harness_path = config.app_dir.join("harness.json");
    let queue_path = config.app_dir.join("session_queues.json");
    let mut harness = read_object_map(&harness_path)?;
    let queues = read_array_map(&queue_path)?;
    let sessions = load_sessions_response(config)?;
    let session_by_id = sessions
        .sessions
        .into_iter()
        .map(|session| (session.session_id.clone(), session))
        .collect::<HashMap<_, _>>();

    let mut session_ids = session_by_id.keys().cloned().collect::<Vec<_>>();
    session_ids.sort();
    let mut did_send = false;

    for session_id in session_ids {
        let Some(session) = session_by_id.get(&session_id) else {
            continue;
        };
        let Some(entry) = harness.get(&session_id).cloned() else {
            continue;
        };
        if !object_bool(Some(&entry), "enabled").unwrap_or(false) {
            continue;
        }
        let cooldown_minutes = entry
            .get("cooldown_minutes")
            .map(clean_harness_cooldown_minutes_value)
            .transpose()?
            .unwrap_or(HARNESS_DEFAULT_IDLE_MINUTES as i64);
        let cooldown_seconds = (cooldown_minutes as f64) * 60.0;
        let remaining_injections = entry
            .get("remaining_injections")
            .map(|value| clean_harness_remaining_injections_value(value, true))
            .transpose()?
            .unwrap_or(HARNESS_DEFAULT_MAX_INJECTIONS);
        if remaining_injections <= 0 {
            let mut updated = entry.as_object().cloned().unwrap_or_default();
            updated.insert("enabled".to_string(), Value::Bool(false));
            updated.insert("remaining_injections".to_string(), json!(0));
            harness.insert(session_id.clone(), Value::Object(updated));
            write_object_map(&harness_path, &harness)?;
            last_injected.remove(&session_id);
            continue;
        }

        let request = object_string(Some(&entry), "request");
        let prompt = render_harness_prompt(request.as_deref());
        let Some(log_path) = session.log_path.as_deref().map(PathBuf::from) else {
            continue;
        };
        if !log_path.exists() {
            continue;
        }
        let scope_key = session
            .thread_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|thread_id| format!("thread:{thread_id}"))
            .unwrap_or_else(|| format!("log:{}", log_path.display()));
        let session_last = last_injected.get(&session_id).copied().unwrap_or(0.0);
        let scope_last = last_injected_scope.get(&scope_key).copied().unwrap_or(0.0);
        if (session_last > 0.0 && (now_ts - session_last) < cooldown_seconds)
            || (scope_last > 0.0 && (now_ts - scope_last) < cooldown_seconds)
        {
            continue;
        }

        let broker_state = match read_live_broker_state(config, session) {
            Ok(state) => state,
            Err(err) => {
                tracing::warn!("rust harness sweep skipped session {}: {}", session_id, err);
                continue;
            }
        };
        let persisted_queue_len =
            normalized_queue_values(queues.get(&session_id).map(Vec::as_slice).unwrap_or(&[]))
                .len();
        if broker_state.busy || broker_state._queue_len > 0 || persisted_queue_len > 0 {
            continue;
        }

        let Some((role, ts)) = last_chat_role_ts_from_log(&log_path) else {
            continue;
        };
        if role != "assistant" || (now_ts - ts) < cooldown_seconds {
            continue;
        }
        let scope_last = last_injected_scope.get(&scope_key).copied().unwrap_or(0.0);
        if scope_last > 0.0 && (now_ts - scope_last) < cooldown_seconds {
            continue;
        }

        let send_result = broker_request_for_session(
            config,
            session,
            &json!({"cmd": "send", "text": prompt}),
            Duration::from_secs_f64(3.0),
        )
        .and_then(|response| {
            if !response.is_object() || response.get("queue_len").and_then(Value::as_u64).is_none()
            {
                return Err("invalid broker send response".to_string());
            }
            Ok(response)
        });
        if let Err(err) = send_result {
            tracing::warn!("rust harness sweep skipped session {}: {}", session_id, err);
            continue;
        }

        did_send = true;
        last_injected.insert(session_id.clone(), now_ts);
        last_injected_scope.insert(scope_key, now_ts);

        let mut updated = entry.as_object().cloned().unwrap_or_default();
        let next_remaining = std::cmp::max(0_i64, remaining_injections - 1);
        updated.insert("remaining_injections".to_string(), json!(next_remaining));
        if next_remaining <= 0 {
            updated.insert("enabled".to_string(), Value::Bool(false));
            last_injected.remove(&session_id);
        }
        harness.insert(session_id.clone(), Value::Object(updated));
        write_object_map(&harness_path, &harness)?;
    }

    Ok(did_send)
}

pub fn load_harness_response(
    config: &RuntimeConfig,
    session_id: &str,
) -> Result<ApiHarnessResponse, String> {
    let _ = find_session(config, session_id)?;
    let harness = read_object_map(&config.app_dir.join("harness.json"))?;
    let entry = harness.get(session_id);
    Ok(ApiHarnessResponse {
        ok: true,
        enabled: object_bool(entry, "enabled").unwrap_or(false),
        request: object_string(entry, "request").unwrap_or_default(),
        cooldown_minutes: object_number(entry, "cooldown_minutes")
            .unwrap_or(HARNESS_DEFAULT_IDLE_MINUTES),
        remaining_injections: object_i64(entry, "remaining_injections")
            .unwrap_or(HARNESS_DEFAULT_MAX_INJECTIONS),
    })
}

pub fn set_harness_config(
    config: &RuntimeConfig,
    session_id: &str,
    enabled: Option<&Value>,
    request: Option<&Value>,
    cooldown_minutes: Option<&Value>,
    remaining_injections: Option<&Value>,
    text_field_present: bool,
) -> Result<ApiHarnessResponse, String> {
    let _ = find_session(config, session_id)?;
    if text_field_present {
        return Err("unknown field: text (use request)".to_string());
    }

    let harness_path = config.app_dir.join("harness.json");
    let mut harness = read_object_map(&harness_path)?;
    let mut entry = harness
        .get(session_id)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    if let Some(value) = enabled.filter(|value| !value.is_null()) {
        entry.insert("enabled".to_string(), Value::Bool(json_truthy(value)));
    }
    if let Some(value) = request.filter(|value| !value.is_null()) {
        let Some(text) = value.as_str() else {
            return Err("request must be a string".to_string());
        };
        entry.insert("request".to_string(), Value::String(text.to_string()));
    }
    if let Some(value) = cooldown_minutes.filter(|value| !value.is_null()) {
        let cooldown = clean_harness_cooldown_minutes_value(value)?;
        entry.insert("cooldown_minutes".to_string(), json!(cooldown));
    }
    if let Some(value) = remaining_injections.filter(|value| !value.is_null()) {
        let remaining = clean_harness_remaining_injections_value(value, true)?;
        entry.insert("remaining_injections".to_string(), json!(remaining));
    }

    let enabled_clean = entry.get("enabled").map(json_truthy).unwrap_or(false);
    let request_clean = entry
        .get("request")
        .map(|value| {
            value
                .as_str()
                .map(ToString::to_string)
                .ok_or_else(|| "request must be a string".to_string())
        })
        .transpose()?
        .unwrap_or_default();
    let cooldown_clean = entry
        .get("cooldown_minutes")
        .map(clean_harness_cooldown_minutes_value)
        .transpose()?
        .unwrap_or(HARNESS_DEFAULT_IDLE_MINUTES as i64);
    let remaining_clean = entry
        .get("remaining_injections")
        .map(|value| clean_harness_remaining_injections_value(value, true))
        .transpose()?
        .unwrap_or(HARNESS_DEFAULT_MAX_INJECTIONS);

    let mut cleaned = serde_json::Map::new();
    cleaned.insert("enabled".to_string(), Value::Bool(enabled_clean));
    cleaned.insert("request".to_string(), Value::String(request_clean.clone()));
    cleaned.insert("cooldown_minutes".to_string(), json!(cooldown_clean));
    cleaned.insert("remaining_injections".to_string(), json!(remaining_clean));
    harness.insert(session_id.to_string(), Value::Object(cleaned));
    write_object_map(&harness_path, &harness)?;

    Ok(ApiHarnessResponse {
        ok: true,
        enabled: enabled_clean,
        request: request_clean,
        cooldown_minutes: cooldown_clean as f64,
        remaining_injections: remaining_clean,
    })
}

pub fn load_file_search_response(
    config: &RuntimeConfig,
    session_id: &str,
    raw_query: &str,
    limit: usize,
) -> Result<ApiFileSearchResponse, String> {
    let session = find_session(config, session_id)?;
    let root = resolve_search_root(&session.cwd)?;
    if !root.exists() {
        return Err("session cwd not found".to_string());
    }
    if !root.is_dir() {
        return Err("session cwd is not a directory".to_string());
    }
    let query = raw_query.trim().to_string();
    if query.is_empty() {
        return Err("query required".to_string());
    }
    let clamped_limit = limit.max(1).min(default_file_search_limit());
    let result = if git_repo_root(&root).is_some() {
        search_git_relative_files(&root, &query, clamped_limit)?
    } else {
        search_walk_relative_files(&root, &query, clamped_limit)?
    };
    Ok(ApiFileSearchResponse {
        ok: true,
        cwd: root.display().to_string(),
        query,
        mode: result.mode,
        matches: result.matches,
        scanned: result.scanned,
        truncated: result.truncated,
    })
}

pub fn load_changed_files_response(
    config: &RuntimeConfig,
    session_id: &str,
) -> Result<ApiChangedFilesResponse, String> {
    let session = find_session(config, session_id)?;
    let cwd = resolve_search_root(&session.cwd)?;
    ensure_git_repo(&cwd)?;

    let unstaged = normalize_git_path_list(
        &run_git_capture(
            &cwd,
            &["diff", "--name-only"],
            default_git_diff_timeout(),
            default_git_diff_max_bytes(),
        )?,
        default_git_changed_files_max(),
    );
    let staged = normalize_git_path_list(
        &run_git_capture(
            &cwd,
            &["diff", "--name-only", "--cached"],
            default_git_diff_timeout(),
            default_git_diff_max_bytes(),
        )?,
        default_git_changed_files_max(),
    );
    let unstaged_numstat = run_git_capture(
        &cwd,
        &["diff", "--numstat"],
        default_git_diff_timeout(),
        default_git_diff_max_bytes().max(128 * 1024),
    )?;
    let staged_numstat = run_git_capture(
        &cwd,
        &["diff", "--numstat", "--cached"],
        default_git_diff_timeout(),
        default_git_diff_max_bytes().max(128 * 1024),
    )?;

    let mut merged = Vec::new();
    let mut seen = HashSet::new();
    for path in unstaged.iter().chain(staged.iter()) {
        if seen.insert(path.clone()) {
            merged.push(path.clone());
        }
    }

    let mut stats = parse_git_numstat(&unstaged_numstat);
    for (path, values) in parse_git_numstat(&staged_numstat) {
        if let Some(prev) = stats.get_mut(&path) {
            prev.0 = match (prev.0, values.0) {
                (Some(left), Some(right)) => Some(left + right),
                _ => None,
            };
            prev.1 = match (prev.1, values.1) {
                (Some(left), Some(right)) => Some(left + right),
                _ => None,
            };
        } else {
            stats.insert(path, values);
        }
    }

    let entries = merged
        .iter()
        .map(|path| {
            let (additions, deletions) = stats.get(path).copied().unwrap_or((None, None));
            ApiChangedFileEntry {
                path: path.clone(),
                additions,
                deletions,
                changed: true,
            }
        })
        .collect::<Vec<_>>();

    Ok(ApiChangedFilesResponse {
        ok: true,
        cwd: cwd.display().to_string(),
        files: merged,
        entries,
        unstaged,
        staged,
    })
}

pub fn load_git_diff_response(
    config: &RuntimeConfig,
    session_id: &str,
    raw_path: &str,
    staged: bool,
) -> Result<ApiGitDiffResponse, String> {
    let session = find_session(config, session_id)?;
    let cwd = resolve_search_root(&session.cwd)?;
    ensure_git_repo(&cwd)?;
    let (_target, _repo_root, rel) = resolve_git_path(&cwd, raw_path)?;

    let mut args = vec!["diff", "-U3"];
    if staged {
        args.push("--cached");
    }
    args.push("--");
    args.push(rel.as_str());
    let diff = run_git_capture(
        &cwd,
        &args,
        default_git_diff_timeout(),
        default_git_diff_max_bytes(),
    )?;

    Ok(ApiGitDiffResponse {
        ok: true,
        cwd: cwd.display().to_string(),
        path: rel,
        staged,
        diff,
    })
}

pub fn load_git_file_versions_response(
    config: &RuntimeConfig,
    session_id: &str,
    raw_path: &str,
) -> Result<ApiGitFileVersionsResponse, String> {
    let session = find_session(config, session_id)?;
    let cwd = resolve_search_root(&session.cwd)?;
    ensure_git_repo(&cwd)?;
    let (target, _repo_root, rel) = resolve_git_path(&cwd, raw_path)?;

    let mut current_text = String::new();
    let mut current_size = 0_u64;
    let current_exists = target.exists() && target.is_file();
    if current_exists {
        let (text, size) = read_text_file_strict(&target, FILE_READ_MAX_BYTES)?;
        current_text = text;
        current_size = size;
    }

    let mut base_exists = false;
    let mut base_text = String::new();
    if let Ok(text) = run_git_capture(
        &cwd,
        &["show", &format!("HEAD:{rel}")],
        default_git_diff_timeout(),
        FILE_READ_MAX_BYTES as usize,
    ) {
        base_exists = true;
        base_text = text;
    }

    Ok(ApiGitFileVersionsResponse {
        ok: true,
        cwd: cwd.display().to_string(),
        path: rel,
        abs_path: target.display().to_string(),
        current_exists,
        current_size,
        current_text,
        base_exists,
        base_text,
    })
}

pub fn load_file_read_response(
    config: &RuntimeConfig,
    session_id: &str,
    raw_path: &str,
) -> Result<Value, String> {
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

pub fn save_file_write_response(
    config: &RuntimeConfig,
    session_id: &str,
    payload: &Value,
) -> Result<Value, FileWriteError> {
    let object = payload.as_object().ok_or_else(|| {
        FileWriteError::BadRequest("invalid json body (expected object)".to_string())
    })?;
    let path_raw = object
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| FileWriteError::BadRequest("path required".to_string()))?;
    let text_raw = object
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| FileWriteError::BadRequest("text must be a string".to_string()))?;
    let create = object
        .get("create")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let version_raw = object
        .get("version")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if !create && version_raw.is_empty() {
        return Err(FileWriteError::BadRequest("version required".to_string()));
    }

    let session = find_session(config, session_id).map_err(|message| {
        if message.starts_with("unknown session:") {
            FileWriteError::NotFound("unknown session".to_string())
        } else {
            FileWriteError::Internal(message)
        }
    })?;
    let base = expand_home(&session.cwd);
    let (resolved, size, next_version) = if create {
        let target = resolve_relative_under(&base, path_raw).map_err(FileWriteError::BadRequest)?;
        match write_new_text_file_atomic(&target, text_raw) {
            Ok(result) => (target, result.0, result.1),
            Err(message) if message == "file already exists" => {
                let mut payload = json!({
                    "error": "file already exists",
                    "conflict": true,
                    "path": target.display().to_string(),
                });
                if target.is_file() {
                    if let Ok((_text, _size, current_version)) =
                        read_text_file_for_write(&target, FILE_READ_MAX_BYTES)
                    {
                        payload["version"] = Value::String(current_version);
                    }
                }
                return Err(FileWriteError::Conflict(payload));
            }
            Err(message) => return Err(map_file_write_message(message)),
        }
    } else {
        let target =
            resolve_session_path(&session.cwd, path_raw).map_err(FileWriteError::BadRequest)?;
        let (_current_text, _current_size, current_version) =
            read_text_file_for_write(&target, FILE_READ_MAX_BYTES)
                .map_err(map_file_write_message)?;
        if current_version != version_raw {
            return Err(FileWriteError::Conflict(json!({
                "error": "file changed on disk",
                "conflict": true,
                "path": target.display().to_string(),
                "version": current_version,
            })));
        }
        let (size, next_version) =
            write_editable_text_file_atomic(&target, text_raw).map_err(map_file_write_message)?;
        (target, size, next_version)
    };

    update_session_file_history(config, session_id, &resolved.display().to_string())
        .map_err(FileWriteError::Internal)?;

    Ok(json!({
        "ok": true,
        "path": resolved.display().to_string(),
        "rel": path_raw,
        "size": size,
        "version": next_version,
        "editable": true,
    }))
}

pub fn load_file_blob(
    config: &RuntimeConfig,
    session_id: &str,
    raw_path: &str,
) -> Result<(Vec<u8>, String), String> {
    let session = find_session(config, session_id)?;
    let resolved = resolve_session_path(&session.cwd, raw_path)?;
    let raw = fs::read(&resolved).map_err(map_io_error)?;
    let (_kind, content_type) = detect_file_kind(&resolved, &raw);
    match content_type {
        Some(value) if value.starts_with("image/") || value == "application/pdf" => {
            Ok((raw, value.to_string()))
        }
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
    let meta_updated_ts = meta.updated_ts.unwrap_or(start_ts);
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
    let priority_offset = object_number(sidebar_entry, "priority_offset")
        .unwrap_or(0.0)
        .clamp(-1.0, 1.0);
    let blocked = object_string(sidebar_entry, "dependency_session_id").is_some();
    let snooze_until = object_number(sidebar_entry, "snooze_until");
    let snoozed = snooze_until
        .map(|value| value > epoch_now())
        .unwrap_or(false);
    let queue_len = queues.get(session_id).map(|items| items.len()).unwrap_or(0);
    let thread_id = clean_optional(meta.session_id).unwrap_or_else(|| session_id.to_string());
    let mut model_provider = clean_optional(meta.model_provider);
    let preferred_auth_method = clean_optional(meta.preferred_auth_method);
    let mut model = clean_optional(meta.model);
    let mut reasoning_effort = clean_optional(meta.reasoning_effort);
    let service_tier = clean_optional(meta.service_tier);
    let mut log_path = clean_optional(meta.log_path);
    if log_path.is_none() && pid_alive(pid) {
        log_path = discover_open_log_for_process(pid, &cwd, &agent_backend)
            .map(|path| path.to_string_lossy().to_string());
    }
    let broker_state = match read_broker_state(sock_path) {
        Ok(state) => state,
        Err(_) if !pid_alive(pid) && !pid_alive(broker_pid) => return Ok(None),
        Err(_) => None,
    };
    if (model_provider.is_none() || model.is_none() || reasoning_effort.is_none())
        && log_path.as_deref().is_some()
    {
        let log = Path::new(log_path.as_deref().unwrap_or_default());
        if log.exists() {
            let (log_provider, log_model, log_effort) =
                read_run_settings_from_log(log, &agent_backend);
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
        .unwrap_or(meta_updated_ts);
    let time_priority = priority_from_elapsed_seconds((epoch_now() - updated_ts).max(0.0));
    let base_priority = clip01(time_priority + priority_offset);
    let final_priority = if blocked || snoozed {
        0.0
    } else {
        base_priority
    };
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
        .or_else(|| {
            log_path
                .as_deref()
                .map(Path::new)
                .filter(|path| path.exists())
                .and_then(latest_token_update_from_log)
        });
    let last_assistant_ts = log_path
        .as_deref()
        .map(Path::new)
        .filter(|path| path.exists())
        .and_then(last_assistant_ts_from_log);
    let git_branch = current_git_branch(Path::new(&cwd));
    Ok(Some(ApiSessionSummary {
        session_id: session_id.to_string(),
        thread_id: Some(thread_id),
        pid,
        broker_pid,
        agent_backend,
        owned: meta.owner.as_deref() == Some("web"),
        transport,
        workspace_cwd: clean_optional(meta.workspace_cwd),
        cwd,
        start_ts,
        updated_ts,
        log_path: log_path.clone(),
        queue_len,
        busy,
        token,
        harness_enabled: object_bool(harness_entry, "enabled").unwrap_or(false),
        harness_cooldown_minutes: object_number(harness_entry, "cooldown_minutes")
            .unwrap_or(HARNESS_DEFAULT_IDLE_MINUTES),
        harness_remaining_injections: object_i64(harness_entry, "remaining_injections")
            .unwrap_or(HARNESS_DEFAULT_MAX_INJECTIONS),
        alias: aliases.get(session_id).cloned().unwrap_or_default(),
        files: files.get(session_id).cloned().unwrap_or_default(),
        git_branch,
        model_provider: model_provider.clone(),
        preferred_auth_method: preferred_auth_method.clone(),
        provider_choice: provider_choice_for_settings(
            model_provider.as_deref(),
            preferred_auth_method.as_deref(),
        ),
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
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        text: value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        created_ts,
        sending: value
            .get("sending")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn normalized_queue_values(values: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let Some(item) = coerce_queue_item_value(value) else {
            continue;
        };
        let item_id = item
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let unique_item = if seen.insert(item_id.clone()) {
            item
        } else {
            let text = item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let created_ts = item.get("created_ts").and_then(Value::as_f64);
            let sending = item
                .get("sending")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut regenerated = new_queue_item_value(&text, created_ts);
            if sending {
                regenerated["sending"] = Value::Bool(true);
            }
            regenerated
        };
        seen.insert(
            unique_item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        );
        out.push(unique_item);
    }
    out
}

fn coerce_queue_item_value(value: &Value) -> Option<Value> {
    match value {
        Value::String(text) => {
            if text.trim().is_empty() {
                None
            } else {
                Some(new_queue_item_value(text, None))
            }
        }
        Value::Object(object) => {
            let text = object.get("text").and_then(Value::as_str)?;
            if text.trim().is_empty() {
                return None;
            }
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .unwrap_or_else(new_queue_item_id);
            let created_ts = object
                .get("created_ts")
                .and_then(Value::as_f64)
                .filter(|ts| ts.is_finite() && *ts > 0.0);
            let sending = object
                .get("sending")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut item = new_queue_item_value(text, created_ts);
            item["id"] = Value::String(id);
            if sending {
                item["sending"] = Value::Bool(true);
            }
            Some(item)
        }
        _ => None,
    }
}

fn new_queue_item_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = QUEUE_ITEM_COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    format!("queue-{nanos:016x}{counter:016x}")
}

fn new_queue_item_value(text: &str, created_ts: Option<f64>) -> Value {
    let ts = created_ts
        .filter(|ts| ts.is_finite() && *ts > 0.0)
        .unwrap_or_else(epoch_now);
    json!({
        "id": new_queue_item_id(),
        "text": text,
        "created_ts": ts,
    })
}

fn set_normalized_queue_values(
    queues: &mut HashMap<String, Vec<Value>>,
    session_id: &str,
    items: Vec<Value>,
) {
    if items.is_empty() {
        queues.remove(session_id);
    } else {
        queues.insert(session_id.to_string(), items);
    }
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("missing parent for {}", path.display()))?;
    fs::create_dir_all(parent).map_err(|err| format!("create {}: {err}", parent.display()))?;
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(value)
        .map(|body| body + "\n")
        .map_err(|err| format!("serialize {}: {err}", path.display()))?;
    fs::write(&tmp, raw).map_err(|err| format!("write {}: {err}", tmp.display()))?;
    fs::rename(&tmp, path)
        .map_err(|err| format!("rename {} -> {}: {err}", tmp.display(), path.display()))
}

fn write_text_file_atomic(path: &Path, text: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("missing parent for {}", path.display()))?;
    fs::create_dir_all(parent).map_err(|err| format!("create {}: {err}", parent.display()))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("invalid filename for {}", path.display()))?;
    let tmp = path.with_file_name(format!("{file_name}.tmp"));
    fs::write(&tmp, text).map_err(|err| format!("write {}: {err}", tmp.display()))?;
    fs::rename(&tmp, path)
        .map_err(|err| format!("rename {} -> {}: {err}", tmp.display(), path.display()))
}

fn unique_temp_sibling_path(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("invalid filename for {}", path.display()))?;
    let counter = QUEUE_ITEM_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(path.with_file_name(format!(".{file_name}.codoxear-tmp-{counter:016x}")))
}

fn write_editable_text_file_atomic(path: &Path, text: &str) -> Result<(u64, String), String> {
    let data = text.as_bytes();
    if data.len() as u64 > FILE_READ_MAX_BYTES {
        return Err(format!("file too large (max {FILE_READ_MAX_BYTES} bytes)"));
    }
    let metadata = fs::metadata(path).map_err(map_io_error)?;
    let tmp = unique_temp_sibling_path(path)?;
    let mode = metadata.permissions().mode() & 0o777;
    let mut write_result = Ok(());
    if let Err(err) = fs::write(&tmp, data) {
        write_result = Err(map_io_error(err));
    }
    if write_result.is_ok() {
        if let Err(err) = fs::set_permissions(&tmp, fs::Permissions::from_mode(mode)) {
            write_result = Err(map_io_error(err));
        }
    }
    if write_result.is_ok() {
        if let Err(err) = fs::rename(&tmp, path) {
            write_result = Err(map_io_error(err));
        }
    }
    if let Err(err) = fs::remove_file(&tmp) {
        if err.kind() != std::io::ErrorKind::NotFound && write_result.is_ok() {
            write_result = Err(map_io_error(err));
        }
    }
    write_result?;
    Ok((data.len() as u64, content_version(data)))
}

fn write_new_text_file_atomic(path: &Path, text: &str) -> Result<(u64, String), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("missing parent for {}", path.display()))?;
    if !parent.exists() {
        return Err("parent directory not found".to_string());
    }
    if !parent.is_dir() {
        return Err("parent path is not a directory".to_string());
    }
    if path.exists() {
        return Err("file already exists".to_string());
    }
    let data = text.as_bytes();
    if data.len() as u64 > FILE_READ_MAX_BYTES {
        return Err(format!("file too large (max {FILE_READ_MAX_BYTES} bytes)"));
    }
    let tmp = unique_temp_sibling_path(path)?;
    let mut write_result = Ok(());
    if let Err(err) = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o666)
        .open(&tmp)
        .and_then(|mut file| file.write_all(data))
    {
        write_result = Err(map_io_error(err));
    }
    if write_result.is_ok() {
        if let Err(err) = fs::hard_link(&tmp, path) {
            write_result = Err(if err.kind() == std::io::ErrorKind::AlreadyExists {
                "file already exists".to_string()
            } else {
                map_io_error(err)
            });
        }
    }
    if let Err(err) = fs::remove_file(&tmp) {
        if err.kind() != std::io::ErrorKind::NotFound && write_result.is_ok() {
            write_result = Err(map_io_error(err));
        }
    }
    write_result?;
    Ok((data.len() as u64, content_version(data)))
}

fn write_string_map(path: &Path, values: &HashMap<String, String>) -> Result<(), String> {
    let mut keys = values.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    let mut object = serde_json::Map::new();
    for key in keys {
        let Some(value) = values.get(&key) else {
            continue;
        };
        object.insert(key, Value::String(value.clone()));
    }
    write_json_value(path, &Value::Object(object))
}

fn write_string_array_map(
    path: &Path,
    values: &HashMap<String, Vec<String>>,
) -> Result<(), String> {
    let mut keys = values.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    let mut object = serde_json::Map::new();
    for key in keys {
        let Some(items) = values.get(&key) else {
            continue;
        };
        if items.is_empty() {
            continue;
        }
        object.insert(
            key,
            Value::Array(items.iter().cloned().map(Value::String).collect::<Vec<_>>()),
        );
    }
    write_json_value(path, &Value::Object(object))
}

fn write_object_map(path: &Path, values: &HashMap<String, Value>) -> Result<(), String> {
    let mut keys = values.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    let mut object = serde_json::Map::new();
    for key in keys {
        let Some(value) = values.get(&key) else {
            continue;
        };
        object.insert(key, value.clone());
    }
    write_json_value(path, &Value::Object(object))
}

fn write_queue_map(path: &Path, queues: &HashMap<String, Vec<Value>>) -> Result<(), String> {
    let mut keys = queues.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    let mut object = serde_json::Map::new();
    for key in keys {
        let Some(items) = queues.get(&key) else {
            continue;
        };
        if items.is_empty() {
            continue;
        }
        object.insert(key, Value::Array(items.clone()));
    }
    write_json_value(path, &Value::Object(object))
}

fn clean_alias(name: &str) -> String {
    let mut cleaned = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        return String::new();
    }
    if cleaned.chars().count() > 80 {
        cleaned = cleaned
            .chars()
            .take(80)
            .collect::<String>()
            .trim_end()
            .to_string();
    }
    cleaned
}

fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number
            .as_i64()
            .map(|value| value != 0)
            .or_else(|| number.as_u64().map(|value| value != 0))
            .or_else(|| number.as_f64().map(|value| value != 0.0))
            .unwrap_or(false),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn notification_subscription_id(endpoint: &str) -> String {
    let digest = Sha256::digest(endpoint.as_bytes());
    digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn notification_device_class_from_user_agent(user_agent: &str) -> String {
    let ua = user_agent.trim().to_ascii_lowercase();
    if ua.contains("mobile")
        || ua.contains("android")
        || ua.contains("iphone")
        || ua.contains("ipad")
        || ua.contains("ipod")
    {
        "mobile".to_string()
    } else {
        "desktop".to_string()
    }
}

fn clean_notification_device_class(raw: &str, user_agent: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "mobile" => "mobile".to_string(),
        "desktop" => "desktop".to_string(),
        _ => notification_device_class_from_user_agent(user_agent),
    }
}

fn clean_notification_subscription(raw: &Value) -> Result<Value, String> {
    let object = raw
        .as_object()
        .ok_or_else(|| "subscription must be an object".to_string())?;
    let endpoint = object
        .get("endpoint")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "subscription endpoint required".to_string())?;
    let keys = object
        .get("keys")
        .and_then(Value::as_object)
        .ok_or_else(|| "subscription keys required".to_string())?;
    let p256dh = keys
        .get("p256dh")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "subscription keys.p256dh and keys.auth required".to_string())?;
    let auth = keys
        .get("auth")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "subscription keys.p256dh and keys.auth required".to_string())?;
    Ok(json!({
        "endpoint": endpoint,
        "keys": {
            "p256dh": p256dh,
            "auth": auth,
        }
    }))
}

fn clean_notification_subscription_record(raw: &Value, now_ts: f64) -> Option<Value> {
    let object = raw.as_object()?;
    let subscription = clean_notification_subscription(object.get("subscription")?).ok()?;
    let endpoint = subscription.get("endpoint").and_then(Value::as_str)?;
    let created_ts = object
        .get("created_ts")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(now_ts);
    let updated_ts = object
        .get("updated_ts")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(created_ts);
    let user_agent = object
        .get("user_agent")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let device_class = clean_notification_device_class(
        object
            .get("device_class")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        &user_agent,
    );
    Some(json!({
        "id": notification_subscription_id(endpoint),
        "subscription": subscription,
        "notifications_enabled": object.get("notifications_enabled").map(json_truthy).unwrap_or(true),
        "created_ts": created_ts,
        "updated_ts": updated_ts,
        "last_success_ts": object.get("last_success_ts").and_then(Value::as_f64),
        "last_failure_ts": object.get("last_failure_ts").and_then(Value::as_f64),
        "last_error": object
            .get("last_error")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim(),
        "user_agent": user_agent,
        "device_label": object
            .get("device_label")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim(),
        "device_class": device_class,
    }))
}

fn read_notification_subscription_records(path: &Path) -> Result<HashMap<String, Value>, String> {
    let Some(Value::Array(items)) = read_optional_value(path)? else {
        return Ok(HashMap::new());
    };
    let now_ts = epoch_now();
    let mut out = HashMap::new();
    for item in items {
        let Some(record) = clean_notification_subscription_record(&item, now_ts) else {
            continue;
        };
        let Some(record_id) = record.get("id").and_then(Value::as_str) else {
            continue;
        };
        out.insert(record_id.to_string(), record);
    }
    Ok(out)
}

fn write_notification_subscription_records(
    path: &Path,
    records: &HashMap<String, Value>,
) -> Result<(), String> {
    let mut ids = records.keys().cloned().collect::<Vec<_>>();
    ids.sort();
    let items = ids
        .into_iter()
        .filter_map(|record_id| records.get(&record_id).cloned())
        .collect::<Vec<_>>();
    write_json_value(path, &Value::Array(items))
}

fn notification_subscriptions_snapshot_value(
    records: &HashMap<String, Value>,
    vapid_public_key: &str,
) -> Value {
    let mut items = records
        .values()
        .filter_map(|record| {
            let endpoint = record
                .get("subscription")
                .and_then(Value::as_object)
                .and_then(|subscription| subscription.get("endpoint"))
                .and_then(Value::as_str)?;
            Some(json!({
                "id": record.get("id").and_then(Value::as_str).unwrap_or_default(),
                "endpoint": endpoint,
                "notifications_enabled": record.get("notifications_enabled").map(json_truthy).unwrap_or(false),
                "device_class": record.get("device_class").and_then(Value::as_str).unwrap_or("desktop"),
                "created_ts": record.get("created_ts").and_then(Value::as_f64),
                "updated_ts": record.get("updated_ts").and_then(Value::as_f64),
                "last_success_ts": record.get("last_success_ts").and_then(Value::as_f64),
                "last_failure_ts": record.get("last_failure_ts").and_then(Value::as_f64),
                "last_error": record.get("last_error").and_then(Value::as_str).unwrap_or_default(),
                "user_agent": record.get("user_agent").and_then(Value::as_str).unwrap_or_default(),
                "device_label": record.get("device_label").and_then(Value::as_str).unwrap_or_default(),
            }))
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        let left_ts = left
            .get("updated_ts")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let right_ts = right
            .get("updated_ts")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        right_ts
            .partial_cmp(&left_ts)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    json!({
        "ok": true,
        "vapid_public_key": vapid_public_key,
        "subscriptions": items,
    })
}

fn string_field(row: &serde_json::Map<String, Value>, field: &str) -> String {
    row.get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn compact_text(raw: impl AsRef<str>) -> String {
    raw.as_ref()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn clip_text(raw: &str, limit: usize) -> String {
    let text = compact_text(raw);
    if text.chars().count() <= limit {
        return text;
    }
    text.chars()
        .take(limit.saturating_sub(1))
        .collect::<String>()
        .trim_end()
        .to_string()
        + "..."
}

fn default_session_display_name(raw: String) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        "Session".to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_voice_base_url(raw: Option<&str>) -> Result<String, String> {
    let value = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_TTS_BASE_URL);
    if !value.starts_with("http://") && !value.starts_with("https://") {
        return Err("tts_base_url must start with http:// or https://".to_string());
    }
    Ok(value.trim_end_matches('/').to_string())
}

fn clean_voice_settings_object(
    object: Option<&serde_json::Map<String, Value>>,
) -> Result<Value, String> {
    let narration = object
        .and_then(|row| row.get("tts_enabled_for_narration"))
        .map(json_truthy)
        .unwrap_or(false);
    let final_response = object
        .and_then(|row| row.get("tts_enabled_for_final_response"))
        .map(json_truthy)
        .unwrap_or(false);
    let base_url = normalize_voice_base_url(
        object
            .and_then(|row| row.get("tts_base_url"))
            .and_then(Value::as_str),
    )?;
    let api_key = object
        .and_then(|row| row.get("tts_api_key"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let summarization_model = object
        .and_then(|row| row.get("summarization_model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_SUMMARIZATION_MODEL)
        .to_string();
    let tts_model = object
        .and_then(|row| row.get("tts_model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_TTS_MODEL)
        .to_string();
    Ok(json!({
        "tts_enabled_for_narration": narration,
        "tts_enabled_for_final_response": final_response,
        "tts_base_url": base_url,
        "tts_api_key": api_key,
        "summarization_model": summarization_model,
        "tts_model": tts_model,
    }))
}

fn clean_voice_settings_value(payload: &Value) -> Result<Value, String> {
    let Some(object) = payload.as_object() else {
        return Err("invalid json body (expected object)".to_string());
    };
    clean_voice_settings_object(Some(object))
}

fn read_clean_voice_settings(path: &Path) -> Result<Value, String> {
    let value = read_optional_value(path)?;
    clean_voice_settings_object(value.as_ref().and_then(Value::as_object))
}

fn read_voice_runtime_audio_snapshot(path: &Path) -> Result<Value, String> {
    let audio = read_optional_value(path)?
        .and_then(|value| value.get("audio").cloned())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    Ok(json!({
        "queue_depth": audio.get("queue_depth").and_then(Value::as_i64).unwrap_or(0),
        "active_listener_count": audio
            .get("active_listener_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        "segment_count": audio.get("segment_count").and_then(Value::as_i64).unwrap_or(0),
        "last_error": audio
            .get("last_error")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim(),
        "media_sequence": audio.get("media_sequence").and_then(Value::as_i64).unwrap_or(1),
    }))
}

fn read_voice_delivery_ledger(path: &Path) -> Result<HashMap<String, Value>, String> {
    let Some(Value::Object(object)) = read_optional_value(path)? else {
        return Ok(HashMap::new());
    };
    let mut out = HashMap::new();
    for (message_id, value) in object {
        if message_id.trim().is_empty() {
            continue;
        }
        let Some(mut row) = value.as_object().cloned() else {
            continue;
        };
        let session_id = string_field(&row, "session_id");
        let message_class = string_field(&row, "message_class");
        if session_id.is_empty()
            || (message_class != "narration" && message_class != "final_response")
        {
            continue;
        }
        row.insert("message_id".to_string(), Value::String(message_id.clone()));
        out.insert(message_id, Value::Object(row));
    }
    Ok(out)
}

fn notification_mobile_device_counts(records: &HashMap<String, Value>) -> (i64, i64) {
    let mut enabled = 0_i64;
    let mut total = 0_i64;
    for record in records.values() {
        let Some(object) = record.as_object() else {
            continue;
        };
        if object.get("device_class").and_then(Value::as_str) != Some("mobile") {
            continue;
        }
        total += 1;
        if object
            .get("notifications_enabled")
            .map(json_truthy)
            .unwrap_or(false)
        {
            enabled += 1;
        }
    }
    (enabled, total)
}

fn clean_harness_cooldown_minutes_value(value: &Value) -> Result<i64, String> {
    if value.is_boolean() {
        return Err("harness cooldown_minutes must be an integer".to_string());
    }
    let Some(raw) = value.as_i64() else {
        return Err("harness cooldown_minutes must be an integer".to_string());
    };
    if raw < 1 {
        return Err("harness cooldown_minutes must be at least 1".to_string());
    }
    Ok(raw)
}

fn clean_harness_remaining_injections_value(
    value: &Value,
    allow_zero: bool,
) -> Result<i64, String> {
    if value.is_boolean() {
        return Err("harness remaining_injections must be an integer".to_string());
    }
    let Some(raw) = value.as_i64() else {
        return Err("harness remaining_injections must be an integer".to_string());
    };
    let minimum = if allow_zero { 0 } else { 1 };
    if raw < minimum {
        return Err(format!(
            "harness remaining_injections must be at least {}",
            if allow_zero { 0 } else { 1 }
        ));
    }
    Ok(raw)
}

fn safe_filename(name: &str, default: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let mut out = String::new();
    for ch in base.chars() {
        if ch.is_alphanumeric() || matches!(ch, '-' | '_' | '.' | ' ') {
            out.push(ch);
        }
    }
    let cleaned = out.trim().replace(' ', "_");
    if cleaned.is_empty() {
        return default.to_string();
    }
    cleaned.chars().take(96).collect()
}

fn stage_uploaded_file(
    config: &RuntimeConfig,
    session_id: &str,
    filename: &str,
    raw: &[u8],
) -> Result<PathBuf, String> {
    if session_id.trim().is_empty() {
        return Err("session_id required".to_string());
    }
    if filename.trim().is_empty() {
        return Err("filename required".to_string());
    }
    if raw.len() > ATTACH_UPLOAD_MAX_BYTES {
        return Err(format!(
            "file too large (max {ATTACH_UPLOAD_MAX_BYTES} bytes)"
        ));
    }
    let safe_name = safe_filename(filename, "file");
    let subdir = config.app_dir.join("uploads").join(session_id);
    fs::create_dir_all(&subdir)
        .map_err(|err| format!("create upload dir {}: {err}", subdir.display()))?;
    let out_path = subdir.join(format!("{}_{}", (epoch_now() * 1000.0) as i64, safe_name));
    fs::write(&out_path, raw)
        .map_err(|err| format!("write upload {}: {err}", out_path.display()))?;
    fs::set_permissions(&out_path, fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("chmod upload {}: {err}", out_path.display()))?;
    Ok(out_path)
}

fn attachment_inject_text(attachment_index: i64, path: &Path) -> Result<String, String> {
    if attachment_index <= 0 {
        return Err("attachment_index must be >= 1".to_string());
    }
    Ok(format!(
        "Attachment {attachment_index}: {}\n",
        path.display()
    ))
}

fn clean_priority_offset_value(raw: Option<&Value>) -> Result<f64, String> {
    let Some(value) = raw else {
        return Ok(0.0);
    };
    if value.is_null() {
        return Ok(0.0);
    }
    if value.is_boolean() {
        return Err("priority_offset must be a number".to_string());
    }
    let Some(out) = value.as_f64() else {
        return Err("priority_offset must be a number".to_string());
    };
    if !out.is_finite() {
        return Err("priority_offset must be finite".to_string());
    }
    if !(-1.0..=1.0).contains(&out) {
        return Err("priority_offset must be within [-1, 1]".to_string());
    }
    Ok(out)
}

fn clean_snooze_until_value(raw: Option<&Value>) -> Result<Option<f64>, String> {
    let Some(value) = raw else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    if let Some(text) = value.as_str() {
        if text.trim().is_empty() {
            return Ok(None);
        }
        return Err("snooze_until must be a unix timestamp or null".to_string());
    }
    if value.is_boolean() {
        return Err("snooze_until must be a unix timestamp or null".to_string());
    }
    let Some(out) = value.as_f64() else {
        return Err("snooze_until must be a unix timestamp or null".to_string());
    };
    if !out.is_finite() {
        return Err("snooze_until must be finite".to_string());
    }
    if out <= 0.0 {
        return Ok(None);
    }
    Ok(Some(out))
}

fn clean_dependency_session_id_value(raw: Option<&Value>) -> Result<Option<String>, String> {
    let Some(value) = raw else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(text) = value.as_str() else {
        return Err("dependency_session_id must be a string or null".to_string());
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed.to_string()))
}

fn clear_deleted_session_state(
    config: &RuntimeConfig,
    session_id: &str,
    cwd: Option<&str>,
) -> Result<(), String> {
    let alias_path = config.app_dir.join("session_aliases.json");
    let mut aliases = read_string_map(&alias_path)?;
    aliases.remove(session_id);
    write_string_map(&alias_path, &aliases)?;

    let sidebar_path = config.app_dir.join("session_sidebar.json");
    let mut sidebar = read_object_map(&sidebar_path)?;
    sidebar.remove(session_id);
    for value in sidebar.values_mut() {
        let Some(object) = value.as_object_mut() else {
            continue;
        };
        if object.get("dependency_session_id").and_then(Value::as_str) == Some(session_id) {
            object.remove("dependency_session_id");
        }
    }
    write_object_map(&sidebar_path, &sidebar)?;

    let harness_path = config.app_dir.join("harness.json");
    let mut harness = read_object_map(&harness_path)?;
    harness.remove(session_id);
    write_object_map(&harness_path, &harness)?;

    let files_path = config.app_dir.join("session_files.json");
    let mut files = read_string_array_map(&files_path)?;
    files.remove(session_id);
    files.remove(&format!("sid:{session_id}"));
    if let Some(cwd_value) = cwd.filter(|value| !value.trim().is_empty()) {
        files.remove(&format!("cwd:{cwd_value}"));
    }
    write_string_array_map(&files_path, &files)?;

    let queue_path = config.app_dir.join("session_queues.json");
    let mut queues = read_array_map(&queue_path)?;
    queues.remove(session_id);
    write_queue_map(&queue_path, &queues)
}

fn unlink_quiet(path: &Path) {
    let _ = fs::remove_file(path);
}

fn signal_process(pid: i64, signal: &str) -> bool {
    if pid <= 0 {
        return false;
    }
    Command::new("kill")
        .args([signal, &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn signal_process_group(root_pid: i64, signal: &str) -> bool {
    if root_pid <= 0 {
        return false;
    }
    Command::new("kill")
        .args([signal, "--", &format!("-{root_pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn process_group_alive(root_pid: i64) -> bool {
    if root_pid <= 0 {
        return false;
    }
    Command::new("kill")
        .args(["-0", "--", &format!("-{root_pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn terminate_process(pid: i64, wait: Duration) -> bool {
    if !pid_alive(pid) {
        return true;
    }
    if !signal_process(pid, "-TERM") {
        return false;
    }
    let deadline = Instant::now() + wait;
    while pid_alive(pid) {
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if !pid_alive(pid) {
        return true;
    }
    if !signal_process(pid, "-KILL") {
        return false;
    }
    let deadline = Instant::now() + Duration::from_millis(200);
    while pid_alive(pid) {
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !pid_alive(pid)
}

fn terminate_process_group(root_pid: i64, wait: Duration) -> bool {
    if !process_group_alive(root_pid) {
        return true;
    }
    if !signal_process_group(root_pid, "-TERM") {
        return false;
    }
    let deadline = Instant::now() + wait;
    while process_group_alive(root_pid) {
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if !process_group_alive(root_pid) {
        return true;
    }
    if !signal_process_group(root_pid, "-KILL") {
        return false;
    }
    let deadline = Instant::now() + Duration::from_millis(200);
    while process_group_alive(root_pid) {
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !process_group_alive(root_pid)
}

fn kill_session_via_pids(config: &RuntimeConfig, session: &ApiSessionSummary) -> bool {
    let sock_path = session_sock_path(config, &session.session_id);
    let meta_path = sock_path.with_extension("json");
    let group_alive = process_group_alive(session.pid);
    let broker_alive = pid_alive(session.broker_pid);
    if !group_alive && !broker_alive {
        unlink_quiet(&sock_path);
        unlink_quiet(&meta_path);
        return true;
    }
    if group_alive && !terminate_process_group(session.pid, Duration::from_secs_f64(1.0)) {
        return false;
    }
    if pid_alive(session.broker_pid)
        && !terminate_process(session.broker_pid, Duration::from_secs_f64(1.0))
    {
        return false;
    }
    let group_dead = !process_group_alive(session.pid);
    let broker_dead = !pid_alive(session.broker_pid);
    if group_dead && broker_dead {
        unlink_quiet(&sock_path);
        unlink_quiet(&meta_path);
        return true;
    }
    false
}

fn queue_remote_ready(config: &RuntimeConfig, session: &ApiSessionSummary) -> Result<bool, String> {
    let broker_state = read_live_broker_state(config, session)?;
    if broker_state.busy || broker_state._queue_len > 0 {
        return Ok(false);
    }
    if let Some(log_path) = session.log_path.as_deref() {
        let path = Path::new(log_path);
        if path.exists() {
            return Ok(matches!(compute_idle_from_log(path), Some(true)));
        }
    }
    Ok(true)
}

fn queue_sweep_interval() -> Duration {
    let seconds = env::var("CODEX_WEB_QUEUE_SWEEP_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_QUEUE_SWEEP_SECONDS);
    Duration::from_secs_f64(seconds)
}

fn harness_sweep_interval() -> Duration {
    let seconds = env::var("CODEX_WEB_HARNESS_SWEEP_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_HARNESS_SWEEP_SECONDS);
    Duration::from_secs_f64(seconds)
}

fn voice_scan_interval() -> Duration {
    let seconds = env::var("CODEX_WEB_VOICE_PUSH_SWEEP_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_VOICE_SCAN_SECONDS);
    Duration::from_secs_f64(seconds)
}

fn queue_idle_grace_seconds() -> f64 {
    env::var("CODEX_WEB_QUEUE_IDLE_GRACE_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(DEFAULT_QUEUE_IDLE_GRACE_SECONDS)
}

fn harness_max_scan_bytes() -> usize {
    env::var("CODEX_WEB_HARNESS_MAX_SCAN_BYTES")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_HARNESS_MAX_SCAN_BYTES)
}

fn render_harness_prompt(request: Option<&str>) -> String {
    let base = HARNESS_PROMPT_PREFIX.trim_end();
    let request_text = request.unwrap_or_default().trim();
    if request_text.is_empty() {
        format!("{base}\n")
    } else {
        format!("{base}\n\n---\n\nAdditional request from user: {request_text}\n")
    }
}

fn map_io_error(err: std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::NotFound => "file not found".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        _ => err.to_string(),
    }
}

fn default_git_diff_max_bytes() -> usize {
    env::var("CODEX_WEB_GIT_DIFF_MAX_BYTES")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(GIT_DIFF_MAX_BYTES)
}

fn default_git_diff_timeout() -> Duration {
    let seconds = env::var("CODEX_WEB_GIT_DIFF_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(GIT_DIFF_TIMEOUT_SECONDS);
    Duration::from_secs_f64(seconds)
}

fn default_git_changed_files_max() -> usize {
    env::var("CODEX_WEB_GIT_CHANGED_FILES_MAX")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(GIT_CHANGED_FILES_MAX)
}

fn ensure_git_repo(cwd: &Path) -> Result<(), String> {
    run_git_capture(
        cwd,
        &["rev-parse", "--is-inside-work-tree"],
        default_git_diff_timeout(),
        4096,
    )
    .map(|_| ())
}

fn run_git_capture(
    cwd: &Path,
    args: &[&str],
    timeout: Duration,
    max_bytes: usize,
) -> Result<String, String> {
    let mut child = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(map_io_error)?;

    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().map_err(map_io_error)?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("git command timed out".to_string());
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let output = child.wait_with_output().map_err(map_io_error)?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!(
                "git failed with code {}",
                output.status.code().unwrap_or_default()
            )
        } else {
            err
        });
    }
    if output.stdout.len() > max_bytes {
        return Err(format!("git output too large (max {max_bytes} bytes)"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn normalize_git_path_list(text: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let path = raw.trim();
        if path.is_empty() {
            continue;
        }
        out.push(path.to_string());
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn parse_git_numstat(text: &str) -> HashMap<String, (Option<i64>, Option<i64>)> {
    let mut out: HashMap<String, (Option<i64>, Option<i64>)> = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let parts = line.splitn(3, '\t').collect::<Vec<_>>();
        if parts.len() != 3 {
            continue;
        }
        let path = parts[2].trim();
        if path.is_empty() {
            continue;
        }
        let additions = if parts[0] == "-" {
            None
        } else {
            parts[0].parse::<i64>().ok()
        };
        let deletions = if parts[1] == "-" {
            None
        } else {
            parts[1].parse::<i64>().ok()
        };
        if let Some(prev) = out.get_mut(path) {
            prev.0 = match (prev.0, additions) {
                (Some(left), Some(right)) => Some(left + right),
                _ => None,
            };
            prev.1 = match (prev.1, deletions) {
                (Some(left), Some(right)) => Some(left + right),
                _ => None,
            };
            continue;
        }
        out.insert(path.to_string(), (additions, deletions));
    }
    out
}

struct FileSearchState {
    heap: BinaryHeap<Reverse<(i64, String)>>,
    scanned: usize,
    truncated: bool,
    limit: usize,
    max_candidates: usize,
    deadline: Instant,
}

struct FileSearchResult {
    mode: String,
    matches: Vec<ApiFileSearchMatch>,
    scanned: usize,
    truncated: bool,
}

pub(crate) fn default_file_search_limit() -> usize {
    env::var("CODEX_WEB_FILE_SEARCH_LIMIT")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(FILE_SEARCH_LIMIT)
}

fn default_file_history_max() -> usize {
    env::var("CODEX_WEB_FILE_HISTORY_MAX")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(20)
}

fn file_search_timeout() -> Duration {
    let seconds = env::var("CODEX_WEB_FILE_SEARCH_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(FILE_SEARCH_TIMEOUT_SECONDS);
    Duration::from_secs_f64(seconds)
}

fn file_search_max_candidates() -> usize {
    env::var("CODEX_WEB_FILE_SEARCH_MAX_CANDIDATES")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(FILE_SEARCH_MAX_CANDIDATES)
}

fn resolve_search_root(raw_cwd: &str) -> Result<PathBuf, String> {
    let path = expand_home(raw_cwd);
    if path.is_absolute() {
        return Ok(path);
    }
    env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|err| err.to_string())
}

fn git_repo_root(cwd: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8(output.stdout).ok()?;
    let trimmed = root.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

fn search_walk_relative_files(
    root: &Path,
    query: &str,
    limit: usize,
) -> Result<FileSearchResult, String> {
    let mut state = FileSearchState {
        heap: BinaryHeap::new(),
        scanned: 0,
        truncated: false,
        limit,
        max_candidates: file_search_max_candidates(),
        deadline: Instant::now() + file_search_timeout(),
    };
    walk_relative_files(root, root, query, &mut state)?;
    Ok(finish_file_search(state, "walk"))
}

fn walk_relative_files(
    root: &Path,
    current: &Path,
    query: &str,
    state: &mut FileSearchState,
) -> Result<(), String> {
    let mut entries = fs::read_dir(current)
        .map_err(map_io_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_io_error)?;
    entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    for entry in entries {
        if state.truncated {
            break;
        }
        let path = entry.path();
        let file_type = entry.file_type().map_err(map_io_error)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if file_type.is_dir() {
            if FILE_LIST_IGNORED_DIRS
                .iter()
                .any(|ignored| *ignored == name)
            {
                continue;
            }
            walk_relative_files(root, &path, query, state)?;
            continue;
        }
        state.scanned += 1;
        if state.scanned > state.max_candidates || Instant::now() > state.deadline {
            state.truncated = true;
            break;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let score = file_search_score(&rel, query);
        if score < 0 {
            continue;
        }
        push_file_search_match(&mut state.heap, &rel, score, state.limit);
    }
    Ok(())
}

fn search_git_relative_files(
    root: &Path,
    query: &str,
    limit: usize,
) -> Result<FileSearchResult, String> {
    let mut state = FileSearchState {
        heap: BinaryHeap::new(),
        scanned: 0,
        truncated: false,
        limit,
        max_candidates: file_search_max_candidates(),
        deadline: Instant::now() + file_search_timeout(),
    };
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(map_io_error)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "git ls-files missing stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "git ls-files missing stderr".to_string())?;
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let path = line.map_err(map_io_error)?;
        if path.is_empty() {
            continue;
        }
        state.scanned += 1;
        if state.scanned > state.max_candidates || Instant::now() > state.deadline {
            state.truncated = true;
            let _ = child.kill();
            break;
        }
        let score = file_search_score(&path, query);
        if score < 0 {
            continue;
        }
        push_file_search_match(&mut state.heap, &path, score, state.limit);
    }
    let status = child.wait().map_err(map_io_error)?;
    let mut stderr_text = String::new();
    BufReader::new(stderr)
        .read_to_string(&mut stderr_text)
        .map_err(map_io_error)?;
    if state.truncated {
        return Ok(finish_file_search(state, "git"));
    }
    if !status.success() {
        let err = stderr_text.trim();
        return Err(if err.is_empty() {
            format!(
                "git ls-files failed with code {}",
                status.code().unwrap_or_default()
            )
        } else {
            err.to_string()
        });
    }
    Ok(finish_file_search(state, "git"))
}

fn push_file_search_match(
    heap: &mut BinaryHeap<Reverse<(i64, String)>>,
    path: &str,
    score: i64,
    limit: usize,
) {
    let item = Reverse((score, path.to_string()));
    if heap.len() < limit {
        heap.push(item);
        return;
    }
    if heap.peek().map(|current| item > *current).unwrap_or(true) {
        let _ = heap.pop();
        heap.push(item);
    }
}

fn finish_file_search(state: FileSearchState, mode: &str) -> FileSearchResult {
    let mut matches = state
        .heap
        .into_iter()
        .map(|Reverse((score, path))| ApiFileSearchMatch { path, score })
        .collect::<Vec<_>>();
    matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    FileSearchResult {
        mode: mode.to_string(),
        matches,
        scanned: if state.truncated {
            state.scanned.saturating_sub(1)
        } else {
            state.scanned
        },
        truncated: state.truncated,
    }
}

fn file_search_score(candidate: &str, query: &str) -> i64 {
    let raw = query.trim().to_ascii_lowercase();
    if raw.is_empty() {
        return 0;
    }
    let lower = candidate.to_ascii_lowercase();
    if lower == raw {
        return 12000;
    }
    let base = Path::new(candidate)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(candidate)
        .to_ascii_lowercase();
    if base == raw {
        return 10000;
    }
    let mut total = 0_i64;
    for token in raw.split_whitespace().filter(|part| !part.is_empty()) {
        if let Some(exact_idx) = lower.find(token) {
            let prev = if exact_idx > 0 {
                lower.as_bytes()[exact_idx - 1] as char
            } else {
                '\0'
            };
            let boundary_bonus = if exact_idx == 0 || is_file_search_boundary(prev) {
                24
            } else {
                0
            };
            let base_idx = base.find(token);
            total += 240 - (exact_idx as i64) * 2
                + boundary_bonus
                + base_idx.map(|idx| 44 - idx as i64).unwrap_or(0);
            continue;
        }
        let mut search_start = 0_usize;
        let mut first: Option<usize> = None;
        let mut last: Option<usize> = None;
        let mut consecutive = 0_i64;
        let mut boundaries = 0_i64;
        for ch in token.chars() {
            let found = lower[search_start..].find(ch).map(|idx| idx + search_start);
            let Some(found_idx) = found else {
                return -1;
            };
            if first.is_none() {
                first = Some(found_idx);
            }
            if let Some(last_idx) = last {
                if found_idx == last_idx + 1 {
                    consecutive += 1;
                }
            }
            if found_idx == 0 || is_file_search_boundary(lower.as_bytes()[found_idx - 1] as char) {
                boundaries += 1;
            }
            last = Some(found_idx);
            search_start = found_idx + ch.len_utf8();
        }
        let first_idx = first.unwrap_or(0);
        let last_idx = last.unwrap_or(first_idx);
        let span = last_idx.saturating_sub(first_idx) + 1;
        total += 120 - first_idx as i64 - ((span.saturating_sub(token.len())) as i64 * 4)
            + consecutive * 10
            + boundaries * 8;
    }
    total
}

fn is_file_search_boundary(ch: char) -> bool {
    matches!(ch, '/' | '.' | '_' | '-')
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

fn resolve_relative_under(base: &Path, raw_path: &str) -> Result<PathBuf, String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("path required".to_string());
    }
    if trimmed.contains('\0') {
        return Err("invalid path".to_string());
    }
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        return Err("path must be relative".to_string());
    }
    let resolved_base = fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
    let mut resolved = resolved_base.clone();
    let mut depth = 0usize;
    for component in candidate.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => {
                resolved.push(segment);
                depth += 1;
            }
            Component::ParentDir => {
                if depth == 0 {
                    return Err("path escapes session cwd".to_string());
                }
                resolved.pop();
                depth = depth.saturating_sub(1);
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("path must be relative".to_string());
            }
        }
    }
    Ok(resolved)
}

fn resolve_git_path(cwd: &Path, raw_path: &str) -> Result<(PathBuf, PathBuf, String), String> {
    let repo_root_raw = run_git_capture(
        cwd,
        &["rev-parse", "--show-toplevel"],
        default_git_diff_timeout(),
        64 * 1024,
    )?;
    let repo_root_candidate = PathBuf::from(repo_root_raw.trim());
    let repo_root = fs::canonicalize(&repo_root_candidate).unwrap_or(repo_root_candidate);
    let target = resolve_session_path(&cwd.display().to_string(), raw_path)?;
    let rel_path = target
        .strip_prefix(&repo_root)
        .map_err(|_| "path is outside git repo".to_string())?;
    let rel = if rel_path.as_os_str().is_empty() {
        ".".to_string()
    } else {
        rel_path.to_string_lossy().replace('\\', "/")
    };
    Ok((target, repo_root, rel))
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
    let mut file = fs::File::open(path).map_err(map_io_error)?;
    let mut buf = vec![0_u8; limit];
    let read = file.read(&mut buf).map_err(map_io_error)?;
    buf.truncate(read);
    Ok(buf)
}

fn read_text_file_strict(path: &Path, max_bytes: u64) -> Result<(String, u64), String> {
    let metadata = fs::metadata(path).map_err(map_io_error)?;
    let size = metadata.len();
    if size > max_bytes {
        return Err(format!("file too large (max {max_bytes} bytes)"));
    }
    let raw = fs::read(path).map_err(map_io_error)?;
    if raw.contains(&0) {
        return Err("binary file not supported".to_string());
    }
    Ok((String::from_utf8_lossy(&raw).into_owned(), size))
}

fn read_text_file_for_write(path: &Path, max_bytes: u64) -> Result<(String, u64, String), String> {
    let metadata = fs::metadata(path).map_err(map_io_error)?;
    if !metadata.is_file() {
        return Err("path is not a file".to_string());
    }
    let size = metadata.len();
    if size > max_bytes {
        return Err(format!("file too large (max {max_bytes} bytes)"));
    }
    let raw = fs::read(path).map_err(map_io_error)?;
    if raw.contains(&0) {
        return Err("binary file not supported".to_string());
    }
    let text = std::str::from_utf8(&raw)
        .map_err(|_| "file is not editable as utf-8 text".to_string())?
        .to_string();
    Ok((text, size, content_version(&raw)))
}

fn map_file_write_message(message: String) -> FileWriteError {
    match message.as_str() {
        "file not found" | "parent directory not found" => FileWriteError::NotFound(message),
        "permission denied" => FileWriteError::Forbidden(message),
        "path required"
        | "invalid path"
        | "path must be relative"
        | "path escapes session cwd"
        | "text must be a string"
        | "version required"
        | "binary file not supported"
        | "file is not editable as utf-8 text"
        | "path is not a file"
        | "parent path is not a directory" => FileWriteError::BadRequest(message),
        _ if message.starts_with("file too large") => FileWriteError::BadRequest(message),
        _ => FileWriteError::Internal(message),
    }
}

fn update_session_file_history(
    config: &RuntimeConfig,
    session_id: &str,
    path: &str,
) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let files_path = config.app_dir.join("session_files.json");
    let mut files = read_string_array_map(&files_path)?;
    let key = format!("sid:{session_id}");
    let mut current = files.remove(&key).unwrap_or_default();
    if current.is_empty() {
        current = files.remove(session_id).unwrap_or_default();
    }
    current.retain(|item| item != trimmed);
    current.insert(0, trimmed.to_string());
    current.truncate(default_file_history_max());
    files.insert(key, current);
    write_string_array_map(&files_path, &files)
}

fn detect_file_kind(path: &Path, raw: &[u8]) -> (&'static str, Option<&'static str>) {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("svg"))
        .unwrap_or(false)
    {
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
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
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
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if TEXTUAL_EXTENSIONS.iter().any(|candidate| *candidate == ext) {
        return true;
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
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

fn provider_choice_for_settings(
    model_provider: Option<&str>,
    preferred_auth_method: Option<&str>,
) -> Option<String> {
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
        let obj = serde_json::from_str(trimmed)
            .map_err(|err| format!("parse {} at byte {start}: {err}", path.display()))?;
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
    single_chat_event(obj)
}

struct ExtractedChatBatch {
    events: Vec<Value>,
    meta: Value,
    turn_start: bool,
    turn_end: bool,
    turn_aborted: bool,
    diag: Value,
}

fn extract_chat_events(objs: &[Value]) -> ExtractedChatBatch {
    let mut events = Vec::new();
    let mut total_thinking = 0_i64;
    let mut total_tools = 0_i64;
    let mut total_system = 0_i64;
    let mut turn_start = false;
    let mut turn_end = false;
    let mut turn_aborted = false;
    let mut tool_names = HashSet::new();
    let mut last_tool: Option<String> = None;
    let mut known_tool_names: HashMap<String, String> = HashMap::new();
    let mut pending_ask_user_calls: HashMap<String, Value> = HashMap::new();

    for obj in objs {
        match obj.get("type").and_then(Value::as_str) {
            Some("message") => {
                if let Some(user_text) = pi_user_text_value(obj) {
                    turn_start = true;
                    events.push(json_text_event("user", &user_text, event_ts(obj), None));
                    continue;
                }

                let assistant_text = pi_assistant_text_value(obj);
                let tool_count = pi_assistant_tool_use_count(obj) as i64;
                let thinking_count = pi_assistant_thinking_count(obj) as i64;
                total_thinking += thinking_count;
                if tool_count > 0 {
                    total_tools += tool_count;
                    tool_names.insert("pi_tool".to_string());
                    last_tool = Some("pi_tool".to_string());
                }

                let payload = match obj.get("message").and_then(Value::as_object) {
                    Some(payload) => payload,
                    None => continue,
                };
                let ets = event_ts(obj);
                match payload.get("role").and_then(Value::as_str) {
                    Some("assistant") => {
                        if let Some(content) = payload.get("content").and_then(Value::as_array) {
                            for item in content {
                                let Some(item) = item.as_object() else {
                                    continue;
                                };
                                if item.get("type").and_then(Value::as_str) != Some("toolCall") {
                                    continue;
                                }
                                let name = non_empty_string(item.get("name"))
                                    .unwrap_or_else(|| "tool".to_string());
                                let call_id = non_empty_string(item.get("id"));
                                let args = coerce_tool_arguments(item.get("arguments"));
                                tool_names.insert("pi_tool".to_string());
                                if let Some(call_id) = call_id.as_ref() {
                                    known_tool_names.insert(call_id.clone(), name.clone());
                                }
                                let Some(ts) = ets else {
                                    continue;
                                };
                                if is_ask_user_tool(&name) {
                                    let event =
                                        ask_user_event(&args, call_id.as_deref(), ts, false);
                                    if let Some(call_id) = call_id.as_ref() {
                                        pending_ask_user_calls
                                            .insert(call_id.clone(), event.clone());
                                    }
                                    events.push(event);
                                    continue;
                                }
                                if let Some(event) =
                                    extension_event_from_tool(&name, &args, call_id.as_deref(), ts)
                                {
                                    events.push(event);
                                    continue;
                                }
                                events.push(tool_event(
                                    &name,
                                    call_id.as_deref(),
                                    tool_call_summary(&name, &args),
                                    ts,
                                ));
                            }
                        }
                    }
                    Some("toolResult") => {
                        total_tools += 1;
                        tool_names.insert("pi_tool".to_string());
                        last_tool = Some("pi_tool".to_string());
                        let call_id = non_empty_string(payload.get("toolCallId"));
                        let name = non_empty_string(payload.get("toolName"))
                            .or_else(|| {
                                call_id
                                    .as_ref()
                                    .and_then(|id| known_tool_names.get(id).cloned())
                            })
                            .unwrap_or_else(|| "tool".to_string());
                        let details = payload.get("details").and_then(Value::as_object);
                        let text = tool_result_text(payload);
                        if let Some(ts) = ets {
                            if let Some(event) = extension_event_from_tool_result(
                                &name,
                                payload,
                                call_id.as_deref(),
                                ts,
                            ) {
                                events.push(event);
                                continue;
                            }
                            if is_ask_user_tool(&name)
                                || call_id
                                    .as_ref()
                                    .map(|id| pending_ask_user_calls.contains_key(id))
                                    .unwrap_or(false)
                            {
                                let base = ask_user_base_event(
                                    call_id.as_deref(),
                                    ts,
                                    &pending_ask_user_calls,
                                );
                                events.push(resolve_ask_user_event(
                                    base,
                                    call_id.as_deref(),
                                    ts,
                                    details,
                                    text.as_deref(),
                                ));
                            } else if text.is_some()
                                || payload.get("isError").and_then(Value::as_bool) == Some(true)
                            {
                                events.push(tool_result_event(
                                    &name,
                                    call_id.as_deref(),
                                    text,
                                    payload.get("isError").and_then(Value::as_bool) == Some(true),
                                    ts,
                                ));
                            }
                        }
                    }
                    _ => {}
                }

                if let Some(text) = assistant_text {
                    let message_class = if pi_assistant_is_final_turn_end(obj) {
                        turn_end = true;
                        Some("final_response")
                    } else {
                        Some("narration")
                    };
                    events.push(json_text_event("assistant", &text, ets, message_class));
                }
            }
            Some("event_msg") => {
                let Some(payload) = obj.get("payload").and_then(Value::as_object) else {
                    continue;
                };
                match payload.get("type").and_then(Value::as_str) {
                    Some("user_message") => {
                        if let Some(message) = payload.get("message").and_then(Value::as_str) {
                            turn_start = true;
                            events.push(json_text_event("user", message, event_ts(obj), None));
                        }
                    }
                    Some("agent_reasoning") => total_thinking += 1,
                    Some("turn_aborted") => turn_aborted = true,
                    Some("task_complete") | Some("turn_complete") => turn_end = true,
                    Some("token_count") => {}
                    _ => {}
                }
            }
            Some("response_item") => {
                let Some(payload) = obj.get("payload").and_then(Value::as_object) else {
                    continue;
                };
                let pt = payload.get("type").and_then(Value::as_str);
                if pt == Some("message") {
                    match payload.get("role").and_then(Value::as_str) {
                        Some("developer") | Some("system") => {
                            total_system += 1;
                            continue;
                        }
                        Some("assistant") => {
                            let Some(text) = payload.get("content").and_then(output_text) else {
                                continue;
                            };
                            let message_class = if payload.get("phase").and_then(Value::as_str)
                                == Some("final_answer")
                                || payload.get("end_turn").and_then(Value::as_bool) == Some(true)
                            {
                                Some("final_response")
                            } else {
                                Some("narration")
                            };
                            events.push(json_text_event(
                                "assistant",
                                &text,
                                event_ts(obj),
                                message_class,
                            ));
                            continue;
                        }
                        _ => {}
                    }
                }

                match pt {
                    Some("reasoning") => total_thinking += 1,
                    Some("function_call") => {
                        let name = non_empty_string(payload.get("name"))
                            .unwrap_or_else(|| "tool".to_string());
                        let call_id = non_empty_string(payload.get("call_id"));
                        let args = coerce_tool_arguments(payload.get("arguments"));
                        tool_names.insert(name.clone());
                        last_tool = Some(name.clone());
                        total_tools += 1;
                        if let Some(call_id) = call_id.as_ref() {
                            known_tool_names.insert(call_id.clone(), name.clone());
                        }
                        if let Some(ts) = event_ts(obj) {
                            if is_ask_user_tool(&name) {
                                let event = ask_user_event(&args, call_id.as_deref(), ts, false);
                                if let Some(call_id) = call_id.as_ref() {
                                    pending_ask_user_calls.insert(call_id.clone(), event.clone());
                                }
                                events.push(event);
                            } else if let Some(event) =
                                extension_event_from_tool(&name, &args, call_id.as_deref(), ts)
                            {
                                events.push(event);
                            } else {
                                events.push(tool_event(
                                    &name,
                                    call_id.as_deref(),
                                    tool_call_summary(&name, &args),
                                    ts,
                                ));
                            }
                        }
                    }
                    Some("custom_tool_call")
                    | Some("web_search_call")
                    | Some("local_shell_call") => {
                        total_tools += 1;
                        let name = match pt {
                            Some("web_search_call") => "web_search".to_string(),
                            Some("local_shell_call") => "local_shell".to_string(),
                            _ => non_empty_string(payload.get("name"))
                                .unwrap_or_else(|| "tool".to_string()),
                        };
                        let call_id = non_empty_string(payload.get("call_id"));
                        if let Some(call_id) = call_id.as_ref() {
                            known_tool_names.insert(call_id.clone(), name.clone());
                        }
                        tool_names.insert(name.clone());
                        last_tool = Some(name.clone());
                        if let Some(ts) = event_ts(obj) {
                            let mut args = coerce_tool_arguments(payload.get("arguments"));
                            if args.is_empty() {
                                args = payload.clone();
                            }
                            if let Some(event) =
                                extension_event_from_tool(&name, &args, call_id.as_deref(), ts)
                            {
                                events.push(event);
                            } else {
                                events.push(tool_event(
                                    &name,
                                    call_id.as_deref(),
                                    tool_call_summary(&name, &args),
                                    ts,
                                ));
                            }
                        }
                    }
                    Some("function_call_output") | Some("custom_tool_call_output") => {
                        total_tools += 1;
                        let call_id = non_empty_string(payload.get("call_id"));
                        let name = non_empty_string(payload.get("name"))
                            .or_else(|| {
                                call_id
                                    .as_ref()
                                    .and_then(|id| known_tool_names.get(id).cloned())
                            })
                            .unwrap_or_else(|| "tool".to_string());
                        tool_names.insert(name.clone());
                        last_tool = Some(name.clone());
                        let details = payload.get("details").and_then(Value::as_object);
                        let text = tool_result_text(payload);
                        if let Some(ts) = event_ts(obj) {
                            if let Some(event) = extension_event_from_tool_result(
                                &name,
                                payload,
                                call_id.as_deref(),
                                ts,
                            ) {
                                events.push(event);
                            } else if is_ask_user_tool(&name)
                                || call_id
                                    .as_ref()
                                    .map(|id| pending_ask_user_calls.contains_key(id))
                                    .unwrap_or(false)
                            {
                                let base = ask_user_base_event(
                                    call_id.as_deref(),
                                    ts,
                                    &pending_ask_user_calls,
                                );
                                events.push(resolve_ask_user_event(
                                    base,
                                    call_id.as_deref(),
                                    ts,
                                    details,
                                    text.as_deref(),
                                ));
                            } else if text.is_some()
                                || payload.get("is_error").and_then(Value::as_bool) == Some(true)
                            {
                                events.push(tool_result_event(
                                    &name,
                                    call_id.as_deref(),
                                    text,
                                    payload.get("is_error").and_then(Value::as_bool) == Some(true),
                                    ts,
                                ));
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let mut tool_names = tool_names.into_iter().collect::<Vec<_>>();
    tool_names.sort();
    ExtractedChatBatch {
        events,
        meta: json!({
            "thinking": total_thinking,
            "tool": total_tools,
            "system": total_system,
        }),
        turn_start,
        turn_end,
        turn_aborted,
        diag: json!({
            "tool_names": tool_names,
            "last_tool": last_tool,
        }),
    }
}

fn single_chat_event(obj: &Value) -> Option<Value> {
    match obj.get("type").and_then(Value::as_str)? {
        "message" => pi_single_chat_event(obj),
        "event_msg" => event_msg_chat_event(obj),
        "response_item" => response_item_chat_event(obj),
        _ => None,
    }
}

fn pi_single_chat_event(obj: &Value) -> Option<Value> {
    if let Some(user_text) = pi_user_text_value(obj) {
        return Some(json_text_event("user", &user_text, event_ts(obj), None));
    }
    if let Some(assistant_text) = pi_assistant_text_value(obj) {
        let class = if pi_assistant_is_final_turn_end(obj) {
            Some("final_response")
        } else {
            Some("narration")
        };
        return Some(json_text_event(
            "assistant",
            &assistant_text,
            event_ts(obj),
            class,
        ));
    }
    let payload = obj.get("message")?.as_object()?;
    let role = payload.get("role")?.as_str()?;
    let ets = event_ts(obj);
    match role {
        "assistant" => {
            let content = payload.get("content")?.as_array()?;
            for item in content {
                let Some(item) = item.as_object() else {
                    continue;
                };
                if item.get("type").and_then(Value::as_str) != Some("toolCall") {
                    continue;
                }
                let name = non_empty_string(item.get("name")).unwrap_or_else(|| "tool".to_string());
                let call_id = non_empty_string(item.get("id"));
                let args = coerce_tool_arguments(item.get("arguments"));
                if is_ask_user_tool(&name) {
                    return ets.map(|ts| ask_user_event(&args, call_id.as_deref(), ts, false));
                }
                if let Some(ts) = ets {
                    if let Some(event) =
                        extension_event_from_tool(&name, &args, call_id.as_deref(), ts)
                    {
                        return Some(event);
                    }
                    return Some(tool_event(
                        &name,
                        call_id.as_deref(),
                        tool_call_summary(&name, &args),
                        ts,
                    ));
                }
            }
            None
        }
        "toolResult" => {
            let call_id = non_empty_string(payload.get("toolCallId"));
            let name =
                non_empty_string(payload.get("toolName")).unwrap_or_else(|| "tool".to_string());
            let text = tool_result_text(payload);
            if let Some(ts) = ets {
                if let Some(event) =
                    extension_event_from_tool_result(&name, payload, call_id.as_deref(), ts)
                {
                    return Some(event);
                }
                if is_ask_user_tool(&name) {
                    return Some(resolve_ask_user_event(
                        ask_user_event(&Map::new(), call_id.as_deref(), ts, true),
                        call_id.as_deref(),
                        ts,
                        payload.get("details").and_then(Value::as_object),
                        text.as_deref(),
                    ));
                }
                if text.is_some() || payload.get("isError").and_then(Value::as_bool) == Some(true) {
                    return Some(tool_result_event(
                        &name,
                        call_id.as_deref(),
                        text,
                        payload.get("isError").and_then(Value::as_bool) == Some(true),
                        ts,
                    ));
                }
            }
            None
        }
        _ => None,
    }
}

fn event_msg_chat_event(obj: &Value) -> Option<Value> {
    let payload = obj.get("payload")?.as_object()?;
    match payload.get("type")?.as_str()? {
        "user_message" => Some(json_text_event(
            "user",
            payload.get("message")?.as_str()?,
            event_ts(obj),
            None,
        )),
        "agent_message" => {
            let class = if payload.get("phase").and_then(Value::as_str) == Some("final_answer") {
                Some("final_response")
            } else {
                Some("narration")
            };
            Some(json_text_event(
                "assistant",
                payload.get("message")?.as_str()?,
                event_ts(obj),
                class,
            ))
        }
        _ => None,
    }
}

fn response_item_chat_event(obj: &Value) -> Option<Value> {
    let payload = obj.get("payload")?.as_object()?;
    let ets = event_ts(obj);
    match payload.get("type")?.as_str()? {
        "function_call" => {
            let name = non_empty_string(payload.get("name")).unwrap_or_else(|| "tool".to_string());
            let call_id = non_empty_string(payload.get("call_id"));
            let args = coerce_tool_arguments(payload.get("arguments"));
            if is_ask_user_tool(&name) {
                return ets.map(|ts| ask_user_event(&args, call_id.as_deref(), ts, false));
            }
            if let Some(ts) = ets {
                if let Some(event) = extension_event_from_tool(&name, &args, call_id.as_deref(), ts)
                {
                    return Some(event);
                }
                return Some(tool_event(
                    &name,
                    call_id.as_deref(),
                    tool_call_summary(&name, &args),
                    ts,
                ));
            }
            None
        }
        "custom_tool_call" | "web_search_call" | "local_shell_call" => {
            let name = match payload.get("type").and_then(Value::as_str) {
                Some("web_search_call") => "web_search".to_string(),
                Some("local_shell_call") => "local_shell".to_string(),
                _ => non_empty_string(payload.get("name")).unwrap_or_else(|| "tool".to_string()),
            };
            let call_id = non_empty_string(payload.get("call_id"));
            let mut args = coerce_tool_arguments(payload.get("arguments"));
            if args.is_empty() {
                args = payload.clone();
            }
            if let Some(ts) = ets {
                if let Some(event) = extension_event_from_tool(&name, &args, call_id.as_deref(), ts)
                {
                    return Some(event);
                }
                return Some(tool_event(
                    &name,
                    call_id.as_deref(),
                    tool_call_summary(&name, &args),
                    ts,
                ));
            }
            None
        }
        "function_call_output" | "custom_tool_call_output" => {
            let call_id = non_empty_string(payload.get("call_id"));
            let name = non_empty_string(payload.get("name")).unwrap_or_else(|| "tool".to_string());
            let text = tool_result_text(payload);
            if let Some(ts) = ets {
                if let Some(event) =
                    extension_event_from_tool_result(&name, payload, call_id.as_deref(), ts)
                {
                    return Some(event);
                }
                if is_ask_user_tool(&name) {
                    return Some(resolve_ask_user_event(
                        ask_user_event(&Map::new(), call_id.as_deref(), ts, true),
                        call_id.as_deref(),
                        ts,
                        payload.get("details").and_then(Value::as_object),
                        text.as_deref(),
                    ));
                }
                if text.is_some() || payload.get("is_error").and_then(Value::as_bool) == Some(true)
                {
                    return Some(tool_result_event(
                        &name,
                        call_id.as_deref(),
                        text,
                        payload.get("is_error").and_then(Value::as_bool) == Some(true),
                        ts,
                    ));
                }
            }
            None
        }
        "message" => {
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
            Some(json_text_event("assistant", &text, ets, class))
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

fn tool_event(name: &str, call_id: Option<&str>, text: Option<String>, ts: f64) -> Value {
    let mut event = json!({"type": "tool", "name": name, "ts": ts});
    if let Some(call_id) = call_id {
        event["tool_call_id"] = json!(call_id);
    }
    if let Some(text) = text {
        event["text"] = json!(text);
    }
    event
}

fn tool_result_event(
    name: &str,
    call_id: Option<&str>,
    text: Option<String>,
    is_error: bool,
    ts: f64,
) -> Value {
    let mut event = json!({"type": "tool_result", "name": name, "ts": ts});
    if let Some(call_id) = call_id {
        event["tool_call_id"] = json!(call_id);
    }
    if let Some(text) = text {
        event["text"] = json!(text);
    }
    if is_error {
        event["is_error"] = json!(true);
    }
    event
}

fn is_ask_user_tool(name: &str) -> bool {
    ASK_USER_TOOL_NAMES.contains(&name)
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    let value = value?.as_str()?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn coerce_tool_arguments(value: Option<&Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(map)) => map.clone(),
        Some(Value::String(raw)) => serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default(),
        _ => Map::new(),
    }
}

fn tool_text_from_content(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(text.to_string());
        }
    }
    let parts = value.as_array()?;
    let text = parts
        .iter()
        .filter_map(|item| {
            let item = item.as_object()?;
            let kind = item.get("type").and_then(Value::as_str);
            if matches!(
                kind,
                Some("text") | Some("output_text") | Some("input_text")
            ) {
                item.get("text").and_then(Value::as_str)
            } else {
                None
            }
        })
        .collect::<String>();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn tool_result_text(payload: &Map<String, Value>) -> Option<String> {
    for key in ["content", "output", "text", "result"] {
        let Some(value) = payload.get(key) else {
            continue;
        };
        if let Some(text) = tool_text_from_content(value) {
            return Some(text);
        }
        if matches!(value, Value::Object(map) if !map.is_empty())
            || matches!(value, Value::Array(items) if !items.is_empty())
        {
            return serde_json::to_string(value).ok();
        }
    }
    None
}

fn tool_call_summary(name: &str, args: &Map<String, Value>) -> Option<String> {
    if is_ask_user_tool(name) {
        if let Some(question) = non_empty_string(args.get("question")) {
            return Some(question);
        }
        if let Some(questions) = args.get("questions").and_then(Value::as_array) {
            for item in questions {
                let Some(item) = item.as_object() else {
                    continue;
                };
                if let Some(question) = non_empty_string(item.get("question")) {
                    return Some(question);
                }
            }
        }
        return None;
    }
    for key in [
        "cmd",
        "command",
        "query",
        "prompt",
        "path",
        "file_path",
        "url",
        "subject",
    ] {
        if let Some(value) = non_empty_string(args.get(key)) {
            return Some(value);
        }
    }
    None
}

fn normalize_bool_arg(args: &Map<String, Value>, keys: &[&str], default: bool) -> bool {
    keys.iter()
        .find_map(|key| args.get(*key).and_then(Value::as_bool))
        .unwrap_or(default)
}

fn normalize_ask_user_questions(value: Option<&Value>) -> Vec<Value> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let item = item.as_object()?;
            let question = item.get("question").and_then(Value::as_str)?.trim().to_string();
            if question.is_empty() {
                return None;
            }
            let header = item.get("header").and_then(Value::as_str).unwrap_or("").to_string();
            let options = item
                .get("options")
                .and_then(Value::as_array)
                .map(|options| options.iter().filter_map(|option| option.as_str().map(|s| s.to_string())).collect::<Vec<_>>())
                .unwrap_or_default();
            Some(json!({
                "header": header,
                "question": question,
                "options": options,
                "multiSelect": normalize_bool_arg(item, &["allow_multiple", "allowMultiple", "multiSelect"], false),
            }))
        })
        .collect()
}

fn ask_user_event(
    args: &Map<String, Value>,
    call_id: Option<&str>,
    ts: f64,
    resolved: bool,
) -> Value {
    let mut question = args
        .get("question")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut context = args
        .get("context")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut options = args
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(|option| option.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut allow_freeform = normalize_bool_arg(args, &["allow_freeform", "allowFreeform"], true);
    let mut allow_multiple = normalize_bool_arg(args, &["allow_multiple", "allowMultiple"], false);
    let timeout_ms = args
        .get("timeout_ms")
        .or_else(|| args.get("timeoutMs"))
        .or_else(|| args.get("timeout"))
        .and_then(Value::as_i64);
    let questions = normalize_ask_user_questions(args.get("questions"));
    let mut header = None;
    if let Some(first) = questions.first().and_then(Value::as_object) {
        if let Some(value) = first.get("question").and_then(Value::as_str) {
            question = value.to_string();
        }
        if let Some(value) = first.get("header").and_then(Value::as_str) {
            header = Some(value.to_string());
            if context.is_empty() && !value.is_empty() {
                context = value.to_string();
            }
        }
        if let Some(values) = first.get("options").and_then(Value::as_array) {
            options = values
                .iter()
                .filter_map(|option| option.as_str().map(|s| s.to_string()))
                .collect();
        }
        allow_freeform = first
            .get("allowFreeform")
            .and_then(Value::as_bool)
            .unwrap_or(allow_freeform);
        allow_multiple = first
            .get("multiSelect")
            .and_then(Value::as_bool)
            .unwrap_or(allow_multiple);
    }
    let mut event = json!({
        "type": "ask_user",
        "question": question,
        "context": context,
        "options": options,
        "allow_freeform": allow_freeform,
        "allow_multiple": allow_multiple,
        "timeout_ms": timeout_ms,
        "resolved": resolved,
        "ts": ts,
    });
    if let Some(call_id) = call_id {
        event["tool_call_id"] = json!(call_id);
    }
    if let Some(header) = header {
        event["header"] = json!(header);
    }
    if !questions.is_empty() {
        event["questions"] = Value::Array(questions);
    }
    event
}

fn ask_user_base_event(call_id: Option<&str>, ts: f64, pending: &HashMap<String, Value>) -> Value {
    call_id
        .and_then(|call_id| pending.get(call_id).cloned())
        .unwrap_or_else(|| ask_user_event(&Map::new(), call_id, ts, true))
}

fn normalize_ask_user_answer(value: Option<&Value>, allow_multiple: bool) -> Option<Value> {
    if let Some(answer) = value.and_then(Value::as_str) {
        return Some(json!(answer));
    }
    if allow_multiple {
        if let Some(values) = value.and_then(Value::as_array) {
            let items = values
                .iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>();
            if !items.is_empty() {
                return Some(json!(items));
            }
        }
    }
    None
}

fn normalize_ask_user_result(
    details: Option<&Map<String, Value>>,
    allow_multiple: bool,
    question: &str,
) -> (Option<Value>, bool) {
    let Some(details) = details else {
        return (None, false);
    };
    if let Some(answers) = details.get("answers").and_then(Value::as_object) {
        if !question.is_empty() {
            if let Some(answer) = normalize_ask_user_answer(answers.get(question), allow_multiple) {
                return (Some(answer), false);
            }
        }
    }
    let was_custom = details
        .get("wasCustom")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if let Some(answer) = normalize_ask_user_answer(details.get("answer"), allow_multiple) {
        return (Some(answer), was_custom);
    }
    if let Some(response) = details.get("response").and_then(Value::as_object) {
        let kind = response.get("kind").and_then(Value::as_str).unwrap_or("");
        if let Some(selections) = response.get("selections").and_then(Value::as_array) {
            let normalized = selections
                .iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>();
            if !normalized.is_empty() {
                if allow_multiple || normalized.len() > 1 {
                    return (Some(json!(normalized)), was_custom || kind == "custom");
                }
                return (Some(json!(normalized[0])), was_custom || kind == "custom");
            }
        }
        if let Some(value) = response.get("value").and_then(Value::as_str) {
            if !value.is_empty() {
                return (Some(json!(value)), was_custom || kind == "custom");
            }
        }
        if let Some(comment) = response.get("comment").and_then(Value::as_str) {
            if !comment.trim().is_empty() {
                return (Some(json!(comment.trim())), true);
            }
        }
    }
    (None, was_custom)
}

fn resolve_ask_user_event(
    mut event: Value,
    call_id: Option<&str>,
    ts: f64,
    details: Option<&Map<String, Value>>,
    _content_text: Option<&str>,
) -> Value {
    event["resolved"] = json!(true);
    event["ts"] = json!(ts);
    if let Some(call_id) = call_id {
        event["tool_call_id"] = json!(call_id);
    }
    let allow_multiple = event
        .get("allow_multiple")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let question = event.get("question").and_then(Value::as_str).unwrap_or("");
    let (answer, was_custom) = normalize_ask_user_result(details, allow_multiple, question);
    if let Some(answer) = answer {
        event["answer"] = answer;
    }
    if let Some(cancelled) = details
        .and_then(|details| details.get("cancelled"))
        .and_then(Value::as_bool)
    {
        event["cancelled"] = json!(cancelled);
    }
    event["was_custom"] = json!(was_custom);
    event
}

fn extension_item(value: &Value) -> Option<Value> {
    if let Some(label) = value
        .as_str()
        .map(str::trim)
        .filter(|label| !label.is_empty())
    {
        return Some(json!({ "label": label }));
    }
    let object = value.as_object()?;
    let label = non_empty_string(object.get("label"))
        .or_else(|| non_empty_string(object.get("step")))
        .or_else(|| non_empty_string(object.get("title")))?;
    let mut item = json!({ "label": label });
    if let Some(status) = non_empty_string(object.get("status")) {
        item["status"] = json!(status);
    }
    if let Some(detail) =
        non_empty_string(object.get("detail")).or_else(|| non_empty_string(object.get("summary")))
    {
        item["detail"] = json!(detail);
    }
    Some(item)
}

fn normalize_extension_items(value: Option<&Value>) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(extension_item).collect::<Vec<_>>())
        .unwrap_or_default()
}

fn extension_display_event(
    payload: &Map<String, Value>,
    call_id: Option<&str>,
    ts: f64,
    default_source: &str,
    default_title: &str,
) -> Value {
    let progress = payload.get("progress").and_then(Value::as_object);
    let mut event = json!({
        "type": "extension",
        "extension_kind": non_empty_string(payload.get("kind")).unwrap_or_else(|| "status".to_string()),
        "source": non_empty_string(payload.get("source")).unwrap_or_else(|| default_source.to_string()),
        "title": non_empty_string(payload.get("title")).unwrap_or_else(|| default_title.to_string()),
        "ts": ts,
    });
    if let Some(call_id) = call_id {
        event["tool_call_id"] = json!(call_id);
    }
    for (source_key, event_key) in [
        ("status", "status"),
        ("summary", "summary"),
        ("text", "text"),
    ] {
        if let Some(value) = non_empty_string(payload.get(source_key)) {
            event[event_key] = json!(value);
        }
    }
    if let Some(current) = progress
        .and_then(|progress| progress.get("current"))
        .or_else(|| payload.get("progress_current"))
        .and_then(number_json)
    {
        event["progress_current"] = current;
    }
    if let Some(total) = progress
        .and_then(|progress| progress.get("total"))
        .or_else(|| payload.get("progress_total"))
        .and_then(number_json)
    {
        event["progress_total"] = total;
    }
    if let Some(label) = progress
        .and_then(|progress| non_empty_string(progress.get("label")))
        .or_else(|| non_empty_string(payload.get("progress_label")))
    {
        event["progress_label"] = json!(label);
    }
    let items = normalize_extension_items(payload.get("items"));
    if !items.is_empty() {
        event["items"] = Value::Array(items);
    }
    event
}

fn number_json(value: &Value) -> Option<Value> {
    match value {
        Value::Number(number) => Some(Value::Number(number.clone())),
        _ => None,
    }
}

fn plan_status(items: &[Value]) -> &'static str {
    let statuses = items
        .iter()
        .filter_map(|item| item.get("status").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if statuses.iter().any(|status| *status == "in_progress") {
        "running"
    } else if !statuses.is_empty() && statuses.iter().all(|status| *status == "completed") {
        "completed"
    } else {
        "pending"
    }
}

fn update_plan_event(args: &Map<String, Value>, call_id: Option<&str>, ts: f64) -> Option<Value> {
    let items = normalize_extension_items(args.get("plan"));
    if items.is_empty() {
        return None;
    }
    let completed = items
        .iter()
        .filter(|item| item.get("status").and_then(Value::as_str) == Some("completed"))
        .count();
    Some(extension_display_event(
        &json!({
            "kind": "progress",
            "source": "codex",
            "title": "Todo",
            "status": plan_status(&items),
            "summary": format!("{completed}/{} completed", items.len()),
            "progress": { "current": completed, "total": items.len(), "label": "items" },
            "items": items,
        })
        .as_object()
        .cloned()
        .unwrap_or_default(),
        call_id,
        ts,
        "codex",
        "Todo",
    ))
}

fn extension_event_from_tool(
    name: &str,
    args: &Map<String, Value>,
    call_id: Option<&str>,
    ts: f64,
) -> Option<Value> {
    if name == "update_plan" {
        return update_plan_event(args, call_id, ts);
    }
    let payload = if EXTENSION_DISPLAY_TOOL_NAMES.contains(&name) {
        Some(args)
    } else {
        args.get(EXTENSION_DISPLAY_KEY).and_then(Value::as_object)
    }?;
    Some(extension_display_event(payload, call_id, ts, name, name))
}

fn extension_payload_from_result<'a>(
    name: &str,
    payload: &'a Map<String, Value>,
) -> Option<&'a Map<String, Value>> {
    for key in [
        EXTENSION_DISPLAY_KEY,
        "structuredContent",
        "structured_content",
        "details",
        "result",
        "output",
    ] {
        let Some(candidate) = payload.get(key).and_then(Value::as_object) else {
            continue;
        };
        if let Some(nested) = candidate
            .get(EXTENSION_DISPLAY_KEY)
            .and_then(Value::as_object)
        {
            return Some(nested);
        }
        if EXTENSION_DISPLAY_TOOL_NAMES.contains(&name) {
            return Some(candidate);
        }
    }
    None
}

fn extension_event_from_tool_result(
    name: &str,
    payload: &Map<String, Value>,
    call_id: Option<&str>,
    ts: f64,
) -> Option<Value> {
    let extension_payload = extension_payload_from_result(name, payload)?;
    Some(extension_display_event(
        extension_payload,
        call_id,
        ts,
        name,
        name,
    ))
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
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
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
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn parse_iso8601_to_epoch(ts: &str) -> Option<f64> {
    let raw = ts.trim();
    let (date, time_and_zone) = raw.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }

    let (time_part, offset_seconds) = if let Some(time_part) = time_and_zone.strip_suffix('Z') {
        (time_part, 0_i32)
    } else if let Some((time_part, offset_part)) = time_and_zone.rsplit_once(['+', '-']) {
        let sign = if time_and_zone.as_bytes().get(time_part.len()) == Some(&b'+') {
            1_i32
        } else {
            -1_i32
        };
        let (offset_hour, offset_minute) = offset_part.split_once(':')?;
        let hours = offset_hour.parse::<i32>().ok()?;
        let minutes = offset_minute.parse::<i32>().ok()?;
        (time_part, sign * (hours * 3600 + minutes * 60))
    } else {
        return None;
    };

    let mut time_parts = time_part.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let second_part = time_parts.next()?;
    if time_parts.next().is_some() {
        return None;
    }
    let (second_raw, fractional_raw) = second_part.split_once('.').unwrap_or((second_part, ""));
    let second = second_raw.parse::<u32>().ok()?;
    let fractional = if fractional_raw.is_empty() {
        0.0
    } else {
        format!("0.{fractional_raw}").parse::<f64>().ok()?
    };

    let days = days_from_civil(year, month, day)?;
    Some(
        days as f64 * 86_400.0
            + hour as f64 * 3_600.0
            + minute as f64 * 60.0
            + second as f64
            + fractional
            - offset_seconds as f64,
    )
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let adjusted_year = year - i32::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let yoe = adjusted_year - era * 400;
    let month_prime = month as i32 + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_prime + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era as i64) * 146_097 + doe as i64 - 719_468)
}

fn event_ts(obj: &Value) -> Option<f64> {
    obj.get("ts")
        .and_then(Value::as_f64)
        .or_else(|| obj.get("timestamp").and_then(Value::as_f64))
        .or_else(|| {
            obj.get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_iso8601_to_epoch)
        })
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

fn read_voice_listener_records(path: &Path, now_ts: f64) -> Result<HashMap<String, f64>, String> {
    let Some(Value::Object(object)) = read_optional_value(path)? else {
        return Ok(HashMap::new());
    };
    let mut listeners = HashMap::new();
    for (client_id, value) in object {
        let client_id = client_id.trim();
        if client_id.is_empty() {
            continue;
        }
        let Some(seen_ts) = value.as_f64() else {
            continue;
        };
        if !seen_ts.is_finite() || (now_ts - seen_ts) > LISTENER_TTL_SECONDS {
            continue;
        }
        listeners.insert(client_id.to_string(), seen_ts);
    }
    Ok(listeners)
}

fn write_voice_listener_records(
    path: &Path,
    listeners: &HashMap<String, f64>,
) -> Result<(), String> {
    let mut keys = listeners.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    let mut object = serde_json::Map::new();
    for key in keys {
        let Some(seen_ts) = listeners.get(&key) else {
            continue;
        };
        object.insert(key, json!(seen_ts));
    }
    write_json_value(path, &Value::Object(object))
}

fn current_voice_listener_count(
    config: &RuntimeConfig,
    fallback_count: i64,
) -> Result<i64, String> {
    let path = voice_listeners_path(config);
    if !path.exists() {
        return Ok(fallback_count.max(0));
    }
    Ok(read_voice_listener_records(&path, epoch_now())?.len() as i64)
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

pub(crate) fn normalize_backend(value: Option<&str>) -> Result<String, String> {
    let raw = value.unwrap_or("codex").trim().to_ascii_lowercase();
    match raw.as_str() {
        "" => Ok("codex".to_string()),
        "codex" | "pi" => Ok(raw),
        _ => Err(format!("agent_backend must be codex or pi, got {raw:?}")),
    }
}

pub fn create_session_request_from_payload(
    payload: &Value,
) -> Result<CreateSessionRequest, CreateSessionError> {
    let obj = payload
        .as_object()
        .ok_or_else(|| CreateSessionError::bad_request("invalid json body (expected object)"))?;

    let agent_backend_raw = obj.get("agent_backend").map(value_to_pythonish_text);
    let agent_backend =
        normalize_backend(agent_backend_raw.as_deref()).map_err(CreateSessionError::bad_request)?;

    let cwd = obj
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CreateSessionError::bad_request_with_field("cwd required", "cwd"))?;

    let workspace_cwd = match obj.get("workspace_cwd") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        _ => {
            return Err(CreateSessionError::bad_request_with_field(
                "workspace_cwd must be a string",
                "workspace_cwd",
            ))
        }
    };

    let (model_provider, preferred_auth_method, model, reasoning_effort, service_tier) =
        if agent_backend == "codex" {
            let defaults = read_codex_launch_defaults().map_err(CreateSessionError::bad_request)?;
            let allowed = allowed_model_providers(&defaults.provider_choices);
            let model_provider = normalize_requested_model_provider(
                obj.get("model_provider").and_then(Value::as_str),
                Some(&allowed),
            )
            .map_err(CreateSessionError::bad_request)?;
            let preferred_auth_method = normalize_requested_preferred_auth_method(
                obj.get("preferred_auth_method").and_then(Value::as_str),
            )
            .map_err(CreateSessionError::bad_request)?;
            let model = normalize_requested_model(obj.get("model"))
                .map_err(CreateSessionError::bad_request)?;
            let reasoning_effort = normalize_requested_reasoning_effort(
                obj.get("reasoning_effort").and_then(Value::as_str),
            )
            .map_err(CreateSessionError::bad_request)?;
            let service_tier =
                normalize_requested_service_tier(obj.get("service_tier").and_then(Value::as_str))
                    .map_err(CreateSessionError::bad_request)?;
            (
                model_provider,
                preferred_auth_method,
                model,
                reasoning_effort,
                service_tier,
            )
        } else {
            let defaults = read_pi_launch_defaults().map_err(CreateSessionError::bad_request)?;
            let allowed = (!defaults.provider_choices.is_empty()).then_some(
                defaults
                    .provider_choices
                    .iter()
                    .cloned()
                    .collect::<HashSet<_>>(),
            );
            let model_provider = normalize_requested_model_provider(
                obj.get("model_provider").and_then(Value::as_str),
                allowed.as_ref(),
            )
            .map_err(CreateSessionError::bad_request)?;
            if !value_is_missing_or_empty_string(obj.get("preferred_auth_method")) {
                return Err(CreateSessionError::bad_request(
                    "preferred_auth_method is not supported for pi",
                ));
            }
            let model = normalize_requested_model(obj.get("model"))
                .map_err(CreateSessionError::bad_request)?;
            let reasoning_effort =
                normalize_requested_pi_reasoning_effort(obj.get("reasoning_effort"))
                    .map_err(CreateSessionError::bad_request)?;
            if !value_is_missing_or_empty_string(obj.get("service_tier")) {
                return Err(CreateSessionError::bad_request(
                    "service_tier is not supported for pi",
                ));
            }
            (model_provider, None, model, reasoning_effort, None)
        };

    let create_in_tmux = match obj.get("create_in_tmux") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        _ => {
            return Err(CreateSessionError::bad_request(
                "create_in_tmux must be a boolean",
            ))
        }
    };

    let resume_session_id = match obj.get("resume_session_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        _ => {
            return Err(CreateSessionError::bad_request(
                "resume_session_id must be a string",
            ))
        }
    };

    let worktree_branch = match obj.get("worktree_branch") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        _ => {
            return Err(CreateSessionError::bad_request(
                "worktree_branch must be a string",
            ))
        }
    };

    let args = match obj.get("args") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(values)) => {
            let mut out = Vec::new();
            for value in values {
                let Some(text) = value.as_str() else {
                    return Err(CreateSessionError::bad_request(
                        "args must be a list of strings",
                    ));
                };
                if !text.is_empty() {
                    out.push(text.to_string());
                }
            }
            out
        }
        _ => {
            return Err(CreateSessionError::bad_request(
                "args must be a list of strings",
            ))
        }
    };

    Ok(CreateSessionRequest {
        cwd,
        workspace_cwd,
        args,
        agent_backend,
        resume_session_id,
        worktree_branch,
        model_provider,
        preferred_auth_method,
        model,
        reasoning_effort,
        service_tier,
        create_in_tmux,
    })
}

fn value_to_pythonish_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn value_is_missing_or_empty_string(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null))
        || value
            .and_then(Value::as_str)
            .map(|text| text.trim().is_empty())
            .unwrap_or(false)
}

fn create_session_cwd_error(message: &str) -> CreateSessionError {
    CreateSessionError::bad_request_with_field(message.to_string(), "cwd")
}

fn normalize_requested_model(value: Option<&Value>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(text) = value.as_str() else {
        return Ok(None);
    };
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
        return Ok(None);
    }
    Ok(Some(trimmed.to_string()))
}

fn normalize_requested_pi_reasoning_effort(
    value: Option<&Value>,
) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(text) = value.as_str() else {
        return Err(format!(
            "reasoning_effort must be one of {}",
            SUPPORTED_PI_REASONING_EFFORTS.join(", ")
        ));
    };
    let trimmed = text.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if display_pi_reasoning_effort(&trimmed).is_none() {
        return Err(format!(
            "reasoning_effort must be one of {}",
            SUPPORTED_PI_REASONING_EFFORTS.join(", ")
        ));
    }
    Ok(Some(trimmed))
}

fn codex_trust_override_for_path(path: &Path) -> String {
    format!(
        "projects={{ {} = {{ trust_level = \"trusted\" }} }}",
        serde_json::to_string(&path.display().to_string()).unwrap_or_else(|_| "\"\"".to_string())
    )
}

fn tmux_session_name() -> String {
    env::var("CODEX_WEB_TMUX_SESSION")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "codoxear".to_string())
}

fn next_spawn_nonce() -> String {
    format!(
        "{:08x}{:08x}",
        std::process::id(),
        QUEUE_ITEM_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn base_spawn_env_overrides(
    backend_name: &str,
    resume_session_id: Option<&str>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    out.insert("CODEX_WEB_OWNER".to_string(), "web".to_string());
    out.insert(
        "CODEX_WEB_AGENT_BACKEND".to_string(),
        backend_name.to_string(),
    );
    if backend_name == "codex" {
        out.insert("CODEX_HOME".to_string(), codex_home().display().to_string());
    } else {
        out.insert("PI_HOME".to_string(), pi_home().display().to_string());
    }
    if let Some(resume_session_id) = resume_session_id {
        out.insert(
            "CODEX_WEB_RESUME_SESSION_ID".to_string(),
            resume_session_id.to_string(),
        );
    }
    out
}

fn apply_spawn_env(command: &mut Command, env_overrides: &HashMap<String, String>) {
    for key in ["TMUX", "TMUX_PANE"] {
        command.env_remove(key);
    }
    for key in [
        "CODEX_WEB_MODEL_PROVIDER",
        "CODEX_WEB_PREFERRED_AUTH_METHOD",
        "CODEX_WEB_MODEL",
        "CODEX_WEB_REASONING_EFFORT",
        "CODEX_WEB_SERVICE_TIER",
        "CODEX_WEB_WORKSPACE_CWD",
        "CODEX_WEB_TRANSPORT",
        "CODEX_WEB_TMUX_SESSION",
        "CODEX_WEB_TMUX_WINDOW",
        "CODEX_WEB_SPAWN_NONCE",
        "CODEX_WEB_RESUME_SESSION_ID",
        "CODEX_WEB_RESUME_LOG_PATH",
    ] {
        command.env_remove(key);
    }
    if env_overrides.get("CODEX_HOME").is_some() {
        command.env_remove("PI_HOME");
    }
    if env_overrides.get("PI_HOME").is_some() {
        command.env_remove("CODEX_HOME");
    }
    command.envs(env_overrides.iter().map(|(key, value)| (key, value)));
}

fn sorted_env_items(env_overrides: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut items = env_overrides
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.0.cmp(&right.0));
    items
}

fn spawn_command(program: &str) -> Command {
    let env_key = match program {
        "tmux" => Some("CODOXEAR_TMUX_BIN"),
        "setsid" => Some("CODOXEAR_SETSID_BIN"),
        _ => None,
    };
    if let Some(env_key) = env_key {
        if let Some(path) = env::var(env_key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            return Command::new(path);
        }
    }
    Command::new(program)
}

fn wait_or_raise(
    child: &mut std::process::Child,
    label: &str,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().map_err(map_io_error)? {
            Some(status) => {
                let mut err_text = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = stderr.read_to_string(&mut err_text);
                }
                let trimmed = err_text.trim();
                return Err(format!(
                    "{label} exited early (rc={}): {}",
                    status.code().unwrap_or_default(),
                    if trimmed.is_empty() {
                        String::new()
                    } else {
                        trimmed
                            .chars()
                            .rev()
                            .take(4000)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect()
                    }
                ));
            }
            None if Instant::now() >= deadline => return Ok(()),
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn wait_for_spawned_broker_meta(
    config: &RuntimeConfig,
    spawn_nonce: &str,
    timeout: Duration,
) -> Result<Value, String> {
    let deadline = Instant::now() + timeout;
    let socks_dir = config.app_dir.join("socks");
    while Instant::now() <= deadline {
        let mut entries = match fs::read_dir(&socks_dir) {
            Ok(entries) => entries.flatten().collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(value) = read_json_file::<Value>(&path) else {
                continue;
            };
            let Some(obj) = value.as_object() else {
                continue;
            };
            if obj.get("spawn_nonce").and_then(Value::as_str) != Some(spawn_nonce) {
                continue;
            }
            if obj.get("broker_pid").and_then(Value::as_i64).is_none() {
                continue;
            }
            return Ok(value);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(format!(
        "tmux launch did not publish broker metadata within {:.1}s",
        timeout.as_secs_f64()
    ))
}

fn tmux_capture_pane_tail(pane_id: &str, lines: i64) -> String {
    let pane = pane_id.trim();
    if pane.is_empty() {
        return String::new();
    }
    let output = match spawn_command("tmux")
        .args([
            "capture-pane",
            "-p",
            "-t",
            pane,
            "-S",
            &format!("-{}", lines.max(1)),
        ])
        .output()
    {
        Ok(output) => output,
        Err(_) => return String::new(),
    };
    if !output.status.success() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return String::new();
    }
    let lines = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }
    let joined = lines[lines.len().saturating_sub(12)..].join("\n");
    joined
        .chars()
        .rev()
        .take(4000)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

fn shell_quote(raw: &str) -> String {
    if raw.is_empty() {
        return "''".to_string();
    }
    if raw
        .bytes()
        .all(|byte| matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'@' | b'%' | b'+' | b'=' | b':' | b',' | b'.' | b'/' | b'-'))
    {
        return raw.to_string();
    }
    format!("'{}'", raw.replace('\'', "'\"'\"'"))
}

fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|value| shell_quote(value))
        .collect::<Vec<_>>()
        .join(" ")
}

fn repo_root() -> Result<PathBuf, String> {
    if let Some(path) = env::var("CODOXEAR_REPO_ROOT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(path));
    }
    let mut candidates = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.to_path_buf());
        }
    }
    for start in candidates {
        for candidate in start.ancestors() {
            if looks_like_repo_root(candidate) {
                return Ok(candidate.to_path_buf());
            }
        }
    }
    Err("unable to locate Codoxear repo root".to_string())
}

fn looks_like_repo_root(path: &Path) -> bool {
    path.join("backend-rs").join("Cargo.toml").is_file()
        && path.join("codoxear").join("static").is_dir()
}

fn rust_broker_bin(repo_root: &Path) -> PathBuf {
    if let Some(path) = env::var("CODOXEAR_RUST_BROKER_BIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return PathBuf::from(path);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            let sibling = parent.join("codoxear-broker-rs");
            if sibling.exists() {
                return sibling;
            }
            if parent.file_name().and_then(|name| name.to_str()) == Some("deps") {
                if let Some(debug_dir) = parent.parent() {
                    let debug_sibling = debug_dir.join("codoxear-broker-rs");
                    if debug_sibling.exists() {
                        return debug_sibling;
                    }
                }
            }
        }
    }
    repo_root
        .join("backend-rs")
        .join("target")
        .join("release")
        .join("codoxear-broker-rs")
}

fn expand_user_and_vars(raw: &str) -> PathBuf {
    let home = home_dir().unwrap_or_else(|| PathBuf::from("~"));
    let home_text = home.display().to_string();
    let expanded = raw
        .trim()
        .replace("${HOME}", &home_text)
        .replace("$HOME", &home_text);
    expand_home(&expanded)
}

pub(crate) fn resolve_dir_target(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("cwd required".to_string());
    }
    let path = expand_user_and_vars(trimmed);
    let absolute = if path.is_absolute() {
        path
    } else {
        env::current_dir().map_err(map_io_error)?.join(path)
    };
    if absolute.exists() && !absolute.is_dir() {
        return Err(format!("cwd is not a directory: {}", absolute.display()));
    }
    Ok(fs::canonicalize(&absolute).unwrap_or(absolute))
}

fn worktree_path_slug(branch: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in branch.chars() {
        let allowed = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        if allowed {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let trimmed = slug.trim_matches(|ch| ch == '.' || ch == '-').to_string();
    if trimmed.is_empty() {
        "worktree".to_string()
    } else {
        trimmed
    }
}

fn default_worktree_path(source_cwd: &Path, branch: &str) -> PathBuf {
    let slug = worktree_path_slug(branch);
    source_cwd.parent().unwrap_or(source_cwd).join(format!(
        "{}-{slug}",
        source_cwd
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("worktree")
    ))
}

fn create_git_worktree(source_cwd: &Path, worktree_branch: &str) -> Result<PathBuf, String> {
    let Some(repo_root) = git_repo_root(source_cwd) else {
        return Err("cwd is not inside a git worktree".to_string());
    };
    let branch = worktree_branch.trim();
    if branch.is_empty() {
        return Err("worktree_branch required".to_string());
    }
    let target = default_worktree_path(source_cwd, branch);
    if target.exists() {
        return Err(format!(
            "derived worktree path already exists: {}",
            target.display()
        ));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(map_io_error)?;
    }
    let output = Command::new("git")
        .current_dir(&repo_root)
        .args([
            "worktree",
            "add",
            "-b",
            branch,
            &target.display().to_string(),
        ])
        .output()
        .map_err(map_io_error)?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let fallback = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(if !detail.is_empty() {
            detail
        } else if !fallback.is_empty() {
            fallback
        } else {
            format!(
                "git worktree add failed with code {}",
                output.status.code().unwrap_or_default()
            )
        });
    }
    Ok(fs::canonicalize(&target).unwrap_or(target))
}

fn find_resume_candidate_for_cwd(
    cwd: &str,
    agent_backend: &str,
    target_session_id: &str,
) -> Option<(String, Option<PathBuf>)> {
    let cwd_text = fs::canonicalize(expand_user_and_vars(cwd))
        .unwrap_or_else(|_| expand_user_and_vars(cwd))
        .display()
        .to_string();
    for log_path in iter_session_logs_for_backend(agent_backend) {
        let payload = read_session_log_header(&log_path, agent_backend)?;
        if agent_backend == "codex" && is_subagent_session_payload(&payload) {
            continue;
        }
        let session_id = payload.get("id").and_then(Value::as_str)?;
        let row_cwd = payload.get("cwd").and_then(Value::as_str)?;
        if session_id == target_session_id && row_cwd == cwd_text {
            return Some((session_id.to_string(), Some(log_path)));
        }
    }
    None
}

fn iter_session_logs_for_backend(agent_backend: &str) -> Vec<PathBuf> {
    let sessions_dir = agent_sessions_dir(agent_backend);
    if !sessions_dir.exists() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut stack = vec![sessions_dir.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !session_log_matches_backend(&path, agent_backend, &sessions_dir) {
                continue;
            }
            out.push(path);
        }
    }
    out.sort_by(|left, right| {
        file_mtime(right)
            .partial_cmp(&file_mtime(left))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

fn session_log_matches_backend(path: &Path, agent_backend: &str, sessions_dir: &Path) -> bool {
    if agent_backend == "pi" {
        return path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
            && path.starts_with(sessions_dir);
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
        .unwrap_or(false)
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
    value?
        .as_object()?
        .get(key)?
        .as_str()
        .map(|text| text.to_string())
}

fn epoch_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn broker_request(sock_path: &Path, request: &Value, timeout: Duration) -> Result<Value, String> {
    let mut stream = UnixStream::connect(sock_path)
        .map_err(|err| format!("connect {}: {err}", sock_path.display()))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let mut line = serde_json::to_string(request)
        .map_err(|err| format!("serialize broker request {}: {err}", sock_path.display()))?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|err| format!("write {}: {err}", sock_path.display()))?;
    let mut reader = BufReader::new(stream);
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .map_err(|err| format!("read {}: {err}", sock_path.display()))?;
    let trimmed = response_line.trim();
    if trimmed.is_empty() {
        return Err(format!(
            "empty broker response from {}",
            sock_path.display()
        ));
    }
    serde_json::from_str(trimmed)
        .map_err(|err| format!("parse broker response {}: {err}", sock_path.display()))
}

fn session_sock_path(config: &RuntimeConfig, session_id: &str) -> PathBuf {
    config
        .app_dir
        .join("socks")
        .join(format!("{}.sock", session_id))
}

fn read_broker_state(sock_path: &Path) -> Result<Option<BrokerState>, String> {
    if !sock_path.exists() {
        return Ok(None);
    }
    let value = broker_request(
        sock_path,
        &json!({"cmd": "state"}),
        Duration::from_millis(500),
    )?;
    if !value.is_object() {
        return Ok(None);
    }
    let busy = value
        .get("busy")
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("missing busy in broker state for {}", sock_path.display()))?;
    let queue_len = value
        .get("queue_len")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .ok_or_else(|| {
            format!(
                "missing queue_len in broker state for {}",
                sock_path.display()
            )
        })?;
    let token = match value.get("token") {
        Some(Value::Object(_)) => value.get("token").cloned(),
        _ => None,
    };
    Ok(Some(BrokerState {
        busy,
        _queue_len: queue_len,
        token,
    }))
}

fn read_live_broker_state(
    config: &RuntimeConfig,
    session: &ApiSessionSummary,
) -> Result<BrokerState, String> {
    let sock_path = session_sock_path(config, &session.session_id);
    match read_broker_state(&sock_path) {
        Ok(Some(state)) => Ok(state),
        Ok(None) | Err(_) if !pid_alive(session.pid) && !pid_alive(session.broker_pid) => {
            Err(format!("unknown session: {}", session.session_id))
        }
        Ok(None) => Err(format!("missing broker state for {}", sock_path.display())),
        Err(err) => Err(err),
    }
}

fn broker_request_for_session(
    config: &RuntimeConfig,
    session: &ApiSessionSummary,
    request: &Value,
    timeout: Duration,
) -> Result<Value, String> {
    let sock_path = session_sock_path(config, &session.session_id);
    match broker_request(&sock_path, request, timeout) {
        Ok(value) => Ok(value),
        Err(_) if !pid_alive(session.pid) && !pid_alive(session.broker_pid) => {
            Err(format!("unknown session: {}", session.session_id))
        }
        Err(err) => Err(err),
    }
}

pub fn send_session_message(
    config: &RuntimeConfig,
    session_id: &str,
    text: &str,
) -> Result<Value, String> {
    if text.trim().is_empty() {
        return Err("text required".to_string());
    }
    let session = find_session(config, session_id)?;
    let response = broker_request_for_session(
        config,
        &session,
        &json!({"cmd": "send", "text": text}),
        Duration::from_secs_f64(3.0),
    )?;
    if !response.is_object() || response.get("queue_len").and_then(Value::as_u64).is_none() {
        return Err("invalid broker send response".to_string());
    }
    Ok(response)
}

pub fn interrupt_session(config: &RuntimeConfig, session_id: &str) -> Result<Value, String> {
    let session = find_session(config, session_id)?;
    let broker = broker_request_for_session(
        config,
        &session,
        &json!({"cmd": "keys", "seq": TERMINAL_INTERRUPT_SEQ}),
        Duration::from_secs_f64(2.0),
    )?;
    Ok(json!({"ok": true, "broker": broker}))
}

pub fn rename_session(
    config: &RuntimeConfig,
    session_id: &str,
    name: &str,
) -> Result<Value, String> {
    let _ = find_session(config, session_id)?;
    let alias_path = config.app_dir.join("session_aliases.json");
    let mut aliases = read_string_map(&alias_path)?;
    let alias = clean_alias(name);
    if alias.is_empty() {
        aliases.remove(session_id);
    } else {
        aliases.insert(session_id.to_string(), alias.clone());
    }
    write_string_map(&alias_path, &aliases)?;
    Ok(json!({"ok": true, "alias": alias}))
}

pub fn edit_session(
    config: &RuntimeConfig,
    session_id: &str,
    name: &str,
    priority_offset: Option<&Value>,
    snooze_until: Option<&Value>,
    dependency_session_id: Option<&Value>,
) -> Result<Value, String> {
    let _ = find_session(config, session_id)?;
    let alias = clean_alias(name);
    let offset = clean_priority_offset_value(priority_offset)?;
    let snooze_until_clean = clean_snooze_until_value(snooze_until)?;
    let dependency_clean = clean_dependency_session_id_value(dependency_session_id)?;
    if dependency_clean.as_deref() == Some(session_id) {
        return Err("session cannot depend on itself".to_string());
    }
    if let Some(dependency_id) = dependency_clean.as_deref() {
        find_session(config, dependency_id)
            .map_err(|_| "dependency session not found".to_string())?;
    }

    let alias_path = config.app_dir.join("session_aliases.json");
    let mut aliases = read_string_map(&alias_path)?;
    if alias.is_empty() {
        aliases.remove(session_id);
    } else {
        aliases.insert(session_id.to_string(), alias.clone());
    }
    write_string_map(&alias_path, &aliases)?;

    let sidebar_path = config.app_dir.join("session_sidebar.json");
    let mut sidebar = read_object_map(&sidebar_path)?;
    let mut entry = serde_json::Map::new();
    entry.insert("priority_offset".to_string(), json!(offset));
    if let Some(value) = snooze_until_clean {
        entry.insert("snooze_until".to_string(), json!(value));
    }
    if let Some(value) = dependency_clean.clone() {
        entry.insert("dependency_session_id".to_string(), json!(value));
    }
    sidebar.insert(session_id.to_string(), Value::Object(entry));
    write_object_map(&sidebar_path, &sidebar)?;

    Ok(json!({
        "ok": true,
        "alias": alias,
        "priority_offset": offset,
        "snooze_until": snooze_until_clean,
        "dependency_session_id": dependency_clean,
    }))
}

pub fn inject_session_attachment(
    config: &RuntimeConfig,
    session_id: &str,
    filename: &str,
    data_b64: &str,
    attachment_index: i64,
) -> Result<Value, String> {
    if filename.trim().is_empty() {
        return Err("filename required".to_string());
    }
    if data_b64.is_empty() {
        return Err("data_b64 required".to_string());
    }
    let raw = base64::engine::general_purpose::STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|_| "invalid base64".to_string())?;
    let out_path = stage_uploaded_file(config, session_id, filename, &raw)?;
    let inject_text = attachment_inject_text(attachment_index, &out_path)?;
    let seq = format!("\u{1b}[200~{}\u{1b}[201~", inject_text);
    let session = find_session(config, session_id)?;
    let broker = broker_request_for_session(
        config,
        &session,
        &json!({"cmd": "keys", "seq": seq}),
        Duration::from_secs_f64(2.0),
    )?;
    Ok(json!({
        "ok": true,
        "path": out_path.display().to_string(),
        "inject_text": inject_text,
        "broker": broker,
    }))
}

pub fn create_session(
    config: &RuntimeConfig,
    request: CreateSessionRequest,
) -> Result<Value, CreateSessionError> {
    let backend_name = normalize_backend(Some(request.agent_backend.as_str()))
        .map_err(CreateSessionError::bad_request)?;
    let cwd_path =
        resolve_dir_target(&request.cwd).map_err(|message| create_session_cwd_error(&message))?;
    let workspace_cwd = request
        .workspace_cwd
        .as_deref()
        .map(resolve_dir_target)
        .transpose()
        .map_err(|message| CreateSessionError::bad_request_with_field(message, "workspace_cwd"))?;
    if !cwd_path.exists() {
        fs::create_dir_all(&cwd_path).map_err(|err| {
            CreateSessionError::bad_request_with_field(
                format!("cwd could not be created: {}: {}", cwd_path.display(), err),
                "cwd",
            )
        })?;
    }
    if !cwd_path.is_dir() {
        return Err(CreateSessionError::bad_request_with_field(
            format!("cwd is not a directory: {}", cwd_path.display()),
            "cwd",
        ));
    }
    if request.resume_session_id.is_some() && request.worktree_branch.is_some() {
        return Err(CreateSessionError::bad_request(
            "worktree_branch cannot be used when resuming a session",
        ));
    }

    let cwd_display = cwd_path.display().to_string();
    let resume_id = request
        .resume_session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let resume_target = if let Some(resume_id) = resume_id {
        Some(
            find_resume_candidate_for_cwd(&cwd_display, &backend_name, resume_id).ok_or_else(
                || {
                    CreateSessionError::bad_request(format!(
                        "resume session not found for cwd: {resume_id}"
                    ))
                },
            )?,
        )
    } else {
        None
    };

    let spawn_cwd = if let Some(branch) = request.worktree_branch.as_deref() {
        create_git_worktree(&cwd_path, branch).map_err(CreateSessionError::bad_request)?
    } else {
        cwd_path.clone()
    };

    let mut agent_args = Vec::new();
    if backend_name == "codex" {
        agent_args.push("-c".to_string());
        agent_args.push(codex_trust_override_for_path(&spawn_cwd));
        agent_args.push("--dangerously-bypass-approvals-and-sandbox".to_string());
        if let Some(model) = request.model.as_deref() {
            agent_args.push("--model".to_string());
            agent_args.push(model.to_string());
        }
        if let Some(reasoning_effort) = request.reasoning_effort.as_deref() {
            agent_args.push("-c".to_string());
            agent_args.push(format!("model_reasoning_effort=\"{}\"", reasoning_effort));
        }
        if let Some(model_provider) = request.model_provider.as_deref() {
            agent_args.push("-c".to_string());
            agent_args.push(format!("model_provider=\"{}\"", model_provider));
        }
        if let Some(preferred_auth_method) = request.preferred_auth_method.as_deref() {
            agent_args.push("-c".to_string());
            agent_args.push(format!(
                "preferred_auth_method=\"{}\"",
                preferred_auth_method
            ));
        }
        if let Some(service_tier) = request.service_tier.as_deref() {
            agent_args.push("-c".to_string());
            agent_args.push(format!("service_tier=\"{}\"", service_tier));
        }
        if let Some(resume_id) = resume_id {
            agent_args.push("resume".to_string());
            agent_args.push(resume_id.to_string());
        }
    } else {
        if request.preferred_auth_method.is_some() {
            return Err(CreateSessionError::bad_request(
                "preferred_auth_method is not supported for pi",
            ));
        }
        if request.service_tier.is_some() {
            return Err(CreateSessionError::bad_request(
                "service_tier is not supported for pi",
            ));
        }
        if let Some(model_provider) = request.model_provider.as_deref() {
            agent_args.push("--provider".to_string());
            agent_args.push(model_provider.to_string());
        }
        if let Some(model) = request.model.as_deref() {
            agent_args.push("--model".to_string());
            agent_args.push(model.to_string());
        }
        if let Some(reasoning_effort) = request.reasoning_effort.as_deref() {
            agent_args.push("--thinking".to_string());
            agent_args.push(reasoning_effort.to_string());
        }
        if let Some((resume_id, resume_log_path)) = resume_target.as_ref() {
            agent_args.push("--session".to_string());
            agent_args.push(
                resume_log_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| resume_id.clone()),
            );
        }
    }
    agent_args.extend(
        request
            .args
            .iter()
            .filter(|value| !value.is_empty())
            .cloned(),
    );

    let repo_root = repo_root().map_err(CreateSessionError::internal)?;
    let broker_program = rust_broker_bin(&repo_root);
    let mut broker_args = vec![
        "--cwd".to_string(),
        spawn_cwd.display().to_string(),
        "--".to_string(),
    ];
    broker_args.extend(agent_args.iter().cloned());
    let tmux_session = tmux_session_name();
    let mut env_overrides = base_spawn_env_overrides(&backend_name, resume_id);
    if let Some(model_provider) = request.model_provider.as_deref() {
        env_overrides.insert(
            "CODEX_WEB_MODEL_PROVIDER".to_string(),
            model_provider.to_string(),
        );
    }
    if let Some(preferred_auth_method) = request.preferred_auth_method.as_deref() {
        env_overrides.insert(
            "CODEX_WEB_PREFERRED_AUTH_METHOD".to_string(),
            preferred_auth_method.to_string(),
        );
    }
    if let Some(model) = request.model.as_deref() {
        env_overrides.insert("CODEX_WEB_MODEL".to_string(), model.to_string());
    }
    if let Some(reasoning_effort) = request.reasoning_effort.as_deref() {
        env_overrides.insert(
            "CODEX_WEB_REASONING_EFFORT".to_string(),
            reasoning_effort.to_string(),
        );
    }
    if let Some(service_tier) = request.service_tier.as_deref() {
        env_overrides.insert(
            "CODEX_WEB_SERVICE_TIER".to_string(),
            service_tier.to_string(),
        );
    }
    if let Some(workspace_cwd) = workspace_cwd.as_ref() {
        env_overrides.insert(
            "CODEX_WEB_WORKSPACE_CWD".to_string(),
            workspace_cwd.display().to_string(),
        );
    }

    if request.create_in_tmux {
        if !tmux_available() {
            return Err(CreateSessionError::bad_request(
                "tmux is unavailable on this host",
            ));
        }
        let spawn_nonce = next_spawn_nonce();
        let tmux_window = safe_filename(
            &format!(
                "{}-{}",
                spawn_cwd
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("session"),
                &spawn_nonce[..6],
            ),
            "session",
        );
        env_overrides.insert("CODEX_WEB_TRANSPORT".to_string(), "tmux".to_string());
        env_overrides.insert("CODEX_WEB_TMUX_SESSION".to_string(), tmux_session.clone());
        env_overrides.insert("CODEX_WEB_TMUX_WINDOW".to_string(), tmux_window.clone());
        env_overrides.insert("CODEX_WEB_SPAWN_NONCE".to_string(), spawn_nonce.clone());

        let mut inline_argv = vec!["env".to_string()];
        for (key, value) in sorted_env_items(&env_overrides) {
            inline_argv.push(format!("{key}={value}"));
        }
        if let Some(codex_bin) = env::var("CODEX_BIN")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            inline_argv.push(format!("CODEX_BIN={codex_bin}"));
        }
        if let Some(pi_bin) = env::var("PI_BIN")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            inline_argv.push(format!("PI_BIN={pi_bin}"));
        }
        inline_argv.push(broker_program.display().to_string());
        inline_argv.extend(broker_args.iter().cloned());
        let shell_cmd = format!(
            "cd {} && exec {}",
            shell_quote(&repo_root.display().to_string()),
            shell_join(&inline_argv),
        );

        let has_session = spawn_command("tmux")
            .args(["has-session", "-t", &tmux_session])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|err| CreateSessionError::internal(format!("tmux launch failed: {err}")))?;
        let tmux_args = if has_session.success() {
            vec![
                "new-window".to_string(),
                "-d".to_string(),
                "-P".to_string(),
                "-F".to_string(),
                "#{pane_id}".to_string(),
                "-t".to_string(),
                format!("{tmux_session}:"),
                "-n".to_string(),
                tmux_window.clone(),
                shell_cmd,
            ]
        } else {
            vec![
                "new-session".to_string(),
                "-d".to_string(),
                "-P".to_string(),
                "-F".to_string(),
                "#{pane_id}".to_string(),
                "-s".to_string(),
                tmux_session.clone(),
                "-n".to_string(),
                tmux_window.clone(),
                shell_cmd,
            ]
        };
        let mut tmux_proc = spawn_command("tmux");
        tmux_proc.current_dir(&repo_root);
        apply_spawn_env(&mut tmux_proc, &env_overrides);
        let tmux_output = tmux_proc
            .args(tmux_args.iter().map(String::as_str))
            .output()
            .map_err(|err| CreateSessionError::internal(format!("tmux launch failed: {err}")))?;
        if !tmux_output.status.success() {
            let detail = String::from_utf8_lossy(&tmux_output.stderr)
                .trim()
                .to_string();
            let fallback = String::from_utf8_lossy(&tmux_output.stdout)
                .trim()
                .to_string();
            let message = if !detail.is_empty() {
                detail
            } else if !fallback.is_empty() {
                fallback
            } else {
                format!(
                    "exit status {}",
                    tmux_output.status.code().unwrap_or_default()
                )
            };
            return Err(CreateSessionError::internal(format!(
                "tmux launch failed: {message}"
            )));
        }
        let pane_id = String::from_utf8_lossy(&tmux_output.stdout)
            .trim()
            .to_string();
        let meta = wait_for_spawned_broker_meta(
            config,
            &spawn_nonce,
            Duration::from_secs_f64(TMUX_META_WAIT_SECONDS),
        )
        .map_err(|message| {
            let pane_tail = tmux_capture_pane_tail(&pane_id, 80);
            if pane_tail.is_empty() {
                CreateSessionError::internal(message)
            } else {
                CreateSessionError::internal(format!(
                    "{message}\nLast tmux pane output:\n{pane_tail}"
                ))
            }
        })?;
        let broker_pid = meta
            .get("broker_pid")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                CreateSessionError::internal("tmux launch metadata is missing broker_pid")
            })?;
        return Ok(json!({
            "ok": true,
            "broker_pid": broker_pid,
            "tmux_session": tmux_session,
            "tmux_window": tmux_window,
        }));
    }

    let mut child = spawn_command("setsid");
    child
        .current_dir(&repo_root)
        .arg(&broker_program)
        .args(broker_args.iter().map(String::as_str))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    apply_spawn_env(&mut child, &env_overrides);
    let mut child = child
        .spawn()
        .map_err(|err| CreateSessionError::internal(format!("spawn failed: {err}")))?;
    wait_or_raise(&mut child, "broker", Duration::from_secs_f64(1.5))
        .map_err(CreateSessionError::internal)?;
    let broker_pid = i64::from(child.id());
    let stderr = child.stderr.take();
    std::thread::spawn(move || {
        if let Some(mut stderr) = stderr {
            let mut sink = Vec::new();
            let _ = stderr.read_to_end(&mut sink);
        }
        let _ = child.wait();
    });
    Ok(json!({"ok": true, "broker_pid": broker_pid}))
}

pub fn delete_session(config: &RuntimeConfig, session_id: &str) -> Result<Value, String> {
    let session = find_session(config, session_id)?;
    let sock_path = session_sock_path(config, &session.session_id);
    let meta_path = sock_path.with_extension("json");
    let deleted = match broker_request_for_session(
        config,
        &session,
        &json!({"cmd": "shutdown"}),
        Duration::from_secs_f64(1.0),
    ) {
        Ok(response) if response.get("ok").and_then(Value::as_bool) == Some(true) => true,
        Ok(_) | Err(_) => kill_session_via_pids(config, &session),
    };
    if !deleted {
        return Err(format!("unknown session: {session_id}"));
    }
    unlink_quiet(&sock_path);
    unlink_quiet(&meta_path);
    clear_deleted_session_state(config, session_id, Some(session.cwd.as_str()))?;
    Ok(json!({"ok": true}))
}

pub(crate) fn discover_open_log_for_process(
    root_pid: i64,
    cwd: &str,
    agent_backend: &str,
) -> Option<PathBuf> {
    let sessions_dir = agent_sessions_dir(agent_backend);
    discover_open_log_for_process_in_sessions(root_pid, cwd, agent_backend, &sessions_dir)
}

pub(crate) fn discover_open_log_for_process_in_sessions(
    root_pid: i64,
    cwd: &str,
    agent_backend: &str,
    sessions_dir: &Path,
) -> Option<PathBuf> {
    discover_open_log_for_process_in_sessions_excluding(
        root_pid,
        cwd,
        agent_backend,
        sessions_dir,
        &HashSet::new(),
    )
}

pub(crate) fn discover_open_log_for_process_in_sessions_excluding(
    root_pid: i64,
    cwd: &str,
    agent_backend: &str,
    sessions_dir: &Path,
    excluded_paths: &HashSet<PathBuf>,
) -> Option<PathBuf> {
    if root_pid <= 0 {
        return None;
    }
    let proc_root = Path::new("/proc");
    if !proc_root.exists() {
        return None;
    }
    let mut candidates =
        proc_open_writable_rollout_logs(proc_root, root_pid, agent_backend, sessions_dir);
    if !excluded_paths.is_empty() {
        candidates.retain(|path| !path_in_set(path, excluded_paths));
    }
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by(|a, b| {
        file_mtime(b)
            .partial_cmp(&file_mtime(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut matches = Vec::new();
    let mut unique_open_main = Vec::new();
    for path in candidates {
        let Some(payload) = read_session_log_header(&path, agent_backend) else {
            continue;
        };
        if agent_backend == "codex" && is_subagent_session_payload(&payload) {
            continue;
        }
        unique_open_main.push(path.clone());
        if payload.get("cwd").and_then(Value::as_str) == Some(cwd) {
            matches.push(path);
        }
    }
    if matches.len() == 1 {
        return matches.into_iter().next();
    }
    if !cwd.is_empty() && matches.is_empty() && unique_open_main.len() == 1 {
        return unique_open_main.into_iter().next();
    }
    None
}

fn path_in_set(path: &Path, paths: &HashSet<PathBuf>) -> bool {
    if paths.contains(path) {
        return true;
    }
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    paths.iter().any(|item| item == path || item == &resolved)
}

fn proc_open_writable_rollout_logs(
    proc_root: &Path,
    root_pid: i64,
    agent_backend: &str,
    sessions_dir: &Path,
) -> Vec<PathBuf> {
    let uid = fs::metadata(proc_root.join("self"))
        .ok()
        .map(|meta| meta.uid());
    let mut out = HashSet::new();
    for pid in proc_descendants(proc_root, root_pid) {
        let pid_uid = proc_pid_uid(proc_root, pid);
        if uid.is_some() && pid_uid.is_some() && uid != pid_uid {
            continue;
        }
        let fd_dir = proc_root.join(pid.to_string()).join("fd");
        let Ok(entries) = fs::read_dir(fd_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let fd_name = entry.file_name();
            let Some(fd_name) = fd_name.to_str() else {
                continue;
            };
            let Some(flags) = proc_fd_flags(proc_root, pid, fd_name) else {
                continue;
            };
            if !fd_has_write_intent(flags) {
                continue;
            }
            let Ok(target) = fs::read_link(entry.path()) else {
                continue;
            };
            let path_text = target.to_string_lossy();
            if path_text.ends_with(" (deleted)") {
                continue;
            }
            if !target.is_absolute()
                || target.extension().and_then(|ext| ext.to_str()) != Some("jsonl")
            {
                continue;
            }
            if is_rollout_log_path(&target, agent_backend, sessions_dir) {
                out.insert(target);
            }
        }
    }
    out.into_iter().collect()
}

fn agent_sessions_dir(agent_backend: &str) -> PathBuf {
    if agent_backend == "pi" {
        pi_home().join("agent").join("sessions")
    } else {
        codex_home().join("sessions")
    }
}

fn is_rollout_log_path(path: &Path, agent_backend: &str, sessions_dir: &Path) -> bool {
    if agent_backend == "pi" {
        return path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
            && path.starts_with(sessions_dir);
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
        .unwrap_or(false)
}

fn proc_pid_uid(proc_root: &Path, pid: i64) -> Option<u32> {
    fs::metadata(proc_root.join(pid.to_string()))
        .ok()
        .map(|meta| meta.uid())
}

fn proc_children(proc_root: &Path, pid: i64) -> Vec<i64> {
    let path = proc_root
        .join(pid.to_string())
        .join("task")
        .join(pid.to_string())
        .join("children");
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    raw.split_whitespace()
        .filter_map(|value| value.parse::<i64>().ok())
        .collect()
}

fn proc_descendants(proc_root: &Path, root_pid: i64) -> Vec<i64> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![root_pid];
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        out.push(pid);
        stack.extend(proc_children(proc_root, pid));
    }
    out
}

fn proc_fd_flags(proc_root: &Path, pid: i64, fd_name: &str) -> Option<i64> {
    let path = proc_root.join(pid.to_string()).join("fdinfo").join(fd_name);
    let raw = fs::read_to_string(path).ok()?;
    for line in raw.lines() {
        if !line.starts_with("flags:") {
            continue;
        }
        let value = line.split_once(':')?.1.trim().split_whitespace().next()?;
        return i64::from_str_radix(value, 8).ok();
    }
    None
}

fn fd_has_write_intent(flags: i64) -> bool {
    matches!(flags & 0o3, 0o1 | 0o2)
}

fn read_session_log_header(path: &Path, agent_backend: &str) -> Option<Value> {
    if agent_backend == "pi" {
        let value = read_first_json_object(path)?;
        return (value.get("type").and_then(Value::as_str) == Some("session")).then_some(value);
    }
    read_codex_session_meta_payload(path)
}

fn is_subagent_session_payload(payload: &Value) -> bool {
    payload
        .get("source")
        .and_then(Value::as_object)
        .and_then(|source| source.get("subagent"))
        .is_some()
}

fn file_mtime(path: &Path) -> f64 {
    path.metadata()
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn pid_alive(pid: i64) -> bool {
    pid > 0 && Path::new("/proc").join(pid.to_string()).exists()
}

fn last_conversation_ts_from_log(path: &Path) -> Option<f64> {
    read_positioned_records(path)
        .ok()?
        .iter()
        .filter_map(|record| sidebar_conversation_ts(&record.obj))
        .last()
}

fn last_assistant_ts_from_log(path: &Path) -> Option<f64> {
    let mut last = None;
    for record in read_positioned_records(path).ok()? {
        if has_assistant_output(&record.obj) {
            last = event_ts(&record.obj);
        }
    }
    last
}

fn latest_token_update_from_log(path: &Path) -> Option<Value> {
    let records = read_positioned_records(path).ok()?;
    for record in records.iter().rev() {
        if let Some(update) = token_update_from_obj(&record.obj) {
            return Some(update);
        }
    }
    None
}

pub(crate) fn compute_idle_from_log(path: &Path) -> Option<bool> {
    let size = file_len(path).ok()?;
    let records = read_positioned_records(path).ok()?;
    if records.is_empty() {
        return None;
    }
    let mut saw_terminal_signal = false;
    let mut idle = true;
    for record in records {
        let obj = &record.obj;
        match obj.get("type").and_then(Value::as_str) {
            Some("message") => {
                if pi_user_text_value(obj).is_some() {
                    saw_terminal_signal = true;
                    idle = false;
                    continue;
                }
                if pi_assistant_text_value(obj).is_some() {
                    saw_terminal_signal = true;
                    idle = pi_assistant_is_final_turn_end(obj);
                    continue;
                }
                if pi_message_keeps_turn_busy(obj) {
                    saw_terminal_signal = true;
                    idle = false;
                    continue;
                }
            }
            Some("event_msg") => {
                let Some(payload) = obj.get("payload").and_then(Value::as_object) else {
                    continue;
                };
                match payload.get("type").and_then(Value::as_str) {
                    Some("user_message")
                        if payload.get("message").and_then(Value::as_str).is_some() =>
                    {
                        saw_terminal_signal = true;
                        idle = false;
                    }
                    Some("agent_message")
                        if payload
                            .get("message")
                            .and_then(Value::as_str)
                            .map(|text| !text.trim().is_empty())
                            .unwrap_or(false) =>
                    {
                        saw_terminal_signal = true;
                        idle = false;
                    }
                    Some("agent_reasoning") => {
                        saw_terminal_signal = true;
                        idle = false;
                    }
                    Some("turn_aborted")
                    | Some("thread_rolled_back")
                    | Some("task_complete")
                    | Some("turn_complete") => {
                        saw_terminal_signal = true;
                        idle = true;
                    }
                    _ => {}
                }
            }
            Some("response_item") => {
                let Some(payload) = obj.get("payload").and_then(Value::as_object) else {
                    continue;
                };
                let payload_type = payload.get("type").and_then(Value::as_str);
                if response_item_has_assistant_output(obj) {
                    saw_terminal_signal = true;
                    idle = payload.get("end_turn").and_then(Value::as_bool) == Some(true);
                    continue;
                }
                if matches!(
                    payload_type,
                    Some("reasoning")
                        | Some("function_call")
                        | Some("function_call_output")
                        | Some("custom_tool_call")
                        | Some("custom_tool_call_output")
                        | Some("web_search_call")
                        | Some("local_shell_call")
                ) {
                    saw_terminal_signal = true;
                    idle = false;
                }
            }
            _ => {}
        }
    }
    if !saw_terminal_signal {
        return Some(size <= 128 * 1024);
    }
    Some(idle)
}

fn last_chat_role_ts_from_log(path: &Path) -> Option<(&'static str, f64)> {
    let records = read_positioned_records(path).ok()?;
    if records.is_empty() {
        return None;
    }
    let max_scan_bytes = harness_max_scan_bytes() as u64;
    let file_size = file_len(path).ok()?;
    let start_byte = file_size.saturating_sub(max_scan_bytes);

    let mut last_user: Option<(u64, f64)> = None;
    let mut last_assistant: Option<(u64, f64)> = None;
    for record in records {
        if record.start < start_byte {
            continue;
        }
        let obj = &record.obj;
        match obj.get("type").and_then(Value::as_str) {
            Some("message") => {
                if pi_user_text_value(obj).is_some() {
                    if let Some(ts) = event_ts(obj) {
                        last_user = Some((record.start, ts));
                    }
                    continue;
                }
                if pi_assistant_text_value(obj).is_some() || pi_message_keeps_turn_busy(obj) {
                    if let Some(ts) = event_ts(obj) {
                        last_assistant = Some((record.start, ts));
                    }
                    continue;
                }
            }
            Some("event_msg") => {
                let Some(payload) = obj.get("payload").and_then(Value::as_object) else {
                    continue;
                };
                match payload.get("type").and_then(Value::as_str) {
                    Some("user_message")
                        if payload.get("message").and_then(Value::as_str).is_some() =>
                    {
                        if let Some(ts) = event_ts(obj) {
                            last_user = Some((record.start, ts));
                        }
                    }
                    Some("agent_message")
                        if payload
                            .get("message")
                            .and_then(Value::as_str)
                            .map(|text| !text.trim().is_empty())
                            .unwrap_or(false) =>
                    {
                        if let Some(ts) = event_ts(obj) {
                            last_assistant = Some((record.start, ts));
                        }
                    }
                    _ => {}
                }
            }
            Some("response_item") if response_item_has_assistant_output(obj) => {
                if let Some(ts) = event_ts(obj) {
                    last_assistant = Some((record.start, ts));
                }
            }
            _ => {}
        }
    }

    match (last_user, last_assistant) {
        (Some((user_pos, user_ts)), Some((assistant_pos, assistant_ts))) => {
            if assistant_pos > user_pos {
                Some(("assistant", assistant_ts))
            } else {
                Some(("user", user_ts))
            }
        }
        (Some((_user_pos, user_ts)), None) => Some(("user", user_ts)),
        (None, Some((_assistant_pos, assistant_ts))) => Some(("assistant", assistant_ts)),
        (None, None) => None,
    }
}

fn sidebar_conversation_ts(obj: &Value) -> Option<f64> {
    match obj.get("type").and_then(Value::as_str) {
        Some("event_msg") => {
            let payload = obj.get("payload").and_then(Value::as_object)?;
            match payload.get("type").and_then(Value::as_str) {
                Some("user_message")
                    if payload.get("message").and_then(Value::as_str).is_some() =>
                {
                    event_ts(obj)
                }
                Some("task_complete") | Some("turn_complete")
                    if payload
                        .get("last_agent_message")
                        .and_then(Value::as_str)
                        .map(|text| !text.trim().is_empty())
                        .unwrap_or(false) =>
                {
                    event_ts(obj)
                }
                Some("agent_message")
                    if payload.get("phase").and_then(Value::as_str) == Some("final_answer")
                        && payload
                            .get("message")
                            .and_then(Value::as_str)
                            .map(|text| !text.trim().is_empty())
                            .unwrap_or(false) =>
                {
                    event_ts(obj)
                }
                _ => None,
            }
        }
        Some("message") => {
            if pi_user_text_value(obj).is_some() || pi_assistant_text_value(obj).is_some() {
                event_ts(obj)
            } else {
                None
            }
        }
        Some("response_item") => {
            let payload = obj.get("payload").and_then(Value::as_object)?;
            if payload.get("type").and_then(Value::as_str) != Some("message")
                || payload.get("role").and_then(Value::as_str) != Some("assistant")
                || !(payload.get("phase").and_then(Value::as_str) == Some("final_answer")
                    || payload.get("end_turn").and_then(Value::as_bool) == Some(true))
                || output_text(payload.get("content")?).is_none()
            {
                None
            } else {
                event_ts(obj)
            }
        }
        _ => None,
    }
}

fn has_assistant_output(obj: &Value) -> bool {
    matches!(
        obj.get("type").and_then(Value::as_str),
        Some("message") | Some("response_item") | Some("event_msg")
    ) && (pi_assistant_text_value(obj).is_some()
        || response_item_has_assistant_output(obj)
        || event_msg_has_assistant_output(obj))
}

fn event_msg_has_assistant_output(obj: &Value) -> bool {
    obj.get("type").and_then(Value::as_str) == Some("event_msg")
        && obj
            .get("payload")
            .and_then(Value::as_object)
            .map(|payload| {
                payload.get("type").and_then(Value::as_str) == Some("agent_message")
                    && payload
                        .get("message")
                        .and_then(Value::as_str)
                        .map(|text| !text.trim().is_empty())
                        .unwrap_or(false)
            })
            .unwrap_or(false)
}

fn response_item_has_assistant_output(obj: &Value) -> bool {
    let Some(payload) = obj.get("payload").and_then(Value::as_object) else {
        return false;
    };
    payload.get("type").and_then(Value::as_str) == Some("message")
        && payload.get("role").and_then(Value::as_str) == Some("assistant")
        && payload.get("content").and_then(output_text).is_some()
}

fn pi_message_role_value(obj: &Value) -> Option<&str> {
    obj.get("message")?.get("role")?.as_str()
}

fn pi_message_content_parts(obj: &Value) -> Vec<&serde_json::Map<String, Value>> {
    obj.get("message")
        .and_then(Value::as_object)
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .map(|parts| parts.iter().filter_map(Value::as_object).collect())
        .unwrap_or_default()
}

fn pi_user_text_value(obj: &Value) -> Option<String> {
    if obj.get("type").and_then(Value::as_str) != Some("message")
        || pi_message_role_value(obj) != Some("user")
    {
        return None;
    }
    content_text(obj.get("message")?.get("content")?)
}

fn pi_assistant_text_value(obj: &Value) -> Option<String> {
    if obj.get("type").and_then(Value::as_str) != Some("message")
        || pi_message_role_value(obj) != Some("assistant")
    {
        return None;
    }
    let text = pi_message_content_parts(obj)
        .iter()
        .filter_map(|part| {
            if part.get("type").and_then(Value::as_str) == Some("text") {
                part.get("text").and_then(Value::as_str)
            } else {
                None
            }
        })
        .collect::<String>();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn pi_assistant_tool_use_count(obj: &Value) -> usize {
    if pi_message_role_value(obj) != Some("assistant") {
        return 0;
    }
    pi_message_content_parts(obj)
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("toolCall"))
        .count()
}

fn pi_assistant_thinking_count(obj: &Value) -> usize {
    if pi_message_role_value(obj) != Some("assistant") {
        return 0;
    }
    pi_message_content_parts(obj)
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("thinking"))
        .count()
}

fn pi_assistant_is_final_turn_end(obj: &Value) -> bool {
    if pi_assistant_text_value(obj).is_none() {
        return false;
    }
    let stop_reason = obj
        .get("message")
        .and_then(Value::as_object)
        .and_then(|message| message.get("stopReason"))
        .and_then(Value::as_str);
    if pi_assistant_tool_use_count(obj) == 0
        && pi_assistant_thinking_count(obj) == 0
        && stop_reason != Some("toolUse")
    {
        return true;
    }
    matches!(stop_reason, Some(reason) if reason != "toolUse")
}

fn pi_message_keeps_turn_busy(obj: &Value) -> bool {
    pi_message_role_value(obj) == Some("toolResult")
        || pi_assistant_tool_use_count(obj) > 0
        || pi_assistant_thinking_count(obj) > 0
}

pub(crate) fn token_update_from_obj(obj: &Value) -> Option<Value> {
    pi_token_update_from_obj(obj).or_else(|| codex_token_update_from_obj(obj))
}

fn codex_token_update_from_obj(obj: &Value) -> Option<Value> {
    if obj.get("type").and_then(Value::as_str) != Some("event_msg") {
        return None;
    }
    let payload = obj.get("payload")?.as_object()?;
    if payload.get("type").and_then(Value::as_str) != Some("token_count") {
        return None;
    }
    let info = payload.get("info")?.as_object()?;
    let total_usage = info.get("total_token_usage")?.as_object()?;
    let _ = total_usage;
    let context_window = info.get("model_context_window")?.as_i64()?;
    let total_tokens = info
        .get("last_token_usage")?
        .get("total_tokens")?
        .as_i64()?;
    Some(json!({
        "context_window": context_window,
        "tokens_in_context": total_tokens,
        "tokens_remaining": (context_window - total_tokens).max(0),
        "percent_remaining": context_percent_remaining(total_tokens, context_window),
        "baseline_tokens": CONTEXT_WINDOW_BASELINE_TOKENS,
        "as_of": obj.get("timestamp").and_then(Value::as_str),
    }))
}

fn pi_token_update_from_obj(obj: &Value) -> Option<Value> {
    if obj.get("type").and_then(Value::as_str) != Some("message")
        || pi_message_role_value(obj) != Some("assistant")
    {
        return None;
    }
    let message = obj.get("message")?.as_object()?;
    let total_tokens = message.get("usage")?.get("totalTokens")?.as_i64()?;
    let provider = message.get("provider")?.as_str()?;
    let model = message.get("model")?.as_str()?;
    let context_window = pi_model_context_window(provider, model)?;
    Some(json!({
        "context_window": context_window,
        "tokens_in_context": total_tokens,
        "tokens_remaining": (context_window - total_tokens).max(0),
        "percent_remaining": context_percent_remaining(total_tokens, context_window),
        "baseline_tokens": CONTEXT_WINDOW_BASELINE_TOKENS,
        "as_of": obj.get("timestamp").and_then(Value::as_str),
    }))
}

fn context_percent_remaining(tokens_in_context: i64, context_window: i64) -> i64 {
    if context_window <= CONTEXT_WINDOW_BASELINE_TOKENS {
        return 0;
    }
    let effective = context_window - CONTEXT_WINDOW_BASELINE_TOKENS;
    let used = (tokens_in_context - CONTEXT_WINDOW_BASELINE_TOKENS).max(0);
    let remaining = (effective - used).max(0);
    ((remaining as f64 / effective as f64) * 100.0).round() as i64
}

fn pi_model_context_window(provider: &str, model: &str) -> Option<i64> {
    let data: Value = read_json_file(&pi_models_path()).ok()?;
    data.get("providers")?
        .get(provider)?
        .get("models")?
        .as_array()?
        .iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(model))?
        .get("contextWindow")?
        .as_i64()
}

fn read_run_settings_from_log(
    path: &Path,
    agent_backend: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    if agent_backend == "pi" {
        let Some(obj) = read_first_json_object(path) else {
            return (None, None, None);
        };
        let provider = obj
            .get("provider")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let model = obj
            .get("model")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let reasoning = obj
            .get("thinking_level")
            .and_then(Value::as_str)
            .and_then(display_pi_reasoning_effort)
            .or(Some("high".to_string()));
        return (provider, model, reasoning);
    }
    let Some(payload) = read_codex_session_meta_payload(path) else {
        return (None, None, None);
    };
    (
        payload
            .get("model_provider")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        payload
            .get("model")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        payload
            .get("reasoning_effort")
            .and_then(Value::as_str)
            .and_then(display_reasoning_effort),
    )
}

fn read_first_json_object(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            return Some(value);
        }
    }
    None
}

fn read_codex_session_meta_payload(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            continue;
        }
        let payload = value.get("payload")?;
        if payload.is_object() {
            return Some(payload.clone());
        }
    }
    None
}

fn new_session_defaults() -> Result<ApiNewSessionDefaults, String> {
    let mut backends = HashMap::new();
    backends.insert("codex".to_string(), read_codex_launch_defaults()?);
    backends.insert("pi".to_string(), read_pi_launch_defaults()?);
    let default_backend_raw =
        env::var("CODEX_WEB_DEFAULT_AGENT_BACKEND").unwrap_or_else(|_| "codex".to_string());
    Ok(ApiNewSessionDefaults {
        default_backend: normalize_backend(Some(default_backend_raw.as_str()))?,
        backends,
    })
}

fn read_codex_launch_defaults() -> Result<ApiBackendDefaults, String> {
    let mut configured_model = None;
    let mut configured_effort = None;
    let mut configured_provider = Some("openai".to_string());
    let mut configured_auth_method = Some("apikey".to_string());
    let mut configured_service_tier = Some("flex".to_string());
    let mut configured_providers = vec!["chatgpt".to_string(), "openai-api".to_string()];
    let config_path = codex_config_path();
    if config_path.exists() {
        let data = fs::read_to_string(&config_path)
            .map_err(|err| format!("read {}: {err}", config_path.display()))?;
        let parsed: TomlValue = toml::from_str(&data)
            .map_err(|err| format!("parse {}: {err}", config_path.display()))?;
        let table = parsed
            .as_table()
            .ok_or_else(|| format!("invalid Codex config in {}", config_path.display()))?;
        configured_model = table.get("model").and_then(toml_string);
        configured_effort = table
            .get("model_reasoning_effort")
            .and_then(toml_string)
            .and_then(|value| display_reasoning_effort(&value));
        if let Some(value) = table.get("preferred_auth_method").and_then(toml_string) {
            if let Some(method) = normalize_requested_preferred_auth_method(Some(&value))? {
                configured_auth_method = Some(method);
            }
        }
        configured_providers = vec!["chatgpt".to_string(), "openai-api".to_string()];
        configured_providers.extend(
            configured_model_providers(table)
                .into_iter()
                .filter(|provider| provider != "openai"),
        );
        let allowed = allowed_model_providers(&configured_providers);
        if let Some(value) = table
            .get("model_provider")
            .or_else(|| table.get("model_provider_id"))
            .and_then(toml_string)
        {
            if let Some(provider) =
                normalize_requested_model_provider(Some(&value), Some(&allowed))?
            {
                configured_provider = Some(provider);
            }
        }
        if let Some(value) = table.get("service_tier").and_then(toml_string) {
            if let Some(tier) = normalize_requested_service_tier(Some(&value))? {
                configured_service_tier = Some(tier);
            }
        }
    }

    if configured_effort.is_none() {
        let cache_path = models_cache_path();
        if cache_path.exists() {
            let cache: Value = read_json_file(&cache_path)?;
            let rows = cache
                .get("models")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(model) = configured_model.as_deref() {
                configured_effort = rows
                    .iter()
                    .find(|row| {
                        [
                            row.get("slug").and_then(Value::as_str),
                            row.get("display_name").and_then(Value::as_str),
                        ]
                        .into_iter()
                        .flatten()
                        .any(|candidate| candidate == model)
                    })
                    .and_then(|row| row.get("default_reasoning_level").and_then(Value::as_str))
                    .and_then(display_reasoning_effort);
            }
            if configured_effort.is_none() {
                configured_effort = rows
                    .iter()
                    .min_by(|left, right| {
                        left.get("priority")
                            .and_then(Value::as_i64)
                            .unwrap_or(999_999)
                            .cmp(
                                &right
                                    .get("priority")
                                    .and_then(Value::as_i64)
                                    .unwrap_or(999_999),
                            )
                            .then_with(|| {
                                left.get("slug")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .cmp(
                                        right
                                            .get("slug")
                                            .and_then(Value::as_str)
                                            .unwrap_or_default(),
                                    )
                            })
                    })
                    .and_then(|row| row.get("default_reasoning_level").and_then(Value::as_str))
                    .and_then(display_reasoning_effort);
            }
        }
    }

    let mut defaults = ApiBackendDefaults {
        agent_backend: "codex".to_string(),
        model_provider: configured_provider,
        preferred_auth_method: configured_auth_method,
        provider_choice: None,
        provider_choices: configured_providers,
        model: configured_model,
        models: vec![],
        reasoning_effort: configured_effort.unwrap_or_default(),
        reasoning_efforts: SUPPORTED_REASONING_EFFORTS
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        service_tier: configured_service_tier,
        supports_fast: true,
    };
    apply_codex_launch_default_env_overrides(&mut defaults)?;
    defaults.provider_choice = provider_choice_for_settings(
        defaults.model_provider.as_deref(),
        defaults.preferred_auth_method.as_deref(),
    );
    Ok(defaults)
}

fn read_pi_launch_defaults() -> Result<ApiBackendDefaults, String> {
    let mut configured_provider = None;
    let mut configured_model = None;
    let configured_effort = "high".to_string();
    let mut provider_choices = Vec::new();
    let mut model_choices = Vec::new();

    let settings_path = pi_settings_path();
    if settings_path.exists() {
        let settings: Value = read_json_file(&settings_path)?;
        configured_provider = settings
            .get("defaultProvider")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        configured_model = settings
            .get("defaultModel")
            .and_then(Value::as_str)
            .map(ToString::to_string);
    }

    let models_path = pi_models_path();
    if models_path.exists() {
        let data: Value = read_json_file(&models_path)?;
        if let Some(providers) = data.get("providers").and_then(Value::as_object) {
            for (key, value) in providers {
                let name = key.trim();
                if name.is_empty() || provider_choices.iter().any(|item| item == name) {
                    continue;
                }
                provider_choices.push(name.to_string());
                if configured_provider.as_deref().is_some()
                    && configured_provider.as_deref() != Some(name)
                {
                    continue;
                }
                for row in value
                    .get("models")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(model_id) = row.get("id").and_then(Value::as_str) {
                        if !model_choices.iter().any(|item| item == model_id) {
                            model_choices.push(model_id.to_string());
                        }
                    }
                }
            }
        }
    }

    let auth_path = pi_auth_path();
    for provider in BUILTIN_PI_PROVIDER_CHOICES {
        if !provider_choices.iter().any(|item| item == provider) {
            provider_choices.push((*provider).to_string());
        }
    }

    if auth_path.exists() {
        let data: Value = read_json_file(&auth_path)?;
        if let Some(entries) = data.as_object() {
            for (key, value) in entries {
                let auth_type = value.get("type").and_then(Value::as_str);
                let access = value.get("access").and_then(Value::as_str);
                let refresh = value.get("refresh").and_then(Value::as_str);
                if auth_type == Some("oauth")
                    && (access.is_some() || refresh.is_some())
                    && !provider_choices.iter().any(|item| item == key)
                {
                    provider_choices.push(key.to_string());
                }
            }
        }
    }

    if let Some(provider) = configured_provider.clone() {
        if !provider_choices.iter().any(|item| item == &provider) {
            provider_choices.insert(0, provider);
        }
    }
    if let Some(model) = configured_model.clone() {
        if !model_choices.iter().any(|item| item == &model) {
            model_choices.insert(0, model);
        }
    }

    Ok(ApiBackendDefaults {
        agent_backend: "pi".to_string(),
        model_provider: configured_provider.clone(),
        preferred_auth_method: None,
        provider_choice: configured_provider,
        provider_choices,
        model: configured_model,
        models: model_choices,
        reasoning_effort: configured_effort,
        reasoning_efforts: SUPPORTED_PI_REASONING_EFFORTS
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        service_tier: None,
        supports_fast: false,
    })
}

fn codex_home() -> PathBuf {
    env::var("CODEX_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".codex")
        })
}

fn pi_home() -> PathBuf {
    env::var("PI_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".pi"))
}

fn codex_config_path() -> PathBuf {
    codex_home().join("config.toml")
}

fn push_subscriptions_path(config: &RuntimeConfig) -> PathBuf {
    config.app_dir.join("push_subscriptions.json")
}

fn voice_settings_path(config: &RuntimeConfig) -> PathBuf {
    config.app_dir.join("voice_settings.json")
}

fn voice_delivery_ledger_path(config: &RuntimeConfig) -> PathBuf {
    config.app_dir.join("voice_delivery_ledger.json")
}

fn voice_runtime_path(config: &RuntimeConfig) -> PathBuf {
    config.app_dir.join("voice_runtime.json")
}

fn voice_listeners_path(config: &RuntimeConfig) -> PathBuf {
    config.app_dir.join("voice_listeners.json")
}

fn audio_root_dir(config: &RuntimeConfig) -> PathBuf {
    config.app_dir.join("audio")
}

fn audio_playlist_path(config: &RuntimeConfig) -> PathBuf {
    audio_root_dir(config).join("live.m3u8")
}

fn audio_segments_dir(config: &RuntimeConfig) -> PathBuf {
    audio_root_dir(config).join("segments")
}

fn vapid_private_key_path(config: &RuntimeConfig) -> PathBuf {
    config.app_dir.join("webpush_vapid_private.pem")
}

fn models_cache_path() -> PathBuf {
    codex_home().join("models_cache.json")
}

fn pi_settings_path() -> PathBuf {
    pi_home().join("agent").join("settings.json")
}

fn pi_models_path() -> PathBuf {
    pi_home().join("agent").join("models.json")
}

fn pi_auth_path() -> PathBuf {
    pi_home().join("agent").join("auth.json")
}

fn ensure_vapid_public_key(config: &RuntimeConfig) -> Result<String, String> {
    let path = vapid_private_key_path(config);
    if !path.exists() {
        let parent = path
            .parent()
            .ok_or_else(|| format!("missing parent for {}", path.display()))?;
        fs::create_dir_all(parent).map_err(|err| format!("create {}: {err}", parent.display()))?;
        let output = Command::new("openssl")
            .arg("genpkey")
            .arg("-algorithm")
            .arg("EC")
            .arg("-pkeyopt")
            .arg("ec_paramgen_curve:P-256")
            .arg("-out")
            .arg(&path)
            .output()
            .map_err(|err| format!("spawn openssl genpkey: {err}"))?;
        if !output.status.success() {
            return Err(format!(
                "openssl genpkey failed for {}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        #[allow(clippy::permissions_set_readonly_false)]
        {
            let mut permissions = fs::metadata(&path)
                .map_err(|err| format!("stat {}: {err}", path.display()))?
                .permissions();
            permissions.set_mode(0o600);
            fs::set_permissions(&path, permissions)
                .map_err(|err| format!("chmod {}: {err}", path.display()))?;
        }
    }
    let output = Command::new("openssl")
        .arg("pkey")
        .arg("-in")
        .arg(&path)
        .arg("-pubout")
        .arg("-outform")
        .arg("DER")
        .output()
        .map_err(|err| format!("spawn openssl pkey: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "openssl pkey failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let public_key = extract_ec_public_key_from_spki_der(&output.stdout)?;
    Ok(URL_SAFE_NO_PAD.encode(public_key))
}

fn extract_ec_public_key_from_spki_der(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut idx = 0_usize;
    let outer_end = der_expect_tag(raw, &mut idx, 0x30)?;
    let algorithm_end = der_expect_tag(raw, &mut idx, 0x30)?;
    idx = algorithm_end;
    if raw.get(idx) != Some(&0x03) {
        return Err("invalid DER public key: missing BIT STRING".to_string());
    }
    idx += 1;
    let bit_string_len = der_read_length(raw, &mut idx)?;
    let bit_string_end = idx
        .checked_add(bit_string_len)
        .ok_or_else(|| "invalid DER public key length".to_string())?;
    if bit_string_end > raw.len() || bit_string_end > outer_end {
        return Err("invalid DER public key: truncated BIT STRING".to_string());
    }
    if raw.get(idx) != Some(&0x00) {
        return Err("invalid DER public key: unsupported unused bits".to_string());
    }
    idx += 1;
    let point = raw[idx..bit_string_end].to_vec();
    if point.len() != 65 || point.first().copied() != Some(0x04) {
        return Err("invalid DER public key: expected uncompressed P-256 point".to_string());
    }
    Ok(point)
}

fn der_expect_tag(raw: &[u8], idx: &mut usize, tag: u8) -> Result<usize, String> {
    if raw.get(*idx) != Some(&tag) {
        return Err(format!("invalid DER tag: expected 0x{tag:02x}"));
    }
    *idx += 1;
    let len = der_read_length(raw, idx)?;
    let end = idx
        .checked_add(len)
        .ok_or_else(|| "invalid DER length overflow".to_string())?;
    if end > raw.len() {
        return Err("invalid DER length: truncated value".to_string());
    }
    Ok(end)
}

fn der_read_length(raw: &[u8], idx: &mut usize) -> Result<usize, String> {
    let Some(first) = raw.get(*idx).copied() else {
        return Err("invalid DER length: missing byte".to_string());
    };
    *idx += 1;
    if first & 0x80 == 0 {
        return Ok(first as usize);
    }
    let count = (first & 0x7f) as usize;
    if count == 0 || count > 4 {
        return Err("invalid DER length encoding".to_string());
    }
    let end = idx
        .checked_add(count)
        .ok_or_else(|| "invalid DER length overflow".to_string())?;
    if end > raw.len() {
        return Err("invalid DER length: truncated".to_string());
    }
    let mut len = 0_usize;
    for byte in &raw[*idx..end] {
        len = (len << 8) | usize::from(*byte);
    }
    *idx = end;
    Ok(len)
}

fn toml_string(value: &TomlValue) -> Option<String> {
    value
        .as_str()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn display_reasoning_effort(value: &str) -> Option<String> {
    let lowered = value.trim().to_ascii_lowercase();
    SUPPORTED_REASONING_EFFORTS
        .iter()
        .find(|candidate| **candidate == lowered)
        .map(|candidate| (*candidate).to_string())
}

fn display_pi_reasoning_effort(value: &str) -> Option<String> {
    let lowered = value.trim().to_ascii_lowercase();
    SUPPORTED_PI_REASONING_EFFORTS
        .iter()
        .find(|candidate| **candidate == lowered)
        .map(|candidate| (*candidate).to_string())
}

fn configured_model_providers(table: &toml::map::Map<String, TomlValue>) -> Vec<String> {
    let mut providers = vec!["openai".to_string()];
    let Some(raw) = table.get("model_providers").and_then(TomlValue::as_table) else {
        return providers;
    };
    for key in raw.keys() {
        let name = key.trim();
        if !name.is_empty() && !providers.iter().any(|item| item == name) {
            providers.push(name.to_string());
        }
    }
    providers
}

fn allowed_model_providers(provider_choices: &[String]) -> HashSet<String> {
    let mut out = HashSet::from(["openai".to_string()]);
    for value in provider_choices {
        if value != "chatgpt" && value != "openai-api" {
            out.insert(value.clone());
        }
    }
    out
}

fn normalize_requested_model_provider(
    value: Option<&str>,
    allowed: Option<&HashSet<String>>,
) -> Result<Option<String>, String> {
    let provider = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let Some(provider) = provider else {
        return Ok(None);
    };
    if let Some(allowed_set) = allowed {
        if !allowed_set.contains(&provider) {
            let mut allowed_values = allowed_set.iter().cloned().collect::<Vec<_>>();
            allowed_values.sort();
            return Err(format!(
                "model_provider must be one of {}",
                allowed_values.join(", ")
            ));
        }
    }
    Ok(Some(provider))
}

fn normalize_requested_preferred_auth_method(
    value: Option<&str>,
) -> Result<Option<String>, String> {
    let method = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let Some(method) = method else {
        return Ok(None);
    };
    if method != "chatgpt" && method != "apikey" {
        return Err("preferred_auth_method must be one of chatgpt, apikey".to_string());
    }
    Ok(Some(method))
}

fn normalize_requested_service_tier(value: Option<&str>) -> Result<Option<String>, String> {
    let tier = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let Some(tier) = tier else {
        return Ok(None);
    };
    if tier != "fast" && tier != "flex" {
        return Err("service_tier must be one of fast, flex".to_string());
    }
    Ok(Some(tier))
}

fn normalize_requested_reasoning_effort(value: Option<&str>) -> Result<Option<String>, String> {
    let effort = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let Some(effort) = effort else {
        return Ok(None);
    };
    if display_reasoning_effort(&effort).is_none() {
        return Err(format!(
            "reasoning_effort must be one of {}",
            SUPPORTED_REASONING_EFFORTS.join(", ")
        ));
    }
    Ok(Some(effort))
}

fn apply_codex_launch_default_env_overrides(
    defaults: &mut ApiBackendDefaults,
) -> Result<(), String> {
    let allowed = allowed_model_providers(&defaults.provider_choices);
    if let Some(model_provider) = normalize_requested_model_provider(
        env::var("CODEX_WEB_DEFAULT_MODEL_PROVIDER").ok().as_deref(),
        Some(&allowed),
    )? {
        defaults.model_provider = Some(model_provider);
    }
    if let Some(method) = normalize_requested_preferred_auth_method(
        env::var("CODEX_WEB_DEFAULT_PREFERRED_AUTH_METHOD")
            .ok()
            .as_deref(),
    )? {
        defaults.preferred_auth_method = Some(method);
    }
    if let Some(model) = env::var("CODEX_WEB_DEFAULT_MODEL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        defaults.model = Some(model);
    }
    if let Some(effort) = normalize_requested_reasoning_effort(
        env::var("CODEX_WEB_DEFAULT_REASONING_EFFORT")
            .ok()
            .as_deref(),
    )? {
        defaults.reasoning_effort = effort;
    }
    if let Some(tier) = normalize_requested_service_tier(
        env::var("CODEX_WEB_DEFAULT_SERVICE_TIER").ok().as_deref(),
    )? {
        defaults.service_tier = Some(tier);
    }
    Ok(())
}

fn tmux_available() -> bool {
    spawn_command("tmux")
        .arg("-V")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        load_changed_files_response, load_file_search_response, load_git_diff_response,
        load_git_file_versions_response, load_harness_response, load_messages_history,
        load_messages_live, load_messages_tail, load_sessions_response, run_harness_sweep_once,
        run_queue_sweep_once, run_voice_scan_once, rust_broker_bin, voice_text_message_id,
        RuntimeConfig,
    };
    use serde_json::Value;
    use std::collections::HashMap;
    use std::env;
    use std::fs;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::{Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

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

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn write_voice_scan_session(app_dir: &Path, session_id: &str, log_path: &Path) {
        fs::write(app_dir.join("socks").join(format!("{session_id}.sock")), "").unwrap();
        fs::write(
            app_dir.join("socks").join(format!("{session_id}.json")),
            format!(
                r#"{{
                  "session_id": "thread-{session_id}",
                  "codex_pid": {},
                  "broker_pid": {},
                  "agent_backend": "codex",
                  "owner": "web",
                  "cwd": "/work/project",
                  "log_path": {},
                  "start_ts": 10.0,
                  "updated_ts": 15.0
                }}"#,
                std::process::id(),
                std::process::id(),
                serde_json::to_string(log_path.to_str().unwrap()).unwrap()
            ),
        )
        .unwrap();
    }

    fn read_voice_delivery_ledger_for_test(app_dir: &Path) -> Value {
        serde_json::from_str(
            &fs::read_to_string(app_dir.join("voice_delivery_ledger.json")).unwrap(),
        )
        .unwrap()
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
              "workspace_cwd": "/work",
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
        fs::write(
            app_dir.join("session_aliases.json"),
            r#"{"sid-a":"Alias A"}"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("session_queues.json"),
            r#"{"sid-a":[{"id":"q1","text":"next"}]}"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("harness.json"),
            r#"{"sid-a":{"enabled":true,"cooldown_minutes":3.5,"remaining_injections":2}}"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("session_files.json"),
            r#"{"sid-a":["README.md"]}"#,
        )
        .unwrap();

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
        assert_eq!(session.workspace_cwd.as_deref(), Some("/work"));
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
    fn voice_scan_writes_new_delivery_messages_to_inbox() {
        let app_dir = temp_app_dir("voice-scan");
        let log_path = app_dir.join("rollout-voice.jsonl");
        fs::write(
            app_dir.join("voice_settings.json"),
            r#"{"tts_api_key":"token"}"#,
        )
        .unwrap();
        fs::write(
            &log_path,
            r#"{"type":"event_msg","timestamp":"2026-04-28T00:00:00Z","payload":{"type":"agent_message","message":"old final","phase":"final_answer"}}
"#,
        )
        .unwrap();
        write_voice_scan_session(&app_dir, "sid-voice", &log_path);

        let config = RuntimeConfig {
            app_dir: app_dir.clone(),
        };
        let mut offsets = HashMap::new();
        assert_eq!(run_voice_scan_once(&config, &mut offsets).unwrap(), 0);
        {
            let mut handle = fs::OpenOptions::new().append(true).open(&log_path).unwrap();
            writeln!(
                handle,
                r#"{{"type":"event_msg","timestamp":"2026-04-28T00:00:01Z","payload":{{"type":"agent_message","message":"new final","phase":"final_answer"}}}}"#
            )
            .unwrap();
        }

        assert_eq!(run_voice_scan_once(&config, &mut offsets).unwrap(), 1);
        let entries = fs::read_dir(app_dir.join("voice_inbox"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(entries.len(), 1);
        let payload: Value =
            serde_json::from_str(&fs::read_to_string(entries[0].path()).unwrap()).unwrap();
        assert_eq!(
            payload.get("session_id").and_then(Value::as_str),
            Some("sid-voice")
        );
        assert_eq!(
            payload.get("session_display_name").and_then(Value::as_str),
            Some("Session")
        );
        assert_eq!(
            payload.get("message_class").and_then(Value::as_str),
            Some("final_response")
        );
        assert_eq!(
            payload.get("text").and_then(Value::as_str),
            Some("new final")
        );
    }

    #[test]
    fn voice_scan_records_raw_final_response_locally_without_tts_or_push() {
        let app_dir = temp_app_dir("voice-scan-local-final");
        let log_path = app_dir.join("rollout-voice.jsonl");
        fs::write(
            app_dir.join("voice_settings.json"),
            r#"{"tts_enabled_for_narration":false,"tts_enabled_for_final_response":false,"tts_api_key":""}"#,
        )
        .unwrap();
        fs::write(
            &log_path,
            r#"{"type":"event_msg","timestamp":"2026-04-28T00:00:00Z","payload":{"type":"agent_message","message":"old final","phase":"final_answer"}}
"#,
        )
        .unwrap();
        write_voice_scan_session(&app_dir, "sid-voice", &log_path);

        let config = RuntimeConfig {
            app_dir: app_dir.clone(),
        };
        let mut offsets = HashMap::new();
        assert_eq!(run_voice_scan_once(&config, &mut offsets).unwrap(), 0);
        {
            let mut handle = fs::OpenOptions::new().append(true).open(&log_path).unwrap();
            writeln!(
                handle,
                r#"{{"type":"event_msg","timestamp":"2026-04-28T00:00:01Z","payload":{{"type":"agent_message","message":"Line one.\n\nLine two.","phase":"final_answer"}}}}"#
            )
            .unwrap();
        }

        assert_eq!(run_voice_scan_once(&config, &mut offsets).unwrap(), 1);
        assert!(!app_dir.join("voice_inbox").exists());
        let ledger = read_voice_delivery_ledger_for_test(&app_dir);
        let rows = ledger.as_object().unwrap();
        assert_eq!(rows.len(), 1);
        let row = rows.values().next().unwrap();
        assert_eq!(
            row.get("message_class").and_then(Value::as_str),
            Some("final_response")
        );
        assert_eq!(
            row.get("notification_text").and_then(Value::as_str),
            Some("Line one. Line two.")
        );
        assert_eq!(
            row.get("summary_status").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            row.get("push_status").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            row.get("narrated_status").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            row.get("session_display_name").and_then(Value::as_str),
            Some("Session")
        );
    }

    #[test]
    fn voice_scan_keeps_python_inbox_when_mobile_push_requires_worker() {
        let app_dir = temp_app_dir("voice-scan-mobile-push");
        let log_path = app_dir.join("rollout-voice.jsonl");
        fs::write(
            app_dir.join("push_subscriptions.json"),
            r#"[{"subscription":{"endpoint":"https://push.example/a","keys":{"p256dh":"p","auth":"a"}},"notifications_enabled":true,"device_class":"mobile"}]"#,
        )
        .unwrap();
        fs::write(
            &log_path,
            r#"{"type":"event_msg","timestamp":"2026-04-28T00:00:00Z","payload":{"type":"agent_message","message":"old final","phase":"final_answer"}}
"#,
        )
        .unwrap();
        write_voice_scan_session(&app_dir, "sid-voice", &log_path);

        let config = RuntimeConfig {
            app_dir: app_dir.clone(),
        };
        let mut offsets = HashMap::new();
        assert_eq!(run_voice_scan_once(&config, &mut offsets).unwrap(), 0);
        {
            let mut handle = fs::OpenOptions::new().append(true).open(&log_path).unwrap();
            writeln!(
                handle,
                r#"{{"type":"event_msg","timestamp":"2026-04-28T00:00:01Z","payload":{{"type":"agent_message","message":"push final","phase":"final_answer"}}}}"#
            )
            .unwrap();
        }

        assert_eq!(run_voice_scan_once(&config, &mut offsets).unwrap(), 1);
        let entries = fs::read_dir(app_dir.join("voice_inbox"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!app_dir.join("voice_delivery_ledger.json").exists());
    }

    #[test]
    fn voice_scan_records_disabled_narration_as_skipped_without_inbox() {
        let app_dir = temp_app_dir("voice-scan-local-narration");
        let log_path = app_dir.join("rollout-voice.jsonl");
        fs::write(
            app_dir.join("voice_settings.json"),
            r#"{"tts_enabled_for_narration":false,"tts_enabled_for_final_response":false,"tts_api_key":""}"#,
        )
        .unwrap();
        fs::write(
            &log_path,
            r#"{"type":"event_msg","timestamp":"2026-04-28T00:00:00Z","payload":{"type":"agent_message","message":"old narration"}}
"#,
        )
        .unwrap();
        write_voice_scan_session(&app_dir, "sid-voice", &log_path);

        let config = RuntimeConfig {
            app_dir: app_dir.clone(),
        };
        let mut offsets = HashMap::new();
        assert_eq!(run_voice_scan_once(&config, &mut offsets).unwrap(), 0);
        {
            let mut handle = fs::OpenOptions::new().append(true).open(&log_path).unwrap();
            writeln!(
                handle,
                r#"{{"type":"event_msg","timestamp":"2026-04-28T00:00:01Z","payload":{{"type":"agent_message","message":"working update"}}}}"#
            )
            .unwrap();
        }

        assert_eq!(run_voice_scan_once(&config, &mut offsets).unwrap(), 1);
        assert!(!app_dir.join("voice_inbox").exists());
        let ledger = read_voice_delivery_ledger_for_test(&app_dir);
        let rows = ledger.as_object().unwrap();
        assert_eq!(rows.len(), 1);
        let row = rows.values().next().unwrap();
        assert_eq!(
            row.get("message_class").and_then(Value::as_str),
            Some("narration")
        );
        assert_eq!(
            row.get("summary_status").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            row.get("narrated_status").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            row.get("push_status").and_then(Value::as_str),
            Some("skipped")
        );
    }

    #[test]
    fn voice_delivery_message_id_matches_python_payload_shape() {
        let id = voice_text_message_id("final_response", "hello   world", Some(1770000000.25));
        assert_eq!(
            id,
            "6dfdb63d008010af119b758cf1c864942eedcbf4f33db478a27859ca557112f7"
        );
    }

    #[test]
    fn rust_broker_bin_uses_explicit_path_and_ignores_legacy_disable_flag() {
        let _guard = env_lock().lock().unwrap();
        env::set_var("CODOXEAR_ENABLE_RUST_BROKER", "0");
        env::set_var("CODOXEAR_RUST_BROKER_BIN", "/tmp/custom-codoxear-broker-rs");
        assert_eq!(
            rust_broker_bin(Path::new("/repo")),
            PathBuf::from("/tmp/custom-codoxear-broker-rs")
        );
        env::remove_var("CODOXEAR_ENABLE_RUST_BROKER");
        env::remove_var("CODOXEAR_RUST_BROKER_BIN");
    }

    #[test]
    fn load_sessions_skips_stale_dead_sockets_and_reads_live_defaults() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("live-defaults");
        let codex_home = app_dir.join("codex-home");
        let pi_home = app_dir.join("pi-home");
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        fs::create_dir_all(pi_home.join("agent")).unwrap();
        fs::write(
            codex_home.join("config.toml"),
            r#"
model = "gpt-5.4"
model_provider = "crs"
preferred_auth_method = "apikey"
service_tier = "fast"

[model_providers.crs]
name = "CRS"
"#,
        )
        .unwrap();
        fs::write(
            codex_home.join("models_cache.json"),
            r#"{"models":[{"slug":"gpt-5.4","default_reasoning_level":"medium","priority":1}]}"#,
        )
        .unwrap();
        fs::write(
            pi_home.join("agent").join("settings.json"),
            r#"{"defaultProvider":"macaron","defaultModel":"gpt-5.4"}"#,
        )
        .unwrap();
        fs::write(
            pi_home.join("agent").join("models.json"),
            r#"{"providers":{"macaron":{"models":[{"id":"gpt-5.4","contextWindow":200000}]}}}"#,
        )
        .unwrap();
        fs::write(
            pi_home.join("agent").join("auth.json"),
            r#"{"openai-codex":{"type":"oauth","access":"abc"}}"#,
        )
        .unwrap();

        let prev_codex_home = env::var("CODEX_HOME").ok();
        let prev_pi_home = env::var("PI_HOME").ok();
        let prev_default_backend = env::var("CODEX_WEB_DEFAULT_AGENT_BACKEND").ok();
        env::set_var("CODEX_HOME", &codex_home);
        env::set_var("PI_HOME", &pi_home);
        env::set_var("CODEX_WEB_DEFAULT_AGENT_BACKEND", "pi");

        let live_sock = app_dir.join("socks").join("sid-live.sock");
        let listener = UnixListener::bind(&live_sock).unwrap();
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            assert_eq!(line.trim(), "{\"cmd\":\"state\"}");
            stream
                .write_all(b"{\"busy\":true,\"queue_len\":0,\"token\":{\"context_window\":128000,\"tokens_in_context\":64000,\"percent_remaining\":50}}\n")
                .unwrap();
        });

        let log_path = app_dir.join("live-rollout.jsonl");
        fs::write(
            &log_path,
            [
                r#"{"type":"session_meta","payload":{"model_provider":"crs","model":"gpt-5.4","reasoning_effort":"medium"}}"#,
                r#"{"type":"event_msg","payload":{"type":"user_message","message":"hello"},"ts":2.0}"#,
                r#"{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"done"},"ts":4.0}"#,
            ]
            .join("\n")
                + "\n",
        )
        .unwrap();
        fs::write(
            app_dir.join("socks").join("sid-live.json"),
            format!(
                r#"{{"session_id":"thread-live","codex_pid":{},"broker_pid":{},"agent_backend":"codex","owner":"web","cwd":"{}","log_path":"{}","start_ts":1.0}}"#,
                std::process::id(),
                std::process::id(),
                app_dir.display(),
                log_path.display(),
            ),
        )
        .unwrap();
        fs::write(
            app_dir.join("session_queues.json"),
            r#"{"sid-live":[{"id":"q1","text":"queued"}],"sid-dead":[{"id":"q2","text":"dead"}]}"#,
        )
        .unwrap();

        fs::write(app_dir.join("socks").join("sid-dead.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-dead.json"),
            r#"{"session_id":"thread-dead","codex_pid":999999,"broker_pid":999998,"cwd":"/tmp","start_ts":1.0}"#,
        )
        .unwrap();

        let response = load_sessions_response(&RuntimeConfig {
            app_dir: app_dir.clone(),
        })
        .unwrap();
        thread.join().unwrap();

        match prev_codex_home {
            Some(value) => env::set_var("CODEX_HOME", value),
            None => env::remove_var("CODEX_HOME"),
        }
        match prev_pi_home {
            Some(value) => env::set_var("PI_HOME", value),
            None => env::remove_var("PI_HOME"),
        }
        match prev_default_backend {
            Some(value) => env::set_var("CODEX_WEB_DEFAULT_AGENT_BACKEND", value),
            None => env::remove_var("CODEX_WEB_DEFAULT_AGENT_BACKEND"),
        }

        assert_eq!(response.sessions.len(), 1);
        let session = &response.sessions[0];
        assert_eq!(session.session_id, "sid-live");
        assert_eq!(session.updated_ts, 4.0);
        assert!(!session.busy);
        assert_eq!(session.token.as_ref().unwrap()["context_window"], 128000);
        assert_eq!(response.new_session_defaults.default_backend, "pi");
        assert_eq!(
            response.new_session_defaults.backends["codex"]
                .provider_choice
                .as_deref(),
            Some("crs")
        );
        assert_eq!(
            response.new_session_defaults.backends["codex"].reasoning_effort,
            "medium"
        );
        assert_eq!(
            response.new_session_defaults.backends["pi"]
                .provider_choice
                .as_deref(),
            Some("macaron")
        );
        assert_eq!(
            response.new_session_defaults.backends["pi"].provider_choices,
            vec![
                "macaron".to_string(),
                "anthropic".to_string(),
                "openai-codex".to_string(),
                "github-copilot".to_string(),
                "google-gemini-cli".to_string(),
                "google-antigravity".to_string(),
            ],
        );
    }

    #[test]
    fn load_sessions_discovers_missing_log_path_from_live_process() {
        let _guard = env_lock().lock().unwrap();
        let app_dir = temp_app_dir("discover-log");
        let codex_home = app_dir.join("codex-home");
        let pi_home = app_dir.join("pi-home");
        fs::create_dir_all(codex_home.join("sessions")).unwrap();
        fs::create_dir_all(pi_home.join("agent")).unwrap();
        fs::write(
            codex_home.join("config.toml"),
            r#"
model_provider = "crs"

[model_providers.crs]
name = "CRS"
"#,
        )
        .unwrap();
        fs::write(
            pi_home.join("agent").join("settings.json"),
            r#"{"defaultProvider":"macaron"}"#,
        )
        .unwrap();
        fs::write(
            pi_home.join("agent").join("models.json"),
            r#"{"providers":{"macaron":{"models":[{"id":"gpt-5.4","contextWindow":200000}]}}}"#,
        )
        .unwrap();

        let prev_codex_home = env::var("CODEX_HOME").ok();
        let prev_pi_home = env::var("PI_HOME").ok();
        env::set_var("CODEX_HOME", &codex_home);
        env::set_var("PI_HOME", &pi_home);

        let log_path = codex_home.join("sessions").join("rollout-discovered.jsonl");
        fs::write(
            &log_path,
            [
                format!(
                    r#"{{"type":"session_meta","payload":{{"cwd":"{}","model_provider":"crs","model":"gpt-5.5","reasoning_effort":"high"}}}}"#,
                    app_dir.display(),
                ),
                r#"{"type":"event_msg","payload":{"type":"user_message","message":"hello"},"ts":2.0}"#.to_string(),
                r#"{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"done"},"ts":3.0}"#.to_string(),
            ]
            .join("\n")
                + "\n",
        )
        .unwrap();

        let mut child = Command::new("/bin/bash")
            .arg("-lc")
            .arg("exec 3>>\"$1\"; sleep 30")
            .arg("_")
            .arg(&log_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        thread::sleep(Duration::from_millis(150));

        fs::write(app_dir.join("socks").join("sid-open.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-open.json"),
            format!(
                r#"{{"session_id":"thread-open","codex_pid":{},"broker_pid":{},"agent_backend":"codex","owner":"web","cwd":"{}","start_ts":1.0}}"#,
                child.id(),
                child.id(),
                app_dir.display(),
            ),
        )
        .unwrap();

        let response = load_sessions_response(&RuntimeConfig {
            app_dir: app_dir.clone(),
        })
        .unwrap();

        let _ = child.kill();
        let _ = child.wait();

        match prev_codex_home {
            Some(value) => env::set_var("CODEX_HOME", value),
            None => env::remove_var("CODEX_HOME"),
        }
        match prev_pi_home {
            Some(value) => env::set_var("PI_HOME", value),
            None => env::remove_var("PI_HOME"),
        }

        assert_eq!(response.sessions.len(), 1);
        let session = &response.sessions[0];
        assert_eq!(session.session_id, "sid-open");
        assert_eq!(
            session.log_path.as_deref(),
            Some(log_path.to_string_lossy().as_ref())
        );
        assert_eq!(session.updated_ts, 3.0);
        assert!(!session.busy);
        assert_eq!(session.model_provider.as_deref(), Some("crs"));
        assert_eq!(session.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(session.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn file_search_uses_git_listing_when_repo_exists() {
        let app_dir = temp_app_dir("file-search-git");
        let repo_dir = app_dir.join("repo");
        fs::create_dir_all(repo_dir.join("src")).unwrap();
        fs::write(repo_dir.join("src").join("notes.txt"), "tracked note\n").unwrap();
        fs::write(repo_dir.join("notes-draft.md"), "draft note\n").unwrap();
        fs::write(repo_dir.join(".gitignore"), "ignored.log\n").unwrap();
        fs::write(repo_dir.join("ignored.log"), "ignore me\n").unwrap();
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["add", ".gitignore", "src/notes.txt"])
            .status()
            .unwrap()
            .success());
        fs::write(app_dir.join("socks").join("sid-search.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-search.json"),
            format!(
                r#"{{"session_id":"thread-search","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":1.0}}"#,
                std::process::id(),
                std::process::id(),
                repo_dir.display(),
            ),
        )
        .unwrap();

        let response =
            load_file_search_response(&RuntimeConfig { app_dir }, "sid-search", "notes", 10)
                .unwrap();

        assert_eq!(response.mode, "git");
        assert!(!response.truncated);
        assert!(response
            .matches
            .iter()
            .any(|entry| entry.path == "src/notes.txt"));
        assert!(response
            .matches
            .iter()
            .any(|entry| entry.path == "notes-draft.md"));
        assert!(!response
            .matches
            .iter()
            .any(|entry| entry.path == "ignored.log"));
    }

    #[test]
    fn file_search_walk_ignores_default_directories() {
        let app_dir = temp_app_dir("file-search-walk");
        let repo_dir = app_dir.join("workspace");
        fs::create_dir_all(repo_dir.join("node_modules")).unwrap();
        fs::write(repo_dir.join("notes.txt"), "visible\n").unwrap();
        fs::write(
            repo_dir.join("node_modules").join("notes-hidden.txt"),
            "hidden\n",
        )
        .unwrap();
        fs::write(app_dir.join("socks").join("sid-walk.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-walk.json"),
            format!(
                r#"{{"session_id":"thread-walk","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":1.0}}"#,
                std::process::id(),
                std::process::id(),
                repo_dir.display(),
            ),
        )
        .unwrap();

        let response =
            load_file_search_response(&RuntimeConfig { app_dir }, "sid-walk", "notes", 10).unwrap();

        assert_eq!(response.mode, "walk");
        assert_eq!(response.matches.len(), 1);
        assert_eq!(response.matches[0].path, "notes.txt");
    }

    #[test]
    fn changed_files_reads_git_status_and_merged_numstat() {
        let app_dir = temp_app_dir("changed-files");
        let repo_dir = app_dir.join("repo");
        fs::create_dir_all(&repo_dir).unwrap();
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap()
            .success());

        fs::write(repo_dir.join("notes.txt"), "base\n").unwrap();
        fs::write(repo_dir.join("mod.txt"), "keep\n").unwrap();
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["add", "notes.txt", "mod.txt"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["commit", "-qm", "init"])
            .status()
            .unwrap()
            .success());

        fs::write(repo_dir.join("notes.txt"), "base\nstaged\n").unwrap();
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["add", "notes.txt"])
            .status()
            .unwrap()
            .success());
        fs::write(repo_dir.join("notes.txt"), "base\nstaged\nunstaged\n").unwrap();
        fs::write(repo_dir.join("mod.txt"), "keep\nedit\n").unwrap();
        fs::write(repo_dir.join("staged.txt"), "staged only\n").unwrap();
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["add", "staged.txt"])
            .status()
            .unwrap()
            .success());

        fs::write(app_dir.join("socks").join("sid-git.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-git.json"),
            format!(
                r#"{{"session_id":"thread-git","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":1.0}}"#,
                std::process::id(),
                std::process::id(),
                repo_dir.display(),
            ),
        )
        .unwrap();

        let response = load_changed_files_response(&RuntimeConfig { app_dir }, "sid-git").unwrap();

        assert!(response.unstaged.iter().any(|path| path == "notes.txt"));
        assert!(response.unstaged.iter().any(|path| path == "mod.txt"));
        assert!(response.staged.iter().any(|path| path == "notes.txt"));
        assert!(response.staged.iter().any(|path| path == "staged.txt"));
        assert!(response.files.iter().any(|path| path == "notes.txt"));
        assert!(response.files.iter().any(|path| path == "mod.txt"));
        assert!(response.files.iter().any(|path| path == "staged.txt"));

        let notes = response
            .entries
            .iter()
            .find(|entry| entry.path == "notes.txt")
            .unwrap();
        assert_eq!(notes.additions, Some(2));
        assert_eq!(notes.deletions, Some(0));
        let staged = response
            .entries
            .iter()
            .find(|entry| entry.path == "staged.txt")
            .unwrap();
        assert_eq!(staged.additions, Some(1));
        assert_eq!(staged.deletions, Some(0));
    }

    #[test]
    fn git_diff_reads_staged_and_unstaged_content() {
        let app_dir = temp_app_dir("git-diff");
        let repo_dir = app_dir.join("repo");
        fs::create_dir_all(&repo_dir).unwrap();
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap()
            .success());

        fs::write(repo_dir.join("notes.txt"), "base\n").unwrap();
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["add", "notes.txt"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["commit", "-qm", "init"])
            .status()
            .unwrap()
            .success());

        fs::write(repo_dir.join("notes.txt"), "base\nstaged\n").unwrap();
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["add", "notes.txt"])
            .status()
            .unwrap()
            .success());
        fs::write(repo_dir.join("notes.txt"), "base\nstaged\nunstaged\n").unwrap();

        fs::write(app_dir.join("socks").join("sid-diff.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-diff.json"),
            format!(
                r#"{{"session_id":"thread-diff","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":1.0}}"#,
                std::process::id(),
                std::process::id(),
                repo_dir.display(),
            ),
        )
        .unwrap();

        let unstaged = load_git_diff_response(
            &RuntimeConfig {
                app_dir: app_dir.clone(),
            },
            "sid-diff",
            "notes.txt",
            false,
        )
        .unwrap();
        assert_eq!(unstaged.path, "notes.txt");
        assert!(!unstaged.staged);
        assert!(unstaged.diff.contains("+unstaged"));
        assert!(!unstaged.diff.contains("path is outside git repo"));

        let staged =
            load_git_diff_response(&RuntimeConfig { app_dir }, "sid-diff", "notes.txt", true)
                .unwrap();
        assert!(staged.staged);
        assert!(staged.diff.contains("+staged"));
        assert!(!staged.diff.contains("+unstaged"));
    }

    #[test]
    fn git_file_versions_reads_head_and_worktree_text() {
        let app_dir = temp_app_dir("git-file-versions");
        let repo_dir = app_dir.join("repo");
        fs::create_dir_all(&repo_dir).unwrap();
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap()
            .success());

        fs::write(repo_dir.join("notes.txt"), "base\n").unwrap();
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["add", "notes.txt"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(&repo_dir)
            .args(["commit", "-qm", "init"])
            .status()
            .unwrap()
            .success());

        fs::write(repo_dir.join("notes.txt"), "base\nworktree\n").unwrap();

        fs::write(app_dir.join("socks").join("sid-versions.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-versions.json"),
            format!(
                r#"{{"session_id":"thread-versions","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":1.0}}"#,
                std::process::id(),
                std::process::id(),
                repo_dir.display(),
            ),
        )
        .unwrap();

        let response = load_git_file_versions_response(
            &RuntimeConfig { app_dir },
            "sid-versions",
            "notes.txt",
        )
        .unwrap();

        assert_eq!(response.path, "notes.txt");
        assert!(response.current_exists);
        assert_eq!(response.current_size, 14);
        assert_eq!(response.current_text, "base\nworktree\n");
        assert!(response.base_exists);
        assert_eq!(response.base_text, "base\n");
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

        let history = load_messages_history(
            &config,
            "sid-msg",
            tail.history_cursor.as_deref().unwrap(),
            10,
        )
        .unwrap();
        assert_eq!(history.events.len(), 1);
        assert_eq!(history.events[0]["role"], "user");

        let live = load_messages_live(&config, "sid-msg", "0").unwrap();
        assert_eq!(live.events.len(), 3);
        assert_eq!(live.live_cursor, tail.live_cursor);
    }

    #[test]
    fn message_live_normalizes_codex_tool_extension_and_ask_user_events() {
        let app_dir = temp_app_dir("message-live-tools");
        let log_path = app_dir.join("rollout.jsonl");
        fs::write(
            &log_path,
            [
                r#"{"type":"response_item","payload":{"type":"function_call","name":"bash","call_id":"tool-1","arguments":{"command":"pwd"}},"ts":1.0}"#,
                r#"{"type":"response_item","payload":{"type":"function_call_output","name":"bash","call_id":"tool-1","output":"\/work"},"ts":2.0}"#,
                r#"{"type":"response_item","payload":{"type":"function_call","name":"update_plan","call_id":"plan-1","arguments":{"plan":[{"step":"Inspect logs","status":"completed"},{"step":"Patch parser","status":"in_progress"}]}},"ts":3.0}"#,
                r#"{"type":"response_item","payload":{"type":"function_call","name":"ask_user","call_id":"ask-1","arguments":{"question":"Proceed?","options":["Yes","No"]}},"ts":4.0}"#,
                r#"{"type":"response_item","payload":{"type":"function_call_output","name":"ask_user","call_id":"ask-1","details":{"answer":"Yes","wasCustom":false}},"ts":5.0}"#,
            ]
            .join("\n")
                + "\n",
        )
        .unwrap();
        fs::write(app_dir.join("socks").join("sid-tools.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-tools.json"),
            format!(
                r#"{{"session_id":"thread-tools","codex_pid":1,"broker_pid":2,"cwd":"/work","log_path":"{}","start_ts":1.0}}"#,
                log_path.display()
            ),
        )
        .unwrap();
        let config = RuntimeConfig { app_dir };

        let live = load_messages_live(&config, "sid-tools", "0").unwrap();
        assert_eq!(live.events.len(), 5);
        assert_eq!(live.events[0]["type"], "tool");
        assert_eq!(live.events[0]["text"], "pwd");
        assert_eq!(live.events[1]["type"], "tool_result");
        assert_eq!(live.events[1]["text"], "/work");
        assert_eq!(live.events[2]["type"], "extension");
        assert_eq!(live.events[2]["title"], "Todo");
        assert_eq!(live.events[2]["summary"], "1/2 completed");
        assert_eq!(live.events[3]["type"], "ask_user");
        assert_eq!(live.events[3]["resolved"], false);
        assert_eq!(live.events[4]["type"], "ask_user");
        assert_eq!(live.events[4]["resolved"], true);
        assert_eq!(live.events[4]["answer"], "Yes");
        assert_eq!(live.meta_delta["tool"], 5);
        assert_eq!(live.diag["last_tool"], "ask_user");
    }

    #[test]
    fn message_tail_and_history_keep_single_event_tool_pages() {
        let app_dir = temp_app_dir("message-tail-tools");
        let log_path = app_dir.join("rollout.jsonl");
        fs::write(
            &log_path,
            [
                r#"{"type":"response_item","payload":{"type":"function_call","name":"bash","call_id":"tool-1","arguments":{"command":"pwd"}},"ts":1.0}"#,
                r#"{"type":"response_item","payload":{"type":"function_call_output","name":"bash","call_id":"tool-1","output":"\/work"},"ts":2.0}"#,
                r#"{"type":"response_item","payload":{"type":"function_call","name":"update_plan","call_id":"plan-1","arguments":{"plan":[{"step":"Inspect logs","status":"completed"},{"step":"Patch parser","status":"in_progress"}]}},"ts":3.0}"#,
                r#"{"type":"response_item","payload":{"type":"function_call","name":"ask_user","call_id":"ask-1","arguments":{"question":"Proceed?","options":["Yes","No"]}},"ts":4.0}"#,
                r#"{"type":"response_item","payload":{"type":"function_call_output","name":"ask_user","call_id":"ask-1","details":{"answer":"Yes","wasCustom":false}},"ts":5.0}"#,
            ]
            .join("\n")
                + "\n",
        )
        .unwrap();
        fs::write(app_dir.join("socks").join("sid-tail.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-tail.json"),
            format!(
                r#"{{"session_id":"thread-tail","codex_pid":1,"broker_pid":2,"cwd":"/work","log_path":"{}","start_ts":1.0}}"#,
                log_path.display()
            ),
        )
        .unwrap();
        let config = RuntimeConfig { app_dir };

        let tail = load_messages_tail(&config, "sid-tail", 3).unwrap();
        assert_eq!(tail.events.len(), 3);
        assert_eq!(tail.events[0]["type"], "extension");
        assert_eq!(tail.events[1]["type"], "ask_user");
        assert_eq!(tail.events[2]["type"], "ask_user");
        assert!(tail.has_older);

        let history = load_messages_history(
            &config,
            "sid-tail",
            tail.history_cursor.as_deref().unwrap(),
            10,
        )
        .unwrap();
        assert_eq!(history.events.len(), 2);
        assert_eq!(history.events[0]["type"], "tool");
        assert_eq!(history.events[1]["type"], "tool_result");
    }

    #[test]
    fn message_live_normalizes_pi_ask_user_events_with_iso_timestamps() {
        let app_dir = temp_app_dir("message-live-pi");
        let log_path = app_dir.join("pi.jsonl");
        fs::write(
            &log_path,
            [
                r#"{"type":"message","timestamp":"2026-04-24T10:00:00Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"ask-4","name":"AskUserQuestion","arguments":{"questions":[{"header":"Testing","question":"How should we test this?","options":["Single","Freeform","Multiple"]}]}}]}}"#,
                r#"{"type":"message","timestamp":"2026-04-24T10:00:01Z","message":{"role":"toolResult","toolCallId":"ask-4","toolName":"AskUserQuestion","details":{"answer":"Single","wasCustom":false}}}"#,
            ]
            .join("\n")
                + "\n",
        )
        .unwrap();
        fs::write(app_dir.join("socks").join("sid-pi.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-pi.json"),
            format!(
                r#"{{"session_id":"thread-pi","codex_pid":1,"broker_pid":2,"cwd":"/work","log_path":"{}","start_ts":1.0,"agent_backend":"pi"}}"#,
                log_path.display()
            ),
        )
        .unwrap();
        let config = RuntimeConfig { app_dir };

        let live = load_messages_live(&config, "sid-pi", "0").unwrap();
        assert_eq!(live.events.len(), 2);
        assert_eq!(live.events[0]["type"], "ask_user");
        assert_eq!(live.events[0]["question"], "How should we test this?");
        assert!(live.events[0]["ts"].as_f64().unwrap() > 0.0);
        assert_eq!(live.events[1]["type"], "ask_user");
        assert_eq!(live.events[1]["resolved"], true);
        assert_eq!(live.events[1]["answer"], "Single");
        assert_eq!(live.meta_delta["tool"], 2);
        assert_eq!(live.diag["last_tool"], "pi_tool");
    }

    #[test]
    fn message_live_treats_pi_stop_message_with_thinking_as_final_response() {
        let app_dir = temp_app_dir("message-live-pi-final");
        let log_path = app_dir.join("pi-final.jsonl");
        fs::write(
            &log_path,
            [
                r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"test"}]},"timestamp":"2026-04-24T10:00:00Z"}"#,
                r#"{"type":"message","message":{"role":"assistant","stopReason":"stop","content":[{"type":"thinking","thinking":""},{"type":"text","text":"done","textSignature":"{\"v\":1,\"phase\":\"final_answer\"}"}]},"timestamp":"2026-04-24T10:00:01Z"}"#,
            ]
            .join("\n")
                + "\n",
        )
        .unwrap();
        fs::write(app_dir.join("socks").join("sid-pi-final.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-pi-final.json"),
            format!(
                r#"{{"session_id":"thread-pi-final","codex_pid":1,"broker_pid":2,"cwd":"/work","log_path":"{}","start_ts":1.0,"agent_backend":"pi"}}"#,
                log_path.display()
            ),
        )
        .unwrap();
        let config = RuntimeConfig { app_dir };

        let live = load_messages_live(&config, "sid-pi-final", "0").unwrap();
        assert_eq!(live.events.len(), 2);
        assert_eq!(live.events[1]["role"], "assistant");
        assert_eq!(live.events[1]["text"], "done");
        assert_eq!(live.events[1]["message_class"], "final_response");
        assert_eq!(live.meta_delta["thinking"], 1);
        assert_eq!(live.turn_end, true);
    }

    #[test]
    fn run_queue_sweep_once_waits_for_idle_grace_and_sends_head() {
        let _guard = env_lock().lock().unwrap();
        let previous_grace = env::var("CODEX_WEB_QUEUE_IDLE_GRACE_SECONDS").ok();
        env::set_var("CODEX_WEB_QUEUE_IDLE_GRACE_SECONDS", "5");

        let app_dir = temp_app_dir("queue-worker");
        let sock_path = app_dir.join("socks").join("sid-queue.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        let listener_thread = thread::spawn(move || {
            for _ in 0..5 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                if line.contains("\"cmd\":\"send\"") {
                    assert!(line.contains("queued from rust"));
                    stream
                        .write_all(b"{\"queued\":false,\"queue_len\":0}\n")
                        .unwrap();
                    return;
                }
                stream
                    .write_all(b"{\"busy\":false,\"queue_len\":0}\n")
                    .unwrap();
            }
            panic!("queue worker never sent queued head");
        });

        fs::write(
            app_dir.join("socks").join("sid-queue.json"),
            format!(
                r#"{{"session_id":"thread-queue","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":11.0}}"#,
                std::process::id(),
                std::process::id(),
                app_dir.display(),
            ),
        )
        .unwrap();
        fs::write(
            app_dir.join("session_queues.json"),
            r#"{"sid-queue":[{"id":"q1","text":"queued from rust","created_ts":1.0}]}"#,
        )
        .unwrap();

        let config = RuntimeConfig {
            app_dir: app_dir.clone(),
        };
        let mut idle_since = std::collections::HashMap::new();
        assert!(!run_queue_sweep_once(&config, &mut idle_since, 10.0).unwrap());
        assert_eq!(idle_since.get("sid-queue").copied(), Some(10.0));

        let queued: Value =
            serde_json::from_str(&fs::read_to_string(app_dir.join("session_queues.json")).unwrap())
                .unwrap();
        assert_eq!(queued["sid-queue"][0]["text"], "queued from rust");
        assert!(queued["sid-queue"][0].get("sending").is_none());

        assert!(run_queue_sweep_once(&config, &mut idle_since, 15.1).unwrap());
        assert!(idle_since.get("sid-queue").is_none());
        let queues_after_send: Value =
            serde_json::from_str(&fs::read_to_string(app_dir.join("session_queues.json")).unwrap())
                .unwrap();
        assert!(queues_after_send.get("sid-queue").is_none());

        listener_thread.join().unwrap();
        match previous_grace {
            Some(value) => env::set_var("CODEX_WEB_QUEUE_IDLE_GRACE_SECONDS", value),
            None => env::remove_var("CODEX_WEB_QUEUE_IDLE_GRACE_SECONDS"),
        }
    }

    #[test]
    fn load_harness_response_uses_python_defaults_when_fields_are_missing() {
        let app_dir = temp_app_dir("harness-defaults");
        fs::write(app_dir.join("socks").join("sid-harness.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-harness.json"),
            format!(
                r#"{{"session_id":"thread-harness","codex_pid":{},"broker_pid":{},"cwd":"{}","start_ts":11.0}}"#,
                std::process::id(),
                std::process::id(),
                app_dir.display(),
            ),
        )
        .unwrap();
        fs::write(
            app_dir.join("harness.json"),
            r#"{"sid-harness":{"enabled":true,"request":"keep going"}}"#,
        )
        .unwrap();

        let response = load_harness_response(&RuntimeConfig { app_dir }, "sid-harness").unwrap();
        assert!(response.enabled);
        assert_eq!(response.request, "keep going");
        assert_eq!(response.cooldown_minutes, 5.0);
        assert_eq!(response.remaining_injections, 10);
    }

    #[test]
    fn run_harness_sweep_once_injects_prompt_and_decrements_remaining() {
        let app_dir = temp_app_dir("harness-worker");
        let sock_path = app_dir.join("socks").join("sid-harness.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        let listener_thread = thread::spawn(move || {
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                if line.contains("\"cmd\":\"send\"") {
                    assert!(line.contains("Additional request from user: Keep going"));
                    stream
                        .write_all(b"{\"queued\":false,\"queue_len\":0}\n")
                        .unwrap();
                    return;
                }
                stream
                    .write_all(b"{\"busy\":false,\"queue_len\":0}\n")
                    .unwrap();
            }
            panic!("harness worker never injected prompt");
        });

        let log_path = app_dir.join("rollout.jsonl");
        fs::write(
            &log_path,
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"done"},"ts":100.0}
"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("socks").join("sid-harness.json"),
            format!(
                r#"{{"session_id":"thread-harness","codex_pid":{},"broker_pid":{},"cwd":"{}","log_path":"{}","start_ts":11.0}}"#,
                std::process::id(),
                std::process::id(),
                app_dir.display(),
                log_path.display(),
            ),
        )
        .unwrap();
        fs::write(
            app_dir.join("harness.json"),
            r#"{"sid-harness":{"enabled":true,"request":"Keep going","cooldown_minutes":5,"remaining_injections":2}}"#,
        )
        .unwrap();

        let config = RuntimeConfig {
            app_dir: app_dir.clone(),
        };
        let mut last_injected = std::collections::HashMap::new();
        let mut last_injected_scope = std::collections::HashMap::new();
        assert!(run_harness_sweep_once(
            &config,
            &mut last_injected,
            &mut last_injected_scope,
            500.0
        )
        .unwrap());
        assert_eq!(last_injected.get("sid-harness").copied(), Some(500.0));
        assert_eq!(
            last_injected_scope.get("thread:thread-harness").copied(),
            Some(500.0)
        );

        let harness_after: Value =
            serde_json::from_str(&fs::read_to_string(app_dir.join("harness.json")).unwrap())
                .unwrap();
        assert_eq!(harness_after["sid-harness"]["remaining_injections"], 1);
        assert_eq!(harness_after["sid-harness"]["enabled"], true);

        listener_thread.join().unwrap();
    }

    #[test]
    fn run_harness_sweep_once_dedupes_same_thread_scope() {
        let app_dir = temp_app_dir("harness-worker-dedupe");
        let sock_path = app_dir.join("socks").join("sid-a.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        let listener_thread = thread::spawn(move || {
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                if line.contains("\"cmd\":\"send\"") {
                    assert!(line.contains("Additional request from user: A"));
                    stream
                        .write_all(b"{\"queued\":false,\"queue_len\":0}\n")
                        .unwrap();
                    return;
                }
                stream
                    .write_all(b"{\"busy\":false,\"queue_len\":0}\n")
                    .unwrap();
            }
            panic!("harness worker never injected first same-thread prompt");
        });

        let log_a = app_dir.join("rollout-a.jsonl");
        let log_b = app_dir.join("rollout-b.jsonl");
        fs::write(
            &log_a,
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"done a"},"ts":100.0}
"#,
        )
        .unwrap();
        fs::write(
            &log_b,
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"done b"},"ts":100.0}
"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("socks").join("sid-a.json"),
            format!(
                r#"{{"session_id":"thread-shared","codex_pid":{},"broker_pid":{},"cwd":"{}","log_path":"{}","start_ts":11.0}}"#,
                std::process::id(),
                std::process::id(),
                app_dir.display(),
                log_a.display(),
            ),
        )
        .unwrap();
        fs::write(app_dir.join("socks").join("sid-b.sock"), "").unwrap();
        fs::write(
            app_dir.join("socks").join("sid-b.json"),
            format!(
                r#"{{"session_id":"thread-shared","codex_pid":{},"broker_pid":{},"cwd":"{}","log_path":"{}","start_ts":12.0}}"#,
                std::process::id(),
                std::process::id(),
                app_dir.display(),
                log_b.display(),
            ),
        )
        .unwrap();
        fs::write(
            app_dir.join("harness.json"),
            r#"{"sid-a":{"enabled":true,"request":"A","cooldown_minutes":5,"remaining_injections":2},"sid-b":{"enabled":true,"request":"B","cooldown_minutes":5,"remaining_injections":2}}"#,
        )
        .unwrap();

        let config = RuntimeConfig {
            app_dir: app_dir.clone(),
        };
        let mut last_injected = std::collections::HashMap::new();
        let mut last_injected_scope = std::collections::HashMap::new();
        assert!(run_harness_sweep_once(
            &config,
            &mut last_injected,
            &mut last_injected_scope,
            500.0
        )
        .unwrap());

        let harness_after: Value =
            serde_json::from_str(&fs::read_to_string(app_dir.join("harness.json")).unwrap())
                .unwrap();
        assert_eq!(harness_after["sid-a"]["remaining_injections"], 1);
        assert_eq!(harness_after["sid-b"]["remaining_injections"], 2);
        assert_eq!(
            last_injected_scope.get("thread:thread-shared").copied(),
            Some(500.0)
        );

        listener_thread.join().unwrap();
    }
}
