use crate::runtime::RuntimeConfig;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time;
use web_push::{
    request_builder, ContentEncoding, SubscriptionInfo, VapidSignatureBuilder, WebPushMessage,
    WebPushMessageBuilder,
};

const DEFAULT_SUMMARIZATION_MODEL: &str = "gpt-4.1-mini";
const DEFAULT_TTS_MODEL: &str = "gpt-4o-mini-tts";
const DEFAULT_TTS_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_VAPID_SUBJECT: &str = "https://localhost";
const HLS_TARGET_DURATION_SECONDS: i64 = 12;
const HLS_MAX_SEGMENTS: usize = 18;
const HLS_KEEPALIVE_SECONDS: f64 = 6.0;
const HLS_SILENCE_SECONDS: f64 = 6.0;
const LISTENER_TTL_SECONDS: f64 = 45.0;
const DELIVERY_LEDGER_MAX: usize = 4000;
const DEFAULT_VOICES: [&str; 13] = [
    "alloy", "ash", "ballad", "cedar", "coral", "echo", "fable", "marin", "nova", "onyx", "sage",
    "shimmer", "verse",
];

#[derive(Clone, Debug)]
struct VoiceInboxMessage {
    message_id: String,
    session_id: String,
    session_display_name: String,
    message_class: String,
    text: String,
    ts: Option<f64>,
    inbox_path: PathBuf,
}

#[derive(Clone, Debug)]
struct VoiceSettings {
    narration: bool,
    final_response: bool,
    base_url: String,
    api_key: String,
    summarization_model: String,
    tts_model: String,
}

#[derive(Clone, Debug)]
struct AnnouncementTask {
    message_id: String,
    source_message_ids: Vec<String>,
    session_id: String,
    session_display_name: String,
    message_class: String,
    source_text: String,
    spoken_text: String,
    notification_text: String,
    voice: String,
    ts: Option<f64>,
    summary_word_target: Option<usize>,
}

#[derive(Clone, Debug)]
struct HlsSegment {
    seq: u64,
    name: String,
    duration: f64,
    path: PathBuf,
}

#[derive(Debug)]
pub struct VoiceDeliveryState {
    hls: MergedHlsStream,
}

impl VoiceDeliveryState {
    pub fn new(config: &RuntimeConfig) -> Result<Self, String> {
        Ok(Self {
            hls: MergedHlsStream::new(audio_root_dir(config))?,
        })
    }
}

#[derive(Debug)]
struct MergedHlsStream {
    root_dir: PathBuf,
    segments_dir: PathBuf,
    playlist_path: PathBuf,
    segments: Vec<HlsSegment>,
    next_seq: u64,
    last_error: String,
    last_append: Option<Instant>,
}

#[derive(Debug)]
struct OpenAiCompatibleClient {
    client: reqwest::Client,
}

pub fn rust_voice_delivery_worker_enabled() -> bool {
    env::var("CODOXEAR_ENABLE_VOICE_WORKER")
        .ok()
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

pub fn spawn_voice_delivery_worker(config: RuntimeConfig) {
    tokio::spawn(async move {
        let mut state = match VoiceDeliveryState::new(&config) {
            Ok(state) => state,
            Err(err) => {
                tracing::error!("voice worker init failed: {err}");
                return;
            }
        };
        let mut interval = time::interval(voice_worker_interval());
        loop {
            interval.tick().await;
            if let Err(err) = run_voice_delivery_worker_once(&config, &mut state).await {
                tracing::error!("voice worker failed: {err}");
                state.hls.set_last_error(err);
                let _ = save_runtime_snapshot(&config, &state);
            }
        }
    });
}

pub async fn run_voice_delivery_worker_once(
    config: &RuntimeConfig,
    state: &mut VoiceDeliveryState,
) -> Result<usize, String> {
    let messages = read_voice_inbox_messages(config)?;
    let mut processed = 0usize;
    for message in messages {
        process_voice_message(config, state, message.clone()).await?;
        fs::remove_file(&message.inbox_path)
            .map_err(|err| format!("remove {}: {err}", message.inbox_path.display()))?;
        processed += 1;
    }
    maybe_append_keepalive(config, state)?;
    save_runtime_snapshot(config, state)?;
    Ok(processed)
}

async fn process_voice_message(
    config: &RuntimeConfig,
    state: &mut VoiceDeliveryState,
    message: VoiceInboxMessage,
) -> Result<(), String> {
    let settings = read_voice_settings(config)?;
    let listeners = read_listener_records(config)?;
    let listener_count = listeners.len();
    let mut ledger = read_delivery_ledger(config)?;
    if ledger.contains_key(&message.message_id) {
        return Ok(());
    }
    let now_ts = epoch_now();
    ledger.insert(
        message.message_id.clone(),
        json!({
            "message_id": message.message_id,
            "session_id": message.session_id,
            "session_display_name": message.session_display_name,
            "message_class": message.message_class,
            "preview_text": clip_text(&message.text, 160),
            "notification_text": "",
            "summary_text": "",
            "summary_status": if message.message_class == "final_response" || settings.narration { "pending" } else { "skipped" },
            "narrated_status": if message.message_class == "final_response" || settings.narration { "pending" } else { "skipped" },
            "push_status": if message.message_class == "final_response" { "pending" } else { "skipped" },
            "voice": "",
            "created_ts": now_ts,
            "updated_ts": now_ts,
            "last_error": "",
        }),
    );
    write_delivery_ledger(config, &mut ledger)?;

    let task = if message.message_class == "final_response" {
        prepare_final_response(config, state, &settings, &message, &mut ledger).await?
    } else if settings.narration {
        Some(AnnouncementTask {
            message_id: message.message_id.clone(),
            source_message_ids: vec![message.message_id.clone()],
            session_id: message.session_id.clone(),
            session_display_name: message.session_display_name.clone(),
            message_class: message.message_class.clone(),
            source_text: compact_text(&message.text),
            spoken_text: String::new(),
            notification_text: String::new(),
            voice: voice_for_session(&message.session_id),
            ts: message.ts,
            summary_word_target: Some(15),
        })
    } else {
        None
    };

    let Some(task) = task else {
        write_delivery_ledger(config, &mut ledger)?;
        return Ok(());
    };
    if listener_count == 0 {
        mark_tasks_skipped_no_listener(&mut ledger, &task);
        write_delivery_ledger(config, &mut ledger)?;
        return Ok(());
    }
    if let Err(err) = process_audio_task(config, state, &settings, &task, &mut ledger).await {
        state.hls.set_last_error(err.clone());
        set_task_error(&mut ledger, &task, &err);
    }
    write_delivery_ledger(config, &mut ledger)
}

async fn prepare_final_response(
    config: &RuntimeConfig,
    state: &mut VoiceDeliveryState,
    settings: &VoiceSettings,
    message: &VoiceInboxMessage,
    ledger: &mut HashMap<String, Value>,
) -> Result<Option<AnnouncementTask>, String> {
    let source_text = compact_text(&message.text);
    let mut summary_text = String::new();
    let mut notification_text = clip_text(&source_text, 120);
    if !settings.api_key.is_empty() {
        match OpenAiCompatibleClient::new()?
            .summarize(
                settings,
                &message.session_display_name,
                "Final assistant response",
                &message.text,
                30,
            )
            .await
        {
            Ok(value) => {
                summary_text = value;
                patch_ledger_row(
                    ledger,
                    &message.message_id,
                    json!({
                        "notification_text": notification_text,
                        "summary_status": "sent",
                        "summary_text": summary_text,
                    }),
                );
                notification_text = clip_text(&summary_text, 120);
                patch_ledger_row(
                    ledger,
                    &message.message_id,
                    json!({ "notification_text": notification_text }),
                );
            }
            Err(err) => {
                patch_ledger_row(
                    ledger,
                    &message.message_id,
                    json!({
                        "summary_status": "error",
                        "push_status": "error",
                        "narrated_status": if settings.final_response { "error" } else { "skipped" },
                        "last_error": clip_text(&err, 400),
                    }),
                );
                state.hls.set_last_error(err);
                return Ok(None);
            }
        }
    } else {
        patch_ledger_row(
            ledger,
            &message.message_id,
            json!({
                "summary_status": "skipped",
                "notification_text": notification_text,
            }),
        );
    }

    send_push_notifications(config, message, &notification_text, ledger).await?;
    if !settings.final_response {
        patch_ledger_row(
            ledger,
            &message.message_id,
            json!({ "narrated_status": "skipped" }),
        );
        return Ok(None);
    }
    if settings.api_key.is_empty() {
        patch_ledger_row(
            ledger,
            &message.message_id,
            json!({
                "narrated_status": "error",
                "last_error": "tts_api_key is required",
            }),
        );
        return Ok(None);
    }
    let spoken_basis = if summary_text.is_empty() {
        source_text.clone()
    } else {
        summary_text.clone()
    };
    Ok(Some(AnnouncementTask {
        message_id: message.message_id.clone(),
        source_message_ids: vec![message.message_id.clone()],
        session_id: message.session_id.clone(),
        session_display_name: message.session_display_name.clone(),
        message_class: "final_response".to_string(),
        source_text,
        spoken_text: format!(
            "Turn summary from {}. {}",
            message.session_display_name, spoken_basis
        ),
        notification_text,
        voice: voice_for_session(&message.session_id),
        ts: message.ts,
        summary_word_target: None,
    }))
}

async fn process_audio_task(
    _config: &RuntimeConfig,
    state: &mut VoiceDeliveryState,
    settings: &VoiceSettings,
    task: &AnnouncementTask,
    ledger: &mut HashMap<String, Value>,
) -> Result<(), String> {
    patch_task_rows(ledger, task, json!({ "voice": task.voice }));
    if settings.api_key.is_empty() {
        patch_task_rows(
            ledger,
            task,
            json!({
                "narrated_status": "error",
                "summary_status": if task.summary_word_target.is_some() { "error" } else { "pending" },
                "last_error": "tts_api_key is required",
            }),
        );
        return Ok(());
    }

    let mut spoken_text = task.spoken_text.clone();
    if let Some(target_words) = task.summary_word_target {
        let compact_source = compact_text(&task.source_text);
        let source_words = compact_source.split_whitespace().count();
        if source_words > 0 && source_words < target_words {
            patch_task_rows(
                ledger,
                task,
                json!({ "summary_status": "skipped", "summary_text": "" }),
            );
            spoken_text = format!("From {}. {}", task.session_display_name, compact_source);
        } else {
            let summary = OpenAiCompatibleClient::new()?
                .summarize(
                    settings,
                    &task.session_display_name,
                    "Narration updates",
                    &task.source_text,
                    target_words,
                )
                .await?;
            patch_task_rows(
                ledger,
                task,
                json!({ "summary_status": "sent", "summary_text": summary }),
            );
            spoken_text = format!("From {}. {}", task.session_display_name, summary);
        }
    }

    let audio = OpenAiCompatibleClient::new()?
        .synthesize(settings, &task.voice, &spoken_text)
        .await?;
    let duration = state.hls.append_audio(&task.message_id, &audio)?;
    let _ = duration;
    state.hls.set_last_error(String::new());
    patch_task_rows(ledger, task, json!({ "narrated_status": "sent" }));
    Ok(())
}

async fn send_push_notifications(
    config: &RuntimeConfig,
    message: &VoiceInboxMessage,
    notification_text: &str,
    ledger: &mut HashMap<String, Value>,
) -> Result<(), String> {
    let path = push_subscriptions_path(config);
    let mut records = read_subscription_records(config)?;
    let subscriptions = records
        .values()
        .filter(|record| {
            record
                .get("notifications_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                && record.get("device_class").and_then(Value::as_str) == Some("mobile")
        })
        .cloned()
        .collect::<Vec<_>>();
    if subscriptions.is_empty() {
        patch_ledger_row(
            ledger,
            &message.message_id,
            json!({ "push_status": "skipped" }),
        );
        return Ok(());
    }

    ensure_vapid_private_key(config)?;
    let subject = default_vapid_subject();
    let payload = json!({
        "session_id": message.session_id,
        "session_display_name": message.session_display_name,
        "message_id": message.message_id,
        "notification_text": notification_text,
        "timestamp": message.ts.unwrap_or_else(epoch_now),
    })
    .to_string();
    let mut any_success = false;
    let mut dropped = Vec::new();
    for record in subscriptions {
        let record_id = record
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let Some(subscription) = record.get("subscription").and_then(Value::as_object) else {
            continue;
        };
        let endpoint = subscription
            .get("endpoint")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let keys = subscription
            .get("keys")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let p256dh = keys
            .get("p256dh")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let auth = keys.get("auth").and_then(Value::as_str).unwrap_or_default();
        let subscription_info = SubscriptionInfo::new(endpoint, p256dh, auth);
        let send_result = send_one_web_push(
            &subscription_info,
            &vapid_private_key_path(config),
            &subject,
            &payload,
        )
        .await;
        let now_ts = epoch_now();
        match send_result {
            Ok(()) => {
                if let Some(current) = records.get_mut(&record_id).and_then(Value::as_object_mut) {
                    current.insert("last_success_ts".to_string(), json!(now_ts));
                    current.insert("last_error".to_string(), json!(""));
                    current.insert("updated_ts".to_string(), json!(now_ts));
                }
                any_success = true;
            }
            Err(err) => {
                if let Some(current) = records.get_mut(&record_id).and_then(Value::as_object_mut) {
                    current.insert("last_failure_ts".to_string(), json!(now_ts));
                    current.insert(
                        "last_error".to_string(),
                        json!(clip_text(&err.to_string(), 400)),
                    );
                    current.insert("updated_ts".to_string(), json!(now_ts));
                }
                if err.drop_subscription {
                    dropped.push(record_id);
                }
            }
        }
    }
    for record_id in dropped {
        records.remove(&record_id);
    }
    write_subscription_records(&path, &records)?;
    patch_ledger_row(
        ledger,
        &message.message_id,
        json!({ "push_status": if any_success { "sent" } else { "error" } }),
    );
    Ok(())
}

async fn send_one_web_push(
    subscription_info: &SubscriptionInfo,
    private_key_path: &Path,
    subject: &str,
    payload: &str,
) -> Result<(), WebPushSendError> {
    let file = fs::File::open(private_key_path).map_err(WebPushSendError::retain)?;
    let mut sig_builder = VapidSignatureBuilder::from_pem(file, subscription_info)
        .map_err(WebPushSendError::retain)?;
    sig_builder.add_claim("sub", subject);
    let signature = sig_builder.build().map_err(WebPushSendError::retain)?;
    let mut builder = WebPushMessageBuilder::new(subscription_info);
    builder.set_ttl(300);
    builder.set_payload(ContentEncoding::Aes128Gcm, payload.as_bytes());
    builder.set_vapid_signature(signature);
    let message = builder.build().map_err(WebPushSendError::retain)?;
    send_web_push_request(message).await
}

#[derive(Debug)]
struct WebPushSendError {
    message: String,
    drop_subscription: bool,
}

impl WebPushSendError {
    fn retain(error: impl std::fmt::Display) -> Self {
        Self {
            message: error.to_string(),
            drop_subscription: false,
        }
    }

    fn with_status(status: u16, body: String) -> Self {
        Self {
            message: format!("web push failed with {status}: {body}"),
            drop_subscription: status == 404 || status == 410,
        }
    }
}

impl std::fmt::Display for WebPushSendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[derive(Debug)]
struct WebPushRequestBody(Vec<u8>);

impl From<Vec<u8>> for WebPushRequestBody {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<&'static str> for WebPushRequestBody {
    fn from(value: &'static str) -> Self {
        Self(value.as_bytes().to_vec())
    }
}

async fn send_web_push_request(message: WebPushMessage) -> Result<(), WebPushSendError> {
    let request = request_builder::build_request::<WebPushRequestBody>(message);
    let (parts, body) = request.into_parts();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(WebPushSendError::retain)?;
    let response = client
        .request(parts.method, parts.uri.to_string())
        .headers(parts.headers)
        .body(body.0)
        .send()
        .await
        .map_err(WebPushSendError::retain)?;
    let status = response.status();
    if status.is_success() || status.as_u16() == 201 {
        return Ok(());
    }
    let text = response
        .text()
        .await
        .unwrap_or_else(|_| "<unreadable response>".to_string());
    Err(WebPushSendError::with_status(status.as_u16(), text))
}

impl OpenAiCompatibleClient {
    fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|err| format!("build http client: {err}"))?;
        Ok(Self { client })
    }

    async fn summarize(
        &self,
        settings: &VoiceSettings,
        session_name: &str,
        source_label: &str,
        text: &str,
        target_words: usize,
    ) -> Result<String, String> {
        if settings.api_key.is_empty() {
            return Err("tts_api_key is required".to_string());
        }
        let max_words = if target_words <= 15 { 15 } else { 30 };
        let system_content = if max_words <= 15 {
            "You compress assistant progress narration for spoken mobile notifications. Return exactly one plain sentence with only the concrete progress fact. Aim for about 15 words, roughly 12 to 18 words. Use at most 15 words. If the source is already 15 words or fewer, do not expand it. Compression only: never add filler, politeness, waiting language, stage directions, or meta-commentary. No markdown, no quotes, no prefixes."
        } else {
            "You compress assistant final responses for spoken mobile notifications. Return exactly one plain sentence with only the main result. Aim for about 30 words, roughly 24 to 36 words. Use at most 30 words. Prefer compression over paraphrase. Never add filler, politeness, stage directions, or meta-commentary, and never invent details not present in the source. No markdown, no quotes, no prefixes."
        };
        let payload = json!({
            "model": settings.summarization_model,
            "temperature": 0.0,
            "max_completion_tokens": if max_words <= 15 { 48 } else { 72 },
            "messages": [
                {"role": "system", "content": system_content},
                {"role": "user", "content": format!("Session name: {session_name}\n{source_label}:\n{text}")},
            ],
        });
        let obj = self
            .request_json(
                &settings.base_url,
                "/chat/completions",
                &settings.api_key,
                &payload,
            )
            .await?;
        let choices = obj
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(|| "chat completions response missing choices".to_string())?;
        let message = choices
            .first()
            .and_then(|choice| choice.get("message"))
            .and_then(Value::as_object)
            .ok_or_else(|| "chat completions response missing message".to_string())?;
        let summary = if let Some(content) = message.get("content").and_then(Value::as_str) {
            compact_text(content)
        } else if let Some(items) = message.get("content").and_then(Value::as_array) {
            compact_text(
                items
                    .iter()
                    .filter_map(|item| {
                        let item_type = item.get("type").and_then(Value::as_str)?;
                        if item_type != "text" && item_type != "output_text" {
                            return None;
                        }
                        item.get("text").and_then(Value::as_str)
                    })
                    .collect::<Vec<_>>()
                    .join(""),
            )
        } else {
            return Err("chat completions response missing content".to_string());
        };
        if summary.is_empty() {
            return Err("empty summary response".to_string());
        }
        if summary.split_whitespace().count() > max_words {
            return Err(format!("summary exceeded {max_words} words"));
        }
        Ok(summary)
    }

    async fn synthesize(
        &self,
        settings: &VoiceSettings,
        voice: &str,
        text: &str,
    ) -> Result<Vec<u8>, String> {
        if settings.api_key.is_empty() {
            return Err("tts_api_key is required".to_string());
        }
        let payload = json!({
            "model": settings.tts_model,
            "voice": voice,
            "input": text,
            "response_format": "aac",
        });
        let audio = self
            .request_bytes(
                &settings.base_url,
                "/audio/speech",
                &settings.api_key,
                &payload,
            )
            .await?;
        if audio.is_empty() {
            return Err("audio/speech returned empty body".to_string());
        }
        Ok(audio)
    }

    async fn request_json(
        &self,
        base_url: &str,
        route: &str,
        api_key: &str,
        payload: &Value,
    ) -> Result<Value, String> {
        let response = self
            .client
            .post(format!("{}{}", base_url.trim_end_matches('/'), route))
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .json(payload)
            .send()
            .await
            .map_err(|err| format!("{route} failed: {err}"))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| format!("{route} failed reading response: {err}"))?;
        if !status.is_success() {
            return Err(format!("{route} failed with {}: {text}", status.as_u16()));
        }
        serde_json::from_str(&text).map_err(|err| format!("{route} returned invalid json: {err}"))
    }

    async fn request_bytes(
        &self,
        base_url: &str,
        route: &str,
        api_key: &str,
        payload: &Value,
    ) -> Result<Vec<u8>, String> {
        let response = self
            .client
            .post(format!("{}{}", base_url.trim_end_matches('/'), route))
            .bearer_auth(api_key)
            .header("Accept", "application/octet-stream")
            .json(payload)
            .send()
            .await
            .map_err(|err| format!("{route} failed: {err}"))?;
        let status = response.status();
        if !status.is_success() {
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable response>".to_string());
            return Err(format!("{route} failed with {}: {text}", status.as_u16()));
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|err| format!("{route} failed reading response: {err}"))
    }
}

impl MergedHlsStream {
    fn new(root_dir: PathBuf) -> Result<Self, String> {
        let segments_dir = root_dir.join("segments");
        fs::create_dir_all(&segments_dir)
            .map_err(|err| format!("mkdir {}: {err}", segments_dir.display()))?;
        let mut stream = Self {
            playlist_path: root_dir.join("live.m3u8"),
            root_dir,
            segments_dir,
            segments: Vec::new(),
            next_seq: 1,
            last_error: String::new(),
            last_append: None,
        };
        stream.rewrite_playlist()?;
        Ok(stream)
    }

    fn snapshot(&self) -> Value {
        json!({
            "queue_depth": 0,
            "active_listener_count": 0,
            "segment_count": self.segments.len(),
            "last_error": self.last_error,
            "media_sequence": self.segments.first().map(|item| item.seq).unwrap_or(self.next_seq),
        })
    }

    fn set_last_error(&mut self, error: String) {
        self.last_error = error.trim().to_string();
    }

    fn append_audio(&mut self, message_id: &str, audio_bytes: &[u8]) -> Result<f64, String> {
        require_command("ffmpeg")?;
        require_command("ffprobe")?;
        fs::create_dir_all(&self.segments_dir)
            .map_err(|err| format!("mkdir {}: {err}", self.segments_dir.display()))?;
        let prefix = segment_prefix(message_id);
        let input_path = self.segments_dir.join(format!("{prefix}.aac"));
        fs::write(&input_path, audio_bytes)
            .map_err(|err| format!("write {}: {err}", input_path.display()))?;
        let tmp_pattern = self.segments_dir.join(format!("{prefix}-part-%03d.ts"));
        let output = Command::new("ffmpeg")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-y")
            .arg("-i")
            .arg(&input_path)
            .arg("-vn")
            .arg("-c:a")
            .arg("aac")
            .arg("-b:a")
            .arg("128k")
            .arg("-f")
            .arg("segment")
            .arg("-segment_time")
            .arg("6")
            .arg("-segment_format")
            .arg("mpegts")
            .arg("-reset_timestamps")
            .arg("1")
            .arg(&tmp_pattern)
            .output()
            .map_err(|err| format!("spawn ffmpeg: {err}"))?;
        let _ = fs::remove_file(&input_path);
        if !output.status.success() {
            return Err(format!(
                "ffmpeg failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let mut chunk_paths = fs::read_dir(&self.segments_dir)
            .map_err(|err| format!("read_dir {}: {err}", self.segments_dir.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| {
                        name.starts_with(&format!("{prefix}-part-")) && name.ends_with(".ts")
                    })
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        chunk_paths.sort();
        if chunk_paths.is_empty() {
            return Err("ffmpeg produced no HLS segments".to_string());
        }
        let mut total_duration = 0.0;
        for chunk_path in chunk_paths {
            let duration = match segment_duration_seconds(&chunk_path) {
                Ok(value) => value,
                Err(err) if err.contains("invalid ffprobe duration: N/A") => {
                    let _ = fs::remove_file(&chunk_path);
                    continue;
                }
                Err(err) => return Err(err),
            };
            let (seq, segment_name, segment_path) = self.reserve_segment(&prefix);
            fs::rename(&chunk_path, &segment_path)
                .map_err(|err| format!("rename {}: {err}", segment_path.display()))?;
            total_duration += duration;
            self.store_segment(seq, segment_name, segment_path, duration)?;
        }
        if total_duration <= 0.0 {
            return Err("ffmpeg produced no valid HLS segments".to_string());
        }
        Ok(total_duration)
    }

    fn append_silence(&mut self, force: bool) -> Result<bool, String> {
        require_command("ffmpeg")?;
        require_command("ffprobe")?;
        if !force {
            if let Some(last_append) = self.last_append {
                if last_append.elapsed().as_secs_f64() < HLS_KEEPALIVE_SECONDS {
                    return Ok(false);
                }
            }
        }
        let (seq, segment_name, segment_path) = self.reserve_segment("silence");
        let output = Command::new("ffmpeg")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("anullsrc=r=24000:cl=mono")
            .arg("-t")
            .arg(HLS_SILENCE_SECONDS.to_string())
            .arg("-c:a")
            .arg("aac")
            .arg("-b:a")
            .arg("32k")
            .arg("-f")
            .arg("mpegts")
            .arg(&segment_path)
            .output()
            .map_err(|err| format!("spawn ffmpeg: {err}"))?;
        if !output.status.success() {
            return Err(format!(
                "ffmpeg failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let duration = segment_duration_seconds(&segment_path)?;
        self.store_segment(seq, segment_name, segment_path, duration)?;
        Ok(true)
    }

    fn reserve_segment(&mut self, prefix: &str) -> (u64, String, PathBuf) {
        let seq = self.next_seq;
        self.next_seq += 1;
        let segment_name = format!(
            "{seq:06}-{}.ts",
            prefix.chars().take(12).collect::<String>()
        );
        let segment_path = self.segments_dir.join(&segment_name);
        (seq, segment_name, segment_path)
    }

    fn store_segment(
        &mut self,
        seq: u64,
        name: String,
        path: PathBuf,
        duration: f64,
    ) -> Result<(), String> {
        self.segments.push(HlsSegment {
            seq,
            name,
            duration,
            path,
        });
        self.segments.sort_by_key(|item| item.seq);
        while self.segments.len() > HLS_MAX_SEGMENTS {
            let old = self.segments.remove(0);
            let _ = fs::remove_file(old.path);
        }
        self.last_append = Some(Instant::now());
        self.rewrite_playlist()
    }

    fn rewrite_playlist(&self) -> Result<(), String> {
        fs::create_dir_all(&self.root_dir)
            .map_err(|err| format!("mkdir {}: {err}", self.root_dir.display()))?;
        let longest = self
            .segments
            .iter()
            .map(|item| item.duration.ceil() as i64)
            .max()
            .unwrap_or(0);
        let target_duration = HLS_TARGET_DURATION_SECONDS.max(longest);
        let mut lines = vec![
            "#EXTM3U".to_string(),
            "#EXT-X-VERSION:3".to_string(),
            format!("#EXT-X-TARGETDURATION:{target_duration}"),
            format!(
                "#EXT-X-MEDIA-SEQUENCE:{}",
                self.segments
                    .first()
                    .map(|item| item.seq)
                    .unwrap_or(self.next_seq)
            ),
        ];
        for segment in &self.segments {
            lines.push(format!("#EXTINF:{:.3},", segment.duration));
            lines.push(format!("segments/{}", segment.name));
        }
        write_text_atomic(&self.playlist_path, &(lines.join("\n") + "\n"))
    }
}

fn read_voice_inbox_messages(config: &RuntimeConfig) -> Result<Vec<VoiceInboxMessage>, String> {
    let inbox = voice_inbox_dir(config);
    if !inbox.exists() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(&inbox)
        .map_err(|err| format!("read_dir {}: {err}", inbox.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    paths.into_iter().map(read_voice_inbox_message).collect()
}

fn read_voice_inbox_message(path: PathBuf) -> Result<VoiceInboxMessage, String> {
    let value = read_json_value(&path)?;
    let session_id = string_value(&value, "session_id");
    let session_display_name =
        default_session_display_name(string_value(&value, "session_display_name"));
    let message_id = string_value(&value, "message_id");
    let message_class = string_value(&value, "message_class");
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if session_id.is_empty()
        || message_id.is_empty()
        || (message_class != "narration" && message_class != "final_response")
        || text.trim().is_empty()
    {
        return Err(format!("invalid voice inbox payload: {}", path.display()));
    }
    let ts = value.get("ts").and_then(Value::as_f64);
    Ok(VoiceInboxMessage {
        message_id,
        session_id,
        session_display_name,
        message_class,
        text,
        ts,
        inbox_path: path,
    })
}

fn maybe_append_keepalive(
    config: &RuntimeConfig,
    state: &mut VoiceDeliveryState,
) -> Result<(), String> {
    if read_listener_records(config)?.is_empty() {
        return Ok(());
    }
    if !read_voice_inbox_messages(config)?.is_empty() {
        return Ok(());
    }
    match state.hls.append_silence(false) {
        Ok(_) => Ok(()),
        Err(err) => {
            state.hls.set_last_error(err);
            Ok(())
        }
    }
}

fn save_runtime_snapshot(config: &RuntimeConfig, state: &VoiceDeliveryState) -> Result<(), String> {
    let mut audio = state.hls.snapshot();
    let active_listener_count = read_listener_records(config)?.len();
    audio["active_listener_count"] = json!(active_listener_count);
    let payload = json!({
        "audio": audio,
        "updated_ts": epoch_now(),
    });
    write_json_pretty_atomic(&voice_runtime_path(config), &payload)
}

fn read_voice_settings(config: &RuntimeConfig) -> Result<VoiceSettings, String> {
    let value =
        read_optional_json_value(&voice_settings_path(config))?.unwrap_or_else(|| json!({}));
    let base_url = string_value(&value, "tts_base_url");
    let base_url = if base_url.is_empty() {
        DEFAULT_TTS_BASE_URL.to_string()
    } else if base_url.starts_with("http://") || base_url.starts_with("https://") {
        base_url.trim_end_matches('/').to_string()
    } else {
        return Err("tts_base_url must start with http:// or https://".to_string());
    };
    let summarization_model = nonempty_or_default(
        string_value(&value, "summarization_model"),
        DEFAULT_SUMMARIZATION_MODEL,
    );
    let tts_model = nonempty_or_default(string_value(&value, "tts_model"), DEFAULT_TTS_MODEL);
    Ok(VoiceSettings {
        narration: value
            .get("tts_enabled_for_narration")
            .map(json_truthy)
            .unwrap_or(false),
        final_response: value
            .get("tts_enabled_for_final_response")
            .map(json_truthy)
            .unwrap_or(false),
        base_url,
        api_key: string_value(&value, "tts_api_key"),
        summarization_model,
        tts_model,
    })
}

fn read_subscription_records(config: &RuntimeConfig) -> Result<HashMap<String, Value>, String> {
    let Some(Value::Array(items)) = read_optional_json_value(&push_subscriptions_path(config))?
    else {
        return Ok(HashMap::new());
    };
    let now_ts = epoch_now();
    let mut records = HashMap::new();
    for item in items {
        let Some(record) = clean_subscription_record(&item, now_ts) else {
            continue;
        };
        let Some(record_id) = record.get("id").and_then(Value::as_str) else {
            continue;
        };
        records.insert(record_id.to_string(), record);
    }
    Ok(records)
}

fn clean_subscription_record(raw: &Value, now_ts: f64) -> Option<Value> {
    let object = raw.as_object()?;
    let subscription = object.get("subscription")?.as_object()?;
    let endpoint = subscription.get("endpoint")?.as_str()?.trim();
    if endpoint.is_empty() {
        return None;
    }
    let keys = subscription.get("keys")?.as_object()?;
    let p256dh = keys.get("p256dh")?.as_str()?.trim();
    let auth = keys.get("auth")?.as_str()?.trim();
    if p256dh.is_empty() || auth.is_empty() {
        return None;
    }
    let user_agent = string_value(raw, "user_agent");
    let device_class = clean_device_class(&string_value(raw, "device_class"), &user_agent);
    Some(json!({
        "id": subscription_id(endpoint),
        "subscription": {
            "endpoint": endpoint,
            "keys": {
                "p256dh": p256dh,
                "auth": auth,
            },
        },
        "notifications_enabled": raw.get("notifications_enabled").map(json_truthy).unwrap_or(true),
        "created_ts": raw.get("created_ts").and_then(Value::as_f64).unwrap_or(now_ts),
        "updated_ts": raw.get("updated_ts").and_then(Value::as_f64).unwrap_or(now_ts),
        "last_success_ts": raw.get("last_success_ts").and_then(Value::as_f64),
        "last_failure_ts": raw.get("last_failure_ts").and_then(Value::as_f64),
        "last_error": string_value(raw, "last_error"),
        "user_agent": user_agent,
        "device_label": string_value(raw, "device_label"),
        "device_class": device_class,
    }))
}

fn write_subscription_records(path: &Path, records: &HashMap<String, Value>) -> Result<(), String> {
    let mut ids = records.keys().cloned().collect::<Vec<_>>();
    ids.sort();
    let values = ids
        .iter()
        .filter_map(|id| records.get(id).cloned())
        .collect::<Vec<_>>();
    write_json_pretty_atomic(path, &Value::Array(values))
}

fn read_listener_records(config: &RuntimeConfig) -> Result<HashMap<String, f64>, String> {
    let Some(Value::Object(object)) = read_optional_json_value(&voice_listeners_path(config))?
    else {
        return Ok(HashMap::new());
    };
    let now_ts = epoch_now();
    let mut records = HashMap::new();
    for (client_id, seen_at) in object {
        let client_id = client_id.trim();
        let Some(seen_ts) = seen_at.as_f64().filter(|value| value.is_finite()) else {
            continue;
        };
        if client_id.is_empty() || now_ts - seen_ts > LISTENER_TTL_SECONDS {
            continue;
        }
        records.insert(client_id.to_string(), seen_ts);
    }
    Ok(records)
}

fn read_delivery_ledger(config: &RuntimeConfig) -> Result<HashMap<String, Value>, String> {
    let Some(Value::Object(object)) =
        read_optional_json_value(&voice_delivery_ledger_path(config))?
    else {
        return Ok(HashMap::new());
    };
    Ok(object.into_iter().collect())
}

fn write_delivery_ledger(
    config: &RuntimeConfig,
    ledger: &mut HashMap<String, Value>,
) -> Result<(), String> {
    trim_ledger(ledger);
    let mut ids = ledger.keys().cloned().collect::<Vec<_>>();
    ids.sort();
    let mut object = serde_json::Map::new();
    for id in ids {
        if let Some(value) = ledger.get(&id) {
            object.insert(id, value.clone());
        }
    }
    write_json_pretty_atomic(&voice_delivery_ledger_path(config), &Value::Object(object))
}

fn trim_ledger(ledger: &mut HashMap<String, Value>) {
    if ledger.len() <= DELIVERY_LEDGER_MAX {
        return;
    }
    let mut rows = ledger
        .values()
        .filter_map(|row| {
            Some((
                string_value(row, "message_id"),
                row.get("updated_ts")
                    .or_else(|| row.get("created_ts"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
            ))
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    for (message_id, _) in rows.into_iter().take(ledger.len() - DELIVERY_LEDGER_MAX) {
        ledger.remove(&message_id);
    }
}

fn patch_ledger_row(ledger: &mut HashMap<String, Value>, message_id: &str, patch: Value) {
    let now_ts = epoch_now();
    let Some(row) = ledger.get_mut(message_id).and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(patch) = patch.as_object() {
        for (key, value) in patch {
            row.insert(key.to_string(), value.clone());
        }
    }
    row.insert("updated_ts".to_string(), json!(now_ts));
}

fn patch_task_rows(ledger: &mut HashMap<String, Value>, task: &AnnouncementTask, patch: Value) {
    for message_id in task.source_message_ids.iter() {
        patch_ledger_row(ledger, message_id, patch.clone());
    }
}

fn mark_tasks_skipped_no_listener(ledger: &mut HashMap<String, Value>, task: &AnnouncementTask) {
    for message_id in task.source_message_ids.iter() {
        let Some(row) = ledger.get_mut(message_id).and_then(Value::as_object_mut) else {
            continue;
        };
        row.insert("narrated_status".to_string(), json!("skipped"));
        row.insert("last_error".to_string(), json!("no active listener"));
        row.insert("updated_ts".to_string(), json!(epoch_now()));
        if row.get("summary_status").and_then(Value::as_str) == Some("pending") {
            row.insert("summary_status".to_string(), json!("skipped"));
        }
    }
}

fn set_task_error(ledger: &mut HashMap<String, Value>, task: &AnnouncementTask, error: &str) {
    for message_id in task.source_message_ids.iter() {
        let Some(row) = ledger.get_mut(message_id).and_then(Value::as_object_mut) else {
            continue;
        };
        row.insert("last_error".to_string(), json!(clip_text(error, 400)));
        row.insert("updated_ts".to_string(), json!(epoch_now()));
        row.insert("narrated_status".to_string(), json!("error"));
        if row.get("summary_status").and_then(Value::as_str) == Some("pending") {
            row.insert("summary_status".to_string(), json!("error"));
        }
        if row.get("push_status").and_then(Value::as_str) == Some("pending") {
            row.insert("push_status".to_string(), json!("error"));
        }
    }
}

fn ensure_vapid_private_key(config: &RuntimeConfig) -> Result<(), String> {
    let path = vapid_private_key_path(config);
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("mkdir {}: {err}", parent.display()))?;
    }
    let output = Command::new("openssl")
        .arg("ecparam")
        .arg("-name")
        .arg("prime256v1")
        .arg("-genkey")
        .arg("-noout")
        .output()
        .map_err(|err| format!("spawn openssl ecparam: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "openssl ecparam failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    fs::write(&path, output.stdout).map_err(|err| format!("write {}: {err}", path.display()))
}

fn default_vapid_subject() -> String {
    env::var("CODEX_WEB_PUSH_VAPID_SUBJECT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| {
            value.starts_with("mailto:")
                || value.starts_with("http://")
                || value.starts_with("https://")
        })
        .unwrap_or_else(|| DEFAULT_VAPID_SUBJECT.to_string())
}

fn segment_duration_seconds(path: &Path) -> Result<f64, String> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path)
        .output()
        .map_err(|err| format!("spawn ffprobe: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let value = raw
        .parse::<f64>()
        .map_err(|_| format!("invalid ffprobe duration: {raw}"))?;
    Ok(value.max(0.2))
}

fn require_command(name: &str) -> Result<(), String> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name} >/dev/null 2>&1"))
        .status()
        .map_err(|err| format!("spawn command -v {name}: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("ffmpeg and ffprobe are required for merged HLS output".to_string())
    }
}

fn write_json_pretty_atomic(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|err| format!("serialize {}: {err}", path.display()))?;
    write_text_atomic(path, &(text + "\n"))
}

fn write_text_atomic(path: &Path, text: &str) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|err| format!("mkdir {}: {err}", parent.display()))?;
    let tmp_path = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("voice"),
        process_id()
    ));
    {
        let mut file = fs::File::create(&tmp_path)
            .map_err(|err| format!("create {}: {err}", tmp_path.display()))?;
        file.write_all(text.as_bytes())
            .map_err(|err| format!("write {}: {err}", tmp_path.display()))?;
    }
    fs::rename(&tmp_path, path)
        .map_err(|err| format!("rename {} -> {}: {err}", tmp_path.display(), path.display()))
}

fn read_optional_json_value(path: &Path) -> Result<Option<Value>, String> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|err| format!("parse {}: {err}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("read {}: {err}", path.display())),
    }
}

fn read_json_value(path: &Path) -> Result<Value, String> {
    read_optional_json_value(path)?.ok_or_else(|| format!("missing {}", path.display()))
}

fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_i64().map(|value| value != 0).unwrap_or(false),
        Value::String(value) => {
            let value = value.trim().to_ascii_lowercase();
            !value.is_empty() && value != "0" && value != "false" && value != "no" && value != "off"
        }
        _ => false,
    }
}

fn string_value(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn nonempty_or_default(value: String, default_value: &str) -> String {
    if value.trim().is_empty() {
        default_value.to_string()
    } else {
        value.trim().to_string()
    }
}

fn compact_text(raw: impl AsRef<str>) -> String {
    raw.as_ref()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn clip_text(raw: impl AsRef<str>, limit: usize) -> String {
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
    let value = compact_text(raw);
    if value.is_empty() {
        "Session".to_string()
    } else {
        value
    }
}

fn clean_device_class(raw: &str, user_agent: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "mobile" => "mobile".to_string(),
        "desktop" => "desktop".to_string(),
        _ => {
            let ua = user_agent.to_ascii_lowercase();
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
    }
}

fn subscription_id(endpoint: &str) -> String {
    Sha256::digest(endpoint.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        .chars()
        .take(24)
        .collect()
}

fn voice_for_session(session_id: &str) -> String {
    let digest = Sha256::digest(session_id.as_bytes());
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&digest[..4]);
    let index = u32::from_be_bytes(bytes) as usize % DEFAULT_VOICES.len();
    DEFAULT_VOICES[index].to_string()
}

fn segment_prefix(message_id: &str) -> String {
    let value = message_id.chars().take(12).collect::<String>();
    if value.is_empty() {
        "audio".to_string()
    } else {
        value
    }
}

fn epoch_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn process_id() -> u32 {
    std::process::id()
}

fn voice_worker_interval() -> Duration {
    let seconds = env::var("CODEX_WEB_VOICE_PUSH_SWEEP_SECONDS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| *value > 0.0)
        .unwrap_or(1.0);
    Duration::from_secs_f64(seconds)
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

fn voice_inbox_dir(config: &RuntimeConfig) -> PathBuf {
    config.app_dir.join("voice_inbox")
}

fn audio_root_dir(config: &RuntimeConfig) -> PathBuf {
    config.app_dir.join("audio")
}

fn vapid_private_key_path(config: &RuntimeConfig) -> PathBuf {
    config.app_dir.join("webpush_vapid_private.pem")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;
    use std::thread;

    fn temp_app_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "codoxear-rs-voice-worker-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn start_fake_openai(request_count: usize) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let mut paths = Vec::new();
            for _ in 0..request_count {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                let mut content_length = 0usize;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.strip_prefix("Content-Length:") {
                        content_length = value.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; content_length];
                reader.read_exact(&mut body).unwrap();
                let body = if path == "/chat/completions" {
                    b"{\"choices\":[{\"message\":{\"content\":\"short final summary\"}}]}".to_vec()
                } else {
                    b"fakeaudio".to_vec()
                };
                let content_type = if path == "/chat/completions" {
                    "application/json"
                } else {
                    "application/octet-stream"
                };
                let mut response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .into_bytes();
                response.extend_from_slice(&body);
                stream.write_all(&response).unwrap();
                paths.push(path);
            }
            paths
        });
        (format!("http://{address}"), handle)
    }

    #[tokio::test]
    async fn voice_worker_summarizes_final_and_marks_tts_error_when_ffmpeg_fails() {
        let app_dir = temp_app_dir("summary-final");
        let config = RuntimeConfig {
            app_dir: app_dir.clone(),
        };
        fs::create_dir_all(app_dir.join("voice_inbox")).unwrap();
        fs::write(
            app_dir.join("voice_listeners.json"),
            r#"{"listener-a":9999999999.0}"#,
        )
        .unwrap();
        let (base_url, server) = start_fake_openai(2);
        fs::write(
            app_dir.join("voice_settings.json"),
            json!({
                "tts_enabled_for_narration": false,
                "tts_enabled_for_final_response": true,
                "tts_base_url": base_url,
                "tts_api_key": "key",
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            app_dir.join("voice_inbox").join("msg.json"),
            r#"{"message_id":"msg-1","session_id":"sid-1","session_display_name":"Repo","message_class":"final_response","text":"Long final answer body","ts":4.0}"#,
        )
        .unwrap();
        let mut state = VoiceDeliveryState::new(&config).unwrap();

        run_voice_delivery_worker_once(&config, &mut state)
            .await
            .unwrap();

        let paths = server.join().unwrap();
        assert_eq!(paths, vec!["/chat/completions", "/audio/speech"]);
        let ledger: Value = serde_json::from_str(
            &fs::read_to_string(app_dir.join("voice_delivery_ledger.json")).unwrap(),
        )
        .unwrap();
        let row = &ledger["msg-1"];
        assert_eq!(row["summary_status"], "sent");
        assert_eq!(row["summary_text"], "short final summary");
        assert_eq!(row["notification_text"], "short final summary");
        assert!(
            row["narrated_status"] == "sent" || row["narrated_status"] == "error",
            "ffmpeg availability decides the final narrated status"
        );
    }

    #[tokio::test]
    async fn voice_worker_records_final_without_listener_after_summary() {
        let app_dir = temp_app_dir("summary-no-listener");
        let config = RuntimeConfig {
            app_dir: app_dir.clone(),
        };
        fs::create_dir_all(app_dir.join("voice_inbox")).unwrap();
        let (base_url, server) = start_fake_openai(1);
        fs::write(
            app_dir.join("voice_settings.json"),
            json!({
                "tts_enabled_for_narration": false,
                "tts_enabled_for_final_response": true,
                "tts_base_url": base_url,
                "tts_api_key": "key",
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            app_dir.join("voice_inbox").join("msg.json"),
            r#"{"message_id":"msg-1","session_id":"sid-1","session_display_name":"Repo","message_class":"final_response","text":"Long final answer body","ts":4.0}"#,
        )
        .unwrap();
        let mut state = VoiceDeliveryState::new(&config).unwrap();

        run_voice_delivery_worker_once(&config, &mut state)
            .await
            .unwrap();

        let paths = server.join().unwrap();
        assert_eq!(paths, vec!["/chat/completions"]);
        let ledger: Value = serde_json::from_str(
            &fs::read_to_string(app_dir.join("voice_delivery_ledger.json")).unwrap(),
        )
        .unwrap();
        let row = &ledger["msg-1"];
        assert_eq!(row["summary_status"], "sent");
        assert_eq!(row["narrated_status"], "skipped");
        assert_eq!(row["last_error"], "no active listener");
        assert!(!app_dir.join("voice_inbox").join("msg.json").exists());
    }

    #[tokio::test]
    async fn voice_worker_keeps_raw_final_when_no_api_or_push() {
        let app_dir = temp_app_dir("raw-final");
        let config = RuntimeConfig {
            app_dir: app_dir.clone(),
        };
        fs::create_dir_all(app_dir.join("voice_inbox")).unwrap();
        fs::write(
            app_dir.join("voice_settings.json"),
            r#"{"tts_enabled_for_narration":false,"tts_enabled_for_final_response":false,"tts_api_key":""}"#,
        )
        .unwrap();
        fs::write(
            app_dir.join("voice_inbox").join("msg.json"),
            r#"{"message_id":"msg-1","session_id":"sid-1","session_display_name":"Repo","message_class":"final_response","text":"Line one.\n\nLine two.","ts":4.0}"#,
        )
        .unwrap();
        let mut state = VoiceDeliveryState::new(&config).unwrap();

        assert_eq!(
            run_voice_delivery_worker_once(&config, &mut state)
                .await
                .unwrap(),
            1
        );

        let ledger: Value = serde_json::from_str(
            &fs::read_to_string(app_dir.join("voice_delivery_ledger.json")).unwrap(),
        )
        .unwrap();
        let row = &ledger["msg-1"];
        assert_eq!(row["summary_status"], "skipped");
        assert_eq!(row["push_status"], "skipped");
        assert_eq!(row["narrated_status"], "skipped");
        assert_eq!(row["notification_text"], "Line one. Line two.");
    }
}
