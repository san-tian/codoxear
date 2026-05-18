use crate::runtime::{
    create_session, create_session_request_from_payload, enqueue_session_message,
    load_messages_live, load_messages_tail, load_queue_response, load_sessions_response,
    rename_session, RuntimeConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::task;
use tokio::time::{self, MissedTickBehavior};

const SCHEDULES_FILE: &str = "schedules.json";
const RUNS_FILE: &str = "schedule_runs.json";
const DEFAULT_TIMEOUT_MINUTES: f64 = 240.0;
const DEFAULT_SWEEP_SECONDS: f64 = 15.0;
const RUN_RETAIN_LIMIT: usize = 50;
const FAILED_RETAIN_MINIMUM: usize = 10;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiSchedule {
    pub schedule_id: String,
    pub name: String,
    pub enabled: bool,
    pub status: String,
    pub target: ScheduleTarget,
    pub rule: ScheduleRule,
    pub message_template: String,
    #[serde(default)]
    pub alias_template: Option<String>,
    pub busy_policy: String,
    #[serde(default = "default_timeout_minutes")]
    pub timeout_minutes: Option<f64>,
    pub created_at: f64,
    pub updated_at: f64,
    #[serde(default)]
    pub next_run_at: Option<f64>,
    #[serde(default)]
    pub last_run_at: Option<f64>,
    #[serde(default)]
    pub run_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum ScheduleTarget {
    #[serde(rename = "existing_session")]
    ExistingSession { session_id: String },
    #[serde(rename = "new_session_each_run")]
    NewSessionEachRun { session_template: Value },
    #[serde(rename = "create_once_reuse")]
    CreateOnceReuse {
        session_template: Value,
        #[serde(default)]
        created_session_id: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduleRule {
    pub kind: String,
    #[serde(default)]
    pub next_run_at: Option<f64>,
    #[serde(default)]
    pub interval_seconds: Option<f64>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub weekdays: Vec<u8>,
    #[serde(default)]
    pub day_of_month: Option<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiScheduleRun {
    pub run_id: String,
    pub schedule_id: String,
    pub status: String,
    pub trigger: String,
    pub scheduled_for: f64,
    pub created_at: f64,
    #[serde(default)]
    pub started_at: Option<f64>,
    #[serde(default)]
    pub finished_at: Option<f64>,
    #[serde(default)]
    pub target_session_id: Option<String>,
    #[serde(default)]
    pub created_session_id: Option<String>,
    #[serde(default)]
    pub queue_item_id: Option<String>,
    #[serde(default)]
    pub started_cursor: Option<String>,
    pub rendered_message: String,
    #[serde(default)]
    pub rendered_alias: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub assistant_preview: Option<String>,
    pub snapshot: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiSchedulesResponse {
    pub ok: bool,
    pub schedules: Vec<ApiSchedule>,
    pub runs: Vec<ApiScheduleRun>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ScheduleStore {
    #[serde(default)]
    schedules: Vec<ApiSchedule>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RunStore {
    #[serde(default)]
    runs: Vec<ApiScheduleRun>,
}

fn default_timeout_minutes() -> Option<f64> {
    Some(DEFAULT_TIMEOUT_MINUTES)
}

pub fn rust_schedule_sweep_enabled() -> bool {
    std::env::var("CODOXEAR_ENABLE_SCHEDULE_SWEEP")
        .ok()
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

pub fn spawn_schedule_sweep_worker(config: RuntimeConfig) {
    tokio::spawn(async move {
        let mut interval = time::interval(schedule_sweep_interval());
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let sweep_config = config.clone();
            match task::spawn_blocking(move || run_schedule_sweep_once(&sweep_config, epoch_now()))
                .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => tracing::warn!("rust schedule sweep failed: {err}"),
                Err(err) => tracing::warn!("rust schedule sweep task join failed: {err}"),
            }
        }
    });
}

fn schedule_sweep_interval() -> Duration {
    let seconds = std::env::var("CODEX_WEB_SCHEDULE_SWEEP_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 1.0)
        .unwrap_or(DEFAULT_SWEEP_SECONDS);
    Duration::from_secs_f64(seconds)
}

pub fn list_schedules_response(
    config: &RuntimeConfig,
    allowed_sessions: Option<&HashSet<String>>,
) -> Result<ApiSchedulesResponse, String> {
    let store = read_schedule_store(config)?;
    let runs = read_run_store(config)?.runs;
    let schedules = store
        .schedules
        .into_iter()
        .filter(|schedule| schedule_visible_to(schedule, allowed_sessions))
        .collect::<Vec<_>>();
    let allowed_ids = schedules
        .iter()
        .map(|schedule| schedule.schedule_id.clone())
        .collect::<HashSet<_>>();
    let runs = runs
        .into_iter()
        .filter(|run| allowed_ids.contains(&run.schedule_id))
        .collect();
    Ok(ApiSchedulesResponse {
        ok: true,
        schedules,
        runs,
    })
}

pub fn create_schedule_response(
    config: &RuntimeConfig,
    payload: &Value,
    allowed_sessions: Option<&HashSet<String>>,
) -> Result<ApiSchedulesResponse, String> {
    let now = epoch_now();
    let mut schedule = schedule_from_payload(payload, now, None)?;
    validate_schedule_access(config, &schedule, allowed_sessions)?;
    let mut store = read_schedule_store(config)?;
    if schedule.schedule_id.trim().is_empty() {
        schedule.schedule_id = generate_id("sch");
    }
    if store
        .schedules
        .iter()
        .any(|item| item.schedule_id == schedule.schedule_id)
    {
        return Err("schedule_id already exists".to_string());
    }
    store.schedules.push(schedule);
    write_schedule_store(config, &store)?;
    list_schedules_response(config, allowed_sessions)
}

pub fn update_schedule_response(
    config: &RuntimeConfig,
    schedule_id: &str,
    payload: &Value,
    allowed_sessions: Option<&HashSet<String>>,
) -> Result<ApiSchedulesResponse, String> {
    let mut store = read_schedule_store(config)?;
    let Some(index) = store
        .schedules
        .iter()
        .position(|schedule| schedule.schedule_id == schedule_id)
    else {
        return Err("schedule not found".to_string());
    };
    validate_schedule_access(config, &store.schedules[index], allowed_sessions)?;
    let mut next =
        schedule_from_payload(payload, epoch_now(), Some(store.schedules[index].clone()))?;
    next.schedule_id = schedule_id.to_string();
    validate_schedule_access(config, &next, allowed_sessions)?;
    store.schedules[index] = next;
    write_schedule_store(config, &store)?;
    list_schedules_response(config, allowed_sessions)
}

pub fn delete_schedule_response(
    config: &RuntimeConfig,
    schedule_id: &str,
    allowed_sessions: Option<&HashSet<String>>,
) -> Result<ApiSchedulesResponse, String> {
    let now = epoch_now();
    let mut store = read_schedule_store(config)?;
    let Some(index) = store
        .schedules
        .iter()
        .position(|schedule| schedule.schedule_id == schedule_id)
    else {
        return Err("schedule not found".to_string());
    };
    validate_schedule_access(config, &store.schedules[index], allowed_sessions)?;
    store.schedules.remove(index);
    write_schedule_store(config, &store)?;

    let mut run_store = read_run_store(config)?;
    for run in run_store
        .runs
        .iter_mut()
        .filter(|run| run.schedule_id == schedule_id && is_active_run_status(&run.status))
    {
        run.status = "cancelled_by_delete".to_string();
        run.finished_at = Some(now);
    }
    retain_runs(&mut run_store.runs);
    write_run_store(config, &run_store)?;
    list_schedules_response(config, allowed_sessions)
}

pub fn set_schedule_enabled_response(
    config: &RuntimeConfig,
    schedule_id: &str,
    enabled: bool,
    allowed_sessions: Option<&HashSet<String>>,
) -> Result<ApiSchedulesResponse, String> {
    let mut store = read_schedule_store(config)?;
    let Some(schedule) = store
        .schedules
        .iter_mut()
        .find(|schedule| schedule.schedule_id == schedule_id)
    else {
        return Err("schedule not found".to_string());
    };
    validate_schedule_access(config, schedule, allowed_sessions)?;
    schedule.enabled = enabled;
    schedule.status = if enabled { "active" } else { "disabled" }.to_string();
    schedule.updated_at = epoch_now();
    write_schedule_store(config, &store)?;
    list_schedules_response(config, allowed_sessions)
}

pub fn run_schedule_now_response(
    config: &RuntimeConfig,
    schedule_id: &str,
    allowed_sessions: Option<&HashSet<String>>,
) -> Result<ApiSchedulesResponse, String> {
    let mut store = read_schedule_store(config)?;
    let Some(index) = store
        .schedules
        .iter()
        .position(|schedule| schedule.schedule_id == schedule_id)
    else {
        return Err("schedule not found".to_string());
    };
    validate_schedule_access(config, &store.schedules[index], allowed_sessions)?;
    let mut run_store = read_run_store(config)?;
    start_schedule_run(
        config,
        &mut store.schedules[index],
        &mut run_store,
        epoch_now(),
        "manual",
    )?;
    retain_runs(&mut run_store.runs);
    write_schedule_store(config, &store)?;
    write_run_store(config, &run_store)?;
    list_schedules_response(config, allowed_sessions)
}

pub fn mark_schedule_run_done_response(
    config: &RuntimeConfig,
    schedule_id: &str,
    run_id: &str,
    allowed_sessions: Option<&HashSet<String>>,
) -> Result<ApiSchedulesResponse, String> {
    let store = read_schedule_store(config)?;
    let Some(schedule) = store
        .schedules
        .iter()
        .find(|schedule| schedule.schedule_id == schedule_id)
    else {
        return Err("schedule not found".to_string());
    };
    validate_schedule_access(config, schedule, allowed_sessions)?;
    let now = epoch_now();
    let mut run_store = read_run_store(config)?;
    let Some(run) = run_store
        .runs
        .iter_mut()
        .find(|run| run.schedule_id == schedule_id && run.run_id == run_id)
    else {
        return Err("run not found".to_string());
    };
    run.status = "marked_done".to_string();
    run.finished_at = Some(now);
    retain_runs(&mut run_store.runs);
    write_run_store(config, &run_store)?;
    list_schedules_response(config, allowed_sessions)
}

pub fn run_schedule_sweep_once(config: &RuntimeConfig, now: f64) -> Result<Value, String> {
    let mut store = read_schedule_store(config)?;
    let mut run_store = read_run_store(config)?;
    let completed = update_active_runs(config, &mut run_store, now)?;
    let mut started = 0_u64;
    for index in 0..store.schedules.len() {
        let due = {
            let schedule = &store.schedules[index];
            schedule.enabled
                && schedule
                    .next_run_at
                    .map(|value| value <= now)
                    .unwrap_or(false)
                && schedule.status != "target_missing"
        };
        if due {
            match start_schedule_run(
                config,
                &mut store.schedules[index],
                &mut run_store,
                now,
                "schedule",
            ) {
                Ok(did_start) => {
                    if did_start {
                        started += 1;
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "schedule {} failed to start: {err}",
                        store.schedules[index].schedule_id
                    );
                }
            }
            advance_schedule_after_due(&mut store.schedules[index], now);
        }
    }
    retain_runs(&mut run_store.runs);
    write_schedule_store(config, &store)?;
    write_run_store(config, &run_store)?;
    Ok(json!({"ok": true, "started": started, "completed": completed}))
}

fn start_schedule_run(
    config: &RuntimeConfig,
    schedule: &mut ApiSchedule,
    run_store: &mut RunStore,
    now: f64,
    trigger: &str,
) -> Result<bool, String> {
    if has_active_run(run_store, &schedule.schedule_id) {
        run_store.runs.push(ApiScheduleRun {
            run_id: generate_id("run"),
            schedule_id: schedule.schedule_id.clone(),
            status: "skipped_overlap".to_string(),
            trigger: trigger.to_string(),
            scheduled_for: schedule.next_run_at.unwrap_or(now),
            created_at: now,
            started_at: None,
            finished_at: Some(now),
            target_session_id: None,
            created_session_id: None,
            queue_item_id: None,
            started_cursor: None,
            rendered_message: render_template(
                &schedule.message_template,
                now,
                schedule.run_count + 1,
            ),
            rendered_alias: render_optional_template(
                schedule.alias_template.as_deref(),
                now,
                schedule.run_count + 1,
            ),
            error: Some("previous run is still queued or running".to_string()),
            assistant_preview: None,
            snapshot: schedule_snapshot(schedule),
        });
        return Ok(false);
    }

    let next_run_number = schedule.run_count + 1;
    let rendered_message = render_template(&schedule.message_template, now, next_run_number);
    let rendered_alias =
        render_optional_template(schedule.alias_template.as_deref(), now, next_run_number);
    let scheduled_for = if trigger == "schedule" {
        schedule.next_run_at.unwrap_or(now)
    } else {
        now
    };
    let (target_session_id, created_session_id) =
        match resolve_target_session(config, schedule, rendered_alias.as_deref()) {
            Ok((session_id, created_id)) => (session_id, created_id),
            Err(err) if err == "target_missing" => {
                schedule.enabled = false;
                schedule.status = "target_missing".to_string();
                schedule.updated_at = now;
                run_store.runs.push(ApiScheduleRun {
                    run_id: generate_id("run"),
                    schedule_id: schedule.schedule_id.clone(),
                    status: "target_missing".to_string(),
                    trigger: trigger.to_string(),
                    scheduled_for,
                    created_at: now,
                    started_at: None,
                    finished_at: Some(now),
                    target_session_id: None,
                    created_session_id: None,
                    queue_item_id: None,
                    started_cursor: None,
                    rendered_message,
                    rendered_alias,
                    error: Some("target session is missing".to_string()),
                    assistant_preview: None,
                    snapshot: schedule_snapshot(schedule),
                });
                return Ok(false);
            }
            Err(err) => return Err(err),
        };

    let cursor = load_messages_tail(config, &target_session_id, 1)
        .ok()
        .and_then(|tail| tail.live_cursor);
    let enqueue_response = enqueue_session_message(config, &target_session_id, &rendered_message)?;
    let queue_item_id = enqueue_response
        .get("item")
        .and_then(|item| item.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let status = if queue_item_id.is_some() {
        "queued"
    } else {
        "running"
    };
    let started_at = if queue_item_id.is_some() {
        None
    } else {
        Some(now)
    };
    run_store.runs.push(ApiScheduleRun {
        run_id: generate_id("run"),
        schedule_id: schedule.schedule_id.clone(),
        status: status.to_string(),
        trigger: trigger.to_string(),
        scheduled_for,
        created_at: now,
        started_at,
        finished_at: None,
        target_session_id: Some(target_session_id),
        created_session_id,
        queue_item_id,
        started_cursor: cursor,
        rendered_message,
        rendered_alias,
        error: None,
        assistant_preview: None,
        snapshot: schedule_snapshot(schedule),
    });
    schedule.run_count = next_run_number;
    schedule.last_run_at = Some(now);
    schedule.status = if schedule.enabled {
        "active"
    } else {
        "disabled"
    }
    .to_string();
    schedule.updated_at = now;
    Ok(true)
}

fn update_active_runs(
    config: &RuntimeConfig,
    run_store: &mut RunStore,
    now: f64,
) -> Result<u64, String> {
    let mut completed = 0_u64;
    for run in run_store.runs.iter_mut() {
        if !is_active_run_status(&run.status) {
            continue;
        }
        let timeout_minutes = run
            .snapshot
            .get("timeout_minutes")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0);
        if let Some(minutes) = timeout_minutes {
            if now - run.created_at >= minutes * 60.0 {
                run.status = "timeout".to_string();
                run.finished_at = Some(now);
                run.error = Some("schedule run timed out".to_string());
                completed += 1;
                continue;
            }
        }
        if run.status == "queued" {
            let Some(session_id) = run.target_session_id.as_deref() else {
                continue;
            };
            let Some(queue_id) = run.queue_item_id.as_deref() else {
                run.status = "running".to_string();
                run.started_at = Some(now);
                continue;
            };
            let queue = load_queue_response(config, session_id)?;
            if queue.items.iter().any(|item| item.id == queue_id) {
                continue;
            }
            run.status = "running".to_string();
            run.started_at = Some(now);
        }
        if run.status != "running" {
            continue;
        }
        let Some(session_id) = run.target_session_id.as_deref() else {
            continue;
        };
        let Some(cursor) = run.started_cursor.as_deref() else {
            run.started_cursor = load_messages_tail(config, session_id, 1)
                .ok()
                .and_then(|tail| tail.live_cursor);
            continue;
        };
        let live = load_messages_live(config, session_id, cursor)?;
        let assistant_preview = final_assistant_preview(&live.events);
        if live.turn_end || assistant_preview.is_some() {
            run.status = "completed".to_string();
            run.finished_at = Some(now);
            run.assistant_preview =
                assistant_preview.or_else(|| assistant_preview_from_events(&live.events));
            completed += 1;
        } else if live.turn_aborted {
            run.status = "interrupted".to_string();
            run.finished_at = Some(now);
            run.error = Some("agent turn aborted".to_string());
            completed += 1;
        }
    }
    Ok(completed)
}

fn resolve_target_session(
    config: &RuntimeConfig,
    schedule: &mut ApiSchedule,
    rendered_alias: Option<&str>,
) -> Result<(String, Option<String>), String> {
    match &mut schedule.target {
        ScheduleTarget::ExistingSession { session_id } => {
            ensure_session_exists(config, session_id)?;
            Ok((session_id.clone(), None))
        }
        ScheduleTarget::NewSessionEachRun { session_template } => {
            let session_id =
                create_session_from_template(config, session_template, rendered_alias)?;
            Ok((session_id.clone(), Some(session_id)))
        }
        ScheduleTarget::CreateOnceReuse {
            session_template,
            created_session_id,
        } => {
            if let Some(session_id) = created_session_id.clone() {
                ensure_session_exists(config, &session_id)?;
                return Ok((session_id, None));
            }
            let session_id =
                create_session_from_template(config, session_template, rendered_alias)?;
            *created_session_id = Some(session_id.clone());
            Ok((session_id.clone(), Some(session_id)))
        }
    }
}

fn create_session_from_template(
    config: &RuntimeConfig,
    template: &Value,
    rendered_alias: Option<&str>,
) -> Result<String, String> {
    let before = load_sessions_response(config)?
        .sessions
        .into_iter()
        .map(|session| session.session_id)
        .collect::<HashSet<_>>();
    let request =
        create_session_request_from_payload(template).map_err(|err| err.message().to_string())?;
    let response = create_session(config, request).map_err(|err| err.message().to_string())?;
    let broker_pid = response.get("broker_pid").and_then(Value::as_i64);
    let mut last_sessions = Vec::new();
    for _ in 0..20 {
        let sessions = load_sessions_response(config)?.sessions;
        if let Some(pid) = broker_pid {
            if let Some(session) = sessions.iter().find(|session| session.broker_pid == pid) {
                if let Some(alias) = rendered_alias.filter(|value| !value.trim().is_empty()) {
                    let _ = rename_session(config, &session.session_id, alias);
                }
                return Ok(session.session_id.clone());
            }
        }
        if let Some(session) = sessions
            .iter()
            .find(|session| !before.contains(&session.session_id))
        {
            if let Some(alias) = rendered_alias.filter(|value| !value.trim().is_empty()) {
                let _ = rename_session(config, &session.session_id, alias);
            }
            return Ok(session.session_id.clone());
        }
        last_sessions = sessions;
        std::thread::sleep(Duration::from_millis(250));
    }
    if let Some(session) = last_sessions
        .iter()
        .find(|session| !before.contains(&session.session_id))
    {
        return Ok(session.session_id.clone());
    }
    Err("created session metadata was not discovered".to_string())
}

fn ensure_session_exists(config: &RuntimeConfig, session_id: &str) -> Result<(), String> {
    let sessions = load_sessions_response(config)?;
    if sessions
        .sessions
        .iter()
        .any(|session| session.session_id == session_id)
    {
        Ok(())
    } else {
        Err("target_missing".to_string())
    }
}

fn schedule_from_payload(
    payload: &Value,
    now: f64,
    existing: Option<ApiSchedule>,
) -> Result<ApiSchedule, String> {
    if !payload.is_object() {
        return Err("invalid json body (expected object)".to_string());
    }
    let mut base = existing.unwrap_or_else(|| ApiSchedule {
        schedule_id: payload
            .get("schedule_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
        name: String::new(),
        enabled: true,
        status: "active".to_string(),
        target: ScheduleTarget::ExistingSession {
            session_id: String::new(),
        },
        rule: ScheduleRule {
            kind: "once".to_string(),
            next_run_at: None,
            interval_seconds: None,
            timezone: None,
            time: None,
            weekdays: Vec::new(),
            day_of_month: None,
        },
        message_template: String::new(),
        alias_template: None,
        busy_policy: "enqueue".to_string(),
        timeout_minutes: Some(DEFAULT_TIMEOUT_MINUTES),
        created_at: now,
        updated_at: now,
        next_run_at: None,
        last_run_at: None,
        run_count: 0,
    });

    if let Some(value) = payload.get("name").and_then(Value::as_str) {
        base.name = value.trim().to_string();
    }
    if let Some(value) = payload.get("enabled").and_then(Value::as_bool) {
        base.enabled = value;
    }
    if let Some(value) = payload.get("message_template").and_then(Value::as_str) {
        base.message_template = value.to_string();
    }
    if let Some(value) = payload.get("alias_template") {
        base.alias_template = value.as_str().map(|text| text.to_string());
    }
    if let Some(value) = payload.get("busy_policy").and_then(Value::as_str) {
        base.busy_policy = value.to_string();
    }
    if let Some(value) = payload.get("timeout_minutes") {
        base.timeout_minutes = if value.is_null() {
            None
        } else {
            Some(
                value
                    .as_f64()
                    .filter(|number| number.is_finite() && *number > 0.0)
                    .ok_or_else(|| {
                        "timeout_minutes must be a positive number or null".to_string()
                    })?,
            )
        };
    }
    if let Some(target) = payload.get("target") {
        base.target = serde_json::from_value(target.clone())
            .map_err(|err| format!("invalid target: {err}"))?;
    }
    if let Some(rule) = payload.get("rule") {
        base.rule =
            serde_json::from_value(rule.clone()).map_err(|err| format!("invalid rule: {err}"))?;
    }
    if let Some(value) = payload.get("next_run_at") {
        base.next_run_at = value.as_f64();
    } else if let Some(value) = base.rule.next_run_at {
        base.next_run_at = Some(value);
    }
    validate_schedule(&base)?;
    base.updated_at = now;
    base.status = if base.enabled { "active" } else { "disabled" }.to_string();
    Ok(base)
}

fn validate_schedule(schedule: &ApiSchedule) -> Result<(), String> {
    if schedule.name.trim().is_empty() {
        return Err("name required".to_string());
    }
    if schedule.message_template.trim().is_empty() {
        return Err("message_template required".to_string());
    }
    if schedule.busy_policy != "enqueue" {
        return Err("busy_policy must be enqueue".to_string());
    }
    match &schedule.target {
        ScheduleTarget::ExistingSession { session_id } if session_id.trim().is_empty() => {
            Err("target.session_id required".to_string())
        }
        ScheduleTarget::NewSessionEachRun { session_template }
        | ScheduleTarget::CreateOnceReuse {
            session_template, ..
        } => {
            if !session_template.is_object() {
                return Err("target.session_template must be an object".to_string());
            }
            Ok(())
        }
        _ => Ok(()),
    }?;
    match schedule.rule.kind.as_str() {
        "once" | "daily" | "weekly" | "monthly" => {
            if schedule.next_run_at.is_none() {
                return Err("next_run_at required".to_string());
            }
        }
        "interval" => {
            let seconds = schedule
                .rule
                .interval_seconds
                .filter(|value| value.is_finite() && *value >= 60.0)
                .ok_or_else(|| "rule.interval_seconds must be at least 60".to_string())?;
            if !seconds.is_finite() {
                return Err("rule.interval_seconds must be finite".to_string());
            }
        }
        _ => return Err("unsupported rule kind".to_string()),
    }
    Ok(())
}

fn validate_schedule_access(
    config: &RuntimeConfig,
    schedule: &ApiSchedule,
    allowed_sessions: Option<&HashSet<String>>,
) -> Result<(), String> {
    let Some(allowed) = allowed_sessions else {
        return Ok(());
    };
    match &schedule.target {
        ScheduleTarget::ExistingSession { session_id } if allowed.contains(session_id) => {
            ensure_session_exists(config, session_id)?;
            Ok(())
        }
        ScheduleTarget::ExistingSession { .. } => Err("session not in share".to_string()),
        _ => Err("share schedules can only target existing shared sessions".to_string()),
    }
}

fn schedule_visible_to(schedule: &ApiSchedule, allowed_sessions: Option<&HashSet<String>>) -> bool {
    let Some(allowed) = allowed_sessions else {
        return true;
    };
    matches!(
        &schedule.target,
        ScheduleTarget::ExistingSession { session_id } if allowed.contains(session_id)
    )
}

fn advance_schedule_after_due(schedule: &mut ApiSchedule, now: f64) {
    let next = next_run_after(&schedule.rule, schedule.next_run_at.unwrap_or(now), now);
    schedule.next_run_at = next;
    if next.is_none() && schedule.rule.kind == "once" {
        schedule.enabled = false;
        schedule.status = "completed".to_string();
    }
    schedule.updated_at = now;
}

fn next_run_after(rule: &ScheduleRule, current: f64, now: f64) -> Option<f64> {
    let step = match rule.kind.as_str() {
        "once" => return None,
        "interval" => rule.interval_seconds.unwrap_or(3600.0),
        "daily" => 86_400.0,
        "weekly" => 604_800.0,
        "monthly" => 2_678_400.0,
        _ => return None,
    };
    let mut next = if current > now {
        current
    } else {
        current + step
    };
    let guard = now + step * 1000.0;
    while next <= now && next < guard {
        next += step;
    }
    Some(next)
}

fn final_assistant_preview(events: &[Value]) -> Option<String> {
    events.iter().rev().find_map(|event| {
        if event.get("role").and_then(Value::as_str) != Some("assistant") {
            return None;
        }
        if event.get("message_class").and_then(Value::as_str) != Some("final_response") {
            return None;
        }
        event
            .get("text")
            .and_then(Value::as_str)
            .map(trim_preview)
            .filter(|value| !value.is_empty())
    })
}

fn assistant_preview_from_events(events: &[Value]) -> Option<String> {
    events.iter().rev().find_map(|event| {
        if event.get("role").and_then(Value::as_str) != Some("assistant") {
            return None;
        }
        event
            .get("text")
            .and_then(Value::as_str)
            .map(trim_preview)
            .filter(|value| !value.is_empty())
    })
}

fn trim_preview(text: &str) -> String {
    let mut value = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.chars().count() > 240 {
        value = value.chars().take(240).collect::<String>();
    }
    value
}

fn has_active_run(run_store: &RunStore, schedule_id: &str) -> bool {
    run_store
        .runs
        .iter()
        .any(|run| run.schedule_id == schedule_id && is_active_run_status(&run.status))
}

fn is_active_run_status(status: &str) -> bool {
    status == "queued" || status == "running"
}

fn retain_runs(runs: &mut Vec<ApiScheduleRun>) {
    runs.sort_by(|a, b| {
        b.created_at
            .partial_cmp(&a.created_at)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut keep = Vec::new();
    let mut failures = 0_usize;
    for run in runs.iter() {
        let failure = matches!(
            run.status.as_str(),
            "timeout"
                | "interrupted"
                | "target_missing"
                | "skipped_overlap"
                | "cancelled_by_delete"
        );
        if keep.len() < RUN_RETAIN_LIMIT || (failure && failures < FAILED_RETAIN_MINIMUM) {
            if failure {
                failures += 1;
            }
            keep.push(run.clone());
        }
    }
    keep.sort_by(|a, b| {
        a.created_at
            .partial_cmp(&b.created_at)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    *runs = keep;
}

fn schedule_snapshot(schedule: &ApiSchedule) -> Value {
    json!({
        "schedule_id": schedule.schedule_id,
        "name": schedule.name,
        "target": schedule.target,
        "rule": schedule.rule,
        "busy_policy": schedule.busy_policy,
        "timeout_minutes": schedule.timeout_minutes,
        "alias_template": schedule.alias_template,
    })
}

fn render_optional_template(template: Option<&str>, now: f64, run_number: u64) -> Option<String> {
    template
        .map(|value| render_template(value, now, run_number))
        .filter(|value| !value.trim().is_empty())
}

fn render_template(template: &str, now: f64, run_number: u64) -> String {
    let (date, time, datetime) = utc_parts(now);
    template
        .replace("{date}", &date)
        .replace("{time}", &time)
        .replace("{datetime}", &datetime)
        .replace("{run_number}", &run_number.to_string())
}

fn utc_parts(ts: f64) -> (String, String, String) {
    let seconds = ts.max(0.0).floor() as i64;
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3600;
    let minute = (day_seconds % 3600) / 60;
    let second = day_seconds % 60;
    let date = format!("{year:04}-{month:02}-{day:02}");
    let time = format!("{hour:02}:{minute:02}:{second:02}");
    let datetime = format!("{date}T{time}Z");
    (date, time, datetime)
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m, d)
}

fn read_schedule_store(config: &RuntimeConfig) -> Result<ScheduleStore, String> {
    read_store(&config.app_dir.join(SCHEDULES_FILE)).map(|value| value.unwrap_or_default())
}

fn write_schedule_store(config: &RuntimeConfig, store: &ScheduleStore) -> Result<(), String> {
    write_store(&config.app_dir.join(SCHEDULES_FILE), store)
}

fn read_run_store(config: &RuntimeConfig) -> Result<RunStore, String> {
    read_store(&config.app_dir.join(RUNS_FILE)).map(|value| value.unwrap_or_default())
}

fn write_run_store(config: &RuntimeConfig, store: &RunStore) -> Result<(), String> {
    write_store(&config.app_dir.join(RUNS_FILE), store)
}

fn read_store<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Result<Option<T>, String> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|err| format!("parse {}: {err}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("read {}: {err}", path.display())),
    }
}

fn write_store<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("missing parent for {}", path.display()))?;
    fs::create_dir_all(parent).map_err(|err| format!("create {}: {err}", parent.display()))?;
    let raw = serde_json::to_string_pretty(value)
        .map(|text| text + "\n")
        .map_err(|err| format!("serialize {}: {err}", path.display()))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw).map_err(|err| format!("write {}: {err}", tmp.display()))?;
    fs::rename(&tmp, path)
        .map_err(|err| format!("rename {} -> {}: {err}", tmp.display(), path.display()))
}

fn generate_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 10];
    if let Ok(mut file) = fs::File::open("/dev/urandom") {
        let _ = file.read_exact(&mut bytes);
    } else {
        bytes[..8].copy_from_slice(&epoch_now().to_le_bytes());
    }
    let suffix = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}_{suffix}")
}

fn epoch_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

impl Default for ScheduleStore {
    fn default() -> Self {
        Self {
            schedules: Vec::new(),
        }
    }
}

impl Default for RunStore {
    fn default() -> Self {
        Self { runs: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_renders_utc_variables_and_run_number() {
        let rendered = render_template("run {run_number} {date} {time} {datetime}", 0.0, 7);
        assert_eq!(rendered, "run 7 1970-01-01 00:00:00 1970-01-01T00:00:00Z");
    }

    #[test]
    fn interval_advancement_skips_missed_runs() {
        let rule = ScheduleRule {
            kind: "interval".to_string(),
            next_run_at: None,
            interval_seconds: Some(60.0),
            timezone: None,
            time: None,
            weekdays: Vec::new(),
            day_of_month: None,
        };
        assert_eq!(next_run_after(&rule, 0.0, 3600.0), Some(3660.0));
    }

    #[test]
    fn share_filter_only_exposes_existing_allowed_sessions() {
        let mut allowed = HashSet::new();
        allowed.insert("s1".to_string());
        let visible = ApiSchedule {
            schedule_id: "a".to_string(),
            name: "A".to_string(),
            enabled: true,
            status: "active".to_string(),
            target: ScheduleTarget::ExistingSession {
                session_id: "s1".to_string(),
            },
            rule: ScheduleRule {
                kind: "once".to_string(),
                next_run_at: Some(100.0),
                interval_seconds: None,
                timezone: None,
                time: None,
                weekdays: Vec::new(),
                day_of_month: None,
            },
            message_template: "hi".to_string(),
            alias_template: None,
            busy_policy: "enqueue".to_string(),
            timeout_minutes: Some(DEFAULT_TIMEOUT_MINUTES),
            created_at: 1.0,
            updated_at: 1.0,
            next_run_at: Some(100.0),
            last_run_at: None,
            run_count: 0,
        };
        let hidden = ApiSchedule {
            target: ScheduleTarget::NewSessionEachRun {
                session_template: json!({"cwd": "/tmp", "agent_backend": "codex"}),
            },
            ..visible.clone()
        };
        assert!(schedule_visible_to(&visible, Some(&allowed)));
        assert!(!schedule_visible_to(&hidden, Some(&allowed)));
    }
}
