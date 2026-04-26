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
pub struct SendMessagePayload {
    pub session_id: String,
    pub text: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LiveEvent {
    Connected { created_at: f64 },
    MessageCreated {
        session_id: String,
        event: TranscriptEvent,
        session: SessionSummary,
    },
}
