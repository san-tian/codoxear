use crate::models::{BootstrapPayload, LiveEvent, SessionDetail, SessionSummary};
use crate::runtime::{load_preview_bootstrap, RuntimeConfig};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};

pub type SharedStore = Arc<RwLock<Store>>;

#[derive(Clone)]
pub struct AppState {
    pub config: RuntimeConfig,
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
    let config = RuntimeConfig::from_env().expect("resolve Codoxear runtime config");
    build_state_from_config(config).expect("load Codoxear runtime state")
}

pub fn build_state_from_config(config: RuntimeConfig) -> Result<AppState, String> {
    let (sessions, session_details, selected_session_id) = load_preview_bootstrap(&config)?;
    let (tx, _) = broadcast::channel(128);
    Ok(AppState {
        config,
        store: Arc::new(RwLock::new(Store {
            app_name: "Codoxear Nova".into(),
            selected_session_id,
            sessions,
            session_details,
        })),
        tx,
    })
}
