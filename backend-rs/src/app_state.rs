use crate::models::{
    BootstrapPayload, DiagnosticItem, EventKind, FileEntry, LiveEvent, SessionDetail, SessionSummary, TranscriptEvent,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};

pub type SharedStore = Arc<RwLock<Store>>;

#[derive(Clone)]
pub struct AppState {
    pub store: SharedStore,
    pub tx: broadcast::Sender<LiveEvent>,
}

pub struct Store {
    pub app_name: String,
    pub selected_session_id: String,
    pub sessions: Vec<SessionSummary>,
    pub session_details: HashMap<String, SessionDetail>,
}

impl Store {
    pub fn bootstrap(&self) -> BootstrapPayload {
        BootstrapPayload {
            app_name: self.app_name.clone(),
            selected_session_id: self.selected_session_id.clone(),
            sessions: self.sessions.clone(),
            session_details: self.session_details.clone(),
        }
    }
}

pub fn epoch_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn build_state() -> AppState {
    let now = epoch_now();
    let sessions = vec![
        SessionSummary {
            id: "sess-alpha".into(),
            title: "Rust replatforming spike".into(),
            backend: "codex".into(),
            status: "running".into(),
            workspace: "ccss/codoxear".into(),
            transport: Some("tmux".into()),
            tmux_session: Some("codoxear-dev".into()),
            tmux_window: Some("nova".into()),
            last_line: "Need a cleaner event spine before swapping the shell.".into(),
            unread: 2,
            updated_at: now - 45.0,
        },
        SessionSummary {
            id: "sess-beta".into(),
            title: "Mobile transcript tuning".into(),
            backend: "pi".into(),
            status: "idle".into(),
            workspace: "ccss/mobile".into(),
            transport: None,
            tmux_session: None,
            tmux_window: None,
            last_line: "Scrolling is stable, but tool panels still feel dense.".into(),
            unread: 0,
            updated_at: now - 640.0,
        },
    ];

    let mut session_details = HashMap::new();
    session_details.insert(
        "sess-alpha".into(),
        SessionDetail {
            files: vec![
                FileEntry {
                    path: "frontend/src/app.tsx".into(),
                    summary: "Main shell coordinates transcript, SSE, and inspector state.".into(),
                    status: "changed".into(),
                },
                FileEntry {
                    path: "backend-rs/src/routes.rs".into(),
                    summary: "Axum routes expose bootstrap, send, health, and SSE endpoints.".into(),
                    status: "new".into(),
                },
            ],
            diagnostics: vec![
                DiagnosticItem {
                    label: "Transport".into(),
                    value: "SSE".into(),
                },
                DiagnosticItem {
                    label: "Frontend".into(),
                    value: "Preact + Pretext".into(),
                },
                DiagnosticItem {
                    label: "Backend".into(),
                    value: "Rust + Axum".into(),
                },
            ],
            transcript: vec![
                TranscriptEvent {
                    id: "evt-1".into(),
                    kind: EventKind::Assistant,
                    title: Some("Design direction".into()),
                    body: "The new shell should feel dense, surgical, and calm under continuous updates.".into(),
                    meta: Some("Prepared with Pretext line layout".into()),
                    created_at: now - 580.0,
                },
                TranscriptEvent {
                    id: "evt-2".into(),
                    kind: EventKind::Tool,
                    title: Some("cargo search axum".into()),
                    body: "Confirming current crate versions for the new backend scaffold.".into(),
                    meta: Some("tool call".into()),
                    created_at: now - 510.0,
                },
                TranscriptEvent {
                    id: "evt-3".into(),
                    kind: EventKind::ToolResult,
                    title: Some("axum = 0.8.9".into()),
                    body: "tokio = 1.52.1 · tower-http = 0.6.8".into(),
                    meta: Some("tool result".into()),
                    created_at: now - 504.0,
                },
                TranscriptEvent {
                    id: "evt-4".into(),
                    kind: EventKind::AskUser,
                    title: Some("Implementation direction".into()),
                    body: "First ship a full shell preview, then deepen the protocol and broker bridge.".into(),
                    meta: Some("resolved interaction".into()),
                    created_at: now - 420.0,
                },
            ],
        },
    );
    session_details.insert(
        "sess-beta".into(),
        SessionDetail {
            files: vec![FileEntry {
                path: "codoxear/static/app.css".into(),
                summary: "Current mobile affordances remain the reference for interaction density.".into(),
                status: "viewed".into(),
            }],
            diagnostics: vec![
                DiagnosticItem {
                    label: "Status".into(),
                    value: "Reference branch".into(),
                },
                DiagnosticItem {
                    label: "Priority".into(),
                    value: "Visual language".into(),
                },
            ],
            transcript: vec![TranscriptEvent {
                id: "evt-5".into(),
                kind: EventKind::Assistant,
                title: Some("Backlog".into()),
                body: "Use this thread as the visual benchmark for compactness and sidebar behavior.".into(),
                meta: None,
                created_at: now - 900.0,
            }],
        },
    );

    let (tx, _) = broadcast::channel(128);
    AppState {
        store: Arc::new(RwLock::new(Store {
            app_name: "Codoxear Nova".into(),
            selected_session_id: "sess-alpha".into(),
            sessions,
            session_details,
        })),
        tx,
    }
}
