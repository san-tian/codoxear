use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    User,
    Assistant,
    Tool,
    ToolResult,
    AskUser,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TranscriptEvent {
    pub id: String,
    pub kind: EventKind,
    pub title: Option<String>,
    pub body: String,
    pub meta: Option<String>,
    pub created_at: f64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub backend: String,
    pub status: String,
    pub workspace: String,
    pub transport: Option<String>,
    pub tmux_session: Option<String>,
    pub tmux_window: Option<String>,
    pub last_line: String,
    pub unread: u32,
    pub updated_at: f64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub summary: String,
    pub status: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DiagnosticItem {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    pub files: Vec<FileEntry>,
    pub diagnostics: Vec<DiagnosticItem>,
    pub transcript: Vec<TranscriptEvent>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BootstrapPayload {
    pub app_name: String,
    pub selected_session_id: String,
    pub sessions: Vec<SessionSummary>,
    pub session_details: HashMap<String, SessionDetail>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiSessionSummary {
    pub session_id: String,
    pub thread_id: Option<String>,
    pub pid: i64,
    pub broker_pid: i64,
    pub agent_backend: String,
    pub owned: bool,
    pub transport: Option<String>,
    pub cwd: String,
    pub workspace_cwd: Option<String>,
    pub start_ts: f64,
    pub updated_ts: f64,
    pub log_path: Option<String>,
    pub queue_len: usize,
    pub busy: bool,
    pub token: Option<serde_json::Value>,
    pub harness_enabled: bool,
    pub harness_cooldown_minutes: f64,
    pub harness_remaining_injections: i64,
    pub alias: String,
    pub files: Vec<String>,
    pub git_branch: Option<String>,
    pub model_provider: Option<String>,
    pub preferred_auth_method: Option<String>,
    pub provider_choice: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub tmux_session: Option<String>,
    pub tmux_window: Option<String>,
    pub priority_offset: f64,
    pub snooze_until: Option<f64>,
    pub dependency_session_id: Option<String>,
    pub time_priority: f64,
    pub base_priority: f64,
    pub final_priority: f64,
    pub blocked: bool,
    pub snoozed: bool,
    pub last_assistant_ts: Option<f64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiBackendDefaults {
    pub agent_backend: String,
    pub model_provider: Option<String>,
    pub preferred_auth_method: Option<String>,
    pub provider_choice: Option<String>,
    pub provider_choices: Vec<String>,
    pub model: Option<String>,
    pub models: Vec<String>,
    pub reasoning_effort: String,
    pub reasoning_efforts: Vec<String>,
    pub service_tier: Option<String>,
    pub supports_fast: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiNewSessionDefaults {
    pub default_backend: String,
    pub backends: HashMap<String, ApiBackendDefaults>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiSessionsResponse {
    pub app_version: String,
    pub sessions: Vec<ApiSessionSummary>,
    pub recent_cwds: Vec<String>,
    pub new_session_defaults: ApiNewSessionDefaults,
    pub tmux_available: bool,
    pub tmux_session_name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiMessagesTailResponse {
    pub thread_id: Option<String>,
    pub log_path: Option<String>,
    pub live_cursor: Option<String>,
    pub history_cursor: Option<String>,
    pub events: Vec<serde_json::Value>,
    pub has_older: bool,
    pub busy: bool,
    pub queue_len: usize,
    pub token: Option<serde_json::Value>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiMessagesHistoryResponse {
    pub thread_id: Option<String>,
    pub log_path: Option<String>,
    pub history_cursor: Option<String>,
    pub events: Vec<serde_json::Value>,
    pub has_older: bool,
    pub busy: bool,
    pub queue_len: usize,
    pub token: Option<serde_json::Value>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiMessagesLiveResponse {
    pub thread_id: Option<String>,
    pub log_path: Option<String>,
    pub live_cursor: Option<String>,
    pub events: Vec<serde_json::Value>,
    pub meta_delta: serde_json::Value,
    pub turn_start: bool,
    pub turn_end: bool,
    pub turn_aborted: bool,
    pub diag: serde_json::Value,
    pub busy: bool,
    pub queue_len: usize,
    pub token: Option<serde_json::Value>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiDiagnosticsResponse {
    pub session_id: String,
    pub thread_id: Option<String>,
    pub agent_backend: String,
    pub owned: bool,
    pub transport: Option<String>,
    pub cwd: String,
    pub start_ts: f64,
    pub updated_ts: f64,
    pub log_path: Option<String>,
    pub broker_pid: i64,
    pub codex_pid: i64,
    pub busy: bool,
    pub broker_busy: bool,
    pub queue_len: usize,
    pub token: Option<serde_json::Value>,
    pub model_provider: Option<String>,
    pub preferred_auth_method: Option<String>,
    pub provider_choice: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub tmux_session: Option<String>,
    pub tmux_window: Option<String>,
    pub git_branch: Option<String>,
    pub time_priority: f64,
    pub base_priority: f64,
    pub final_priority: f64,
    pub priority_offset: f64,
    pub snooze_until: Option<f64>,
    pub dependency_session_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiQueueItem {
    pub id: String,
    pub text: String,
    pub created_ts: f64,
    pub sending: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiQueueResponse {
    pub ok: bool,
    pub items: Vec<ApiQueueItem>,
    pub queue: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiFileSearchMatch {
    pub path: String,
    pub score: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiFileSearchResponse {
    pub ok: bool,
    pub cwd: String,
    pub query: String,
    pub mode: String,
    pub matches: Vec<ApiFileSearchMatch>,
    pub scanned: usize,
    pub truncated: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiChangedFileEntry {
    pub path: String,
    pub additions: Option<i64>,
    pub deletions: Option<i64>,
    pub changed: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiChangedFilesResponse {
    pub ok: bool,
    pub cwd: String,
    pub files: Vec<String>,
    pub entries: Vec<ApiChangedFileEntry>,
    pub unstaged: Vec<String>,
    pub staged: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiGitDiffResponse {
    pub ok: bool,
    pub cwd: String,
    pub path: String,
    pub staged: bool,
    pub diff: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiGitFileVersionsResponse {
    pub ok: bool,
    pub cwd: String,
    pub path: String,
    pub abs_path: String,
    pub current_exists: bool,
    pub current_size: u64,
    pub current_text: String,
    pub base_exists: bool,
    pub base_text: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiHarnessResponse {
    pub ok: bool,
    pub enabled: bool,
    pub request: String,
    pub cooldown_minutes: f64,
    pub remaining_injections: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SendMessagePayload {
    pub session_id: String,
    pub text: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LiveEvent {
    Connected {
        created_at: f64,
    },
    MessageCreated {
        session_id: String,
        event: TranscriptEvent,
        session: SessionSummary,
    },
}
