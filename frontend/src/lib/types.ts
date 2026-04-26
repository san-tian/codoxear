export type SessionSummary = {
  session_id: string;
  thread_id: string | null;
  pid: number;
  broker_pid: number;
  agent_backend: string;
  owned: boolean;
  transport: string | null;
  cwd: string;
  start_ts: number;
  updated_ts: number;
  log_path: string | null;
  queue_len: number;
  busy: boolean;
  token?: Record<string, unknown> | null;
  harness_enabled: boolean;
  harness_cooldown_minutes: number;
  harness_remaining_injections: number;
  alias: string;
  files: string[];
  git_branch: string | null;
  model_provider: string | null;
  preferred_auth_method: string | null;
  provider_choice: string | null;
  model: string | null;
  reasoning_effort: string | null;
  service_tier: string | null;
  tmux_session: string | null;
  tmux_window: string | null;
  priority_offset: number;
  snooze_until: number | null;
  dependency_session_id: string | null;
  time_priority: number;
  base_priority: number;
  final_priority: number;
  blocked: boolean;
  snoozed: boolean;
  last_assistant_ts?: number | null;
};

export type SessionsResponse = {
  app_version: string;
  sessions: SessionSummary[];
  recent_cwds: string[];
  new_session_defaults: NewSessionDefaults;
  tmux_available: boolean;
  tmux_session_name: string | null;
};

export type NewSessionBackendDefaults = {
  agent_backend: "codex" | "pi";
  model_provider: string | null;
  preferred_auth_method: string | null;
  provider_choice: string | null;
  provider_choices: string[];
  model: string | null;
  models?: string[];
  reasoning_effort: string;
  reasoning_efforts: string[];
  service_tier: string | null;
  supports_fast: boolean;
};

export type NewSessionDefaults = {
  default_backend: "codex" | "pi";
  backends: {
    codex: NewSessionBackendDefaults;
    pi: NewSessionBackendDefaults;
  };
};

export type ResumeCandidate = {
  session_id: string;
  alias?: string;
  first_user_message?: string;
  updated_ts?: number;
  log_path?: string | null;
};

export type ResumeCandidatesResponse = {
  ok: true;
  exists: boolean;
  will_create: boolean;
  git_repo: boolean;
  git_root: string;
  git_branch: string;
  sessions: ResumeCandidate[];
};

export type RawChatEvent =
  | {
      role: "user" | "assistant";
      text: string;
      ts?: number;
      pending?: boolean;
      localId?: string;
      message_class?: string;
      message_id?: string;
      notification_text?: string;
    }
  | {
      type: "tool" | "tool_result" | "ask_user";
      ts?: number;
      name?: string;
      text?: string;
      tool_call_id?: string;
      question?: string;
      context?: string;
      options?: string[];
      questions?: Array<Record<string, unknown>>;
      header?: string;
      answer?: string | string[];
      allow_freeform?: boolean;
      allow_multiple?: boolean;
      resolved?: boolean;
      cancelled?: boolean;
      was_custom?: boolean;
      timeout_ms?: number;
      is_error?: boolean;
    };

export type UiTranscriptEvent = {
  id: string;
  kind: "user" | "assistant" | "tool" | "tool_result" | "ask_user";
  ts: number | null;
  title: string;
  body: string;
  meta: string;
};

export type TailResponse = {
  thread_id: string | null;
  log_path: string | null;
  live_cursor: string | null;
  history_cursor: string | null;
  events: RawChatEvent[];
  has_older?: boolean;
  busy: boolean;
  queue_len: number;
  token?: Record<string, unknown> | null;
};

export type HistoryResponse = {
  thread_id: string | null;
  log_path: string | null;
  history_cursor: string | null;
  events: RawChatEvent[];
  has_older: boolean;
  busy: boolean;
  queue_len: number;
  token?: Record<string, unknown> | null;
};

export type LiveResponse = {
  thread_id: string | null;
  log_path: string | null;
  live_cursor: string | null;
  events: RawChatEvent[];
  meta_delta?: Record<string, number>;
  turn_start?: boolean;
  turn_end?: boolean;
  turn_aborted?: boolean;
  diag?: Record<string, unknown>;
  busy: boolean;
  queue_len: number;
  token?: Record<string, unknown> | null;
};

export type DiagnosticsResponse = {
  session_id: string;
  thread_id: string | null;
  agent_backend: string;
  owned: boolean;
  transport: string | null;
  cwd: string;
  start_ts: number;
  updated_ts: number;
  log_path: string | null;
  broker_pid: number;
  codex_pid: number;
  busy: boolean;
  broker_busy: boolean;
  queue_len: number;
  token?: Record<string, unknown> | null;
  model_provider: string | null;
  preferred_auth_method: string | null;
  provider_choice: string | null;
  model: string | null;
  reasoning_effort: string | null;
  service_tier: string | null;
  tmux_session: string | null;
  tmux_window: string | null;
  git_branch: string | null;
  time_priority: number;
  base_priority: number;
  final_priority: number;
  priority_offset: number;
  snooze_until: number | null;
  dependency_session_id: string | null;
};

export type VoiceSettingsResponse = {
  ok: true;
  tts_enabled_for_narration: boolean;
  tts_enabled_for_final_response: boolean;
  tts_base_url: string;
  tts_api_key: string;
  summarization_model: string;
  tts_model: string;
  audio: {
    queue_depth: number;
    active_listener_count: number;
    stream_url: string;
    segment_count: number;
    last_error: string;
    media_sequence: number;
  };
  notifications: {
    enabled_devices: number;
    total_devices: number;
    vapid_public_key: string;
  };
};

export type NotificationSubscriptionsResponse = {
  ok: true;
  vapid_public_key: string;
  subscriptions: Array<Record<string, unknown>>;
};

export type QueueItem = {
  id: string;
  text: string;
  sending: boolean;
};

export type QueueResponse = {
  ok: true;
  items?: QueueItem[];
  queue?: string[];
};

export type HarnessConfig = {
  enabled: boolean;
  request: string;
  cooldown_minutes: number;
  remaining_injections: number;
};

export type ChangedFileEntry = {
  path: string;
  additions: number | null;
  deletions: number | null;
  changed: boolean;
};

export type ChangedFilesResponse = {
  ok: true;
  cwd: string;
  files: string[];
  entries: ChangedFileEntry[];
  unstaged: string[];
  staged: string[];
};

export type FileEntry = {
  request_path: string;
  display_path: string;
  summary: string;
  source: "tracked" | "git" | "search";
};

export type FileSearchResponse = {
  ok: true;
  cwd: string;
  query: string;
  mode: string;
  matches: Array<{ path: string; score?: number }>;
  scanned: number;
  truncated: boolean;
};

export type TextFileView = {
  ok: true;
  kind: "text";
  path: string;
  rel: string;
  size: number;
  text: string;
  editable: boolean;
  version: string;
};

export type ImageFileView = {
  ok: true;
  kind: "image";
  path: string;
  rel: string;
  size: number;
  content_type: string;
  image_url: string;
};

export type PdfFileView = {
  ok: true;
  kind: "pdf";
  path: string;
  rel: string;
  size: number;
  content_type: string;
  pdf_url: string;
};

export type DownloadOnlyFileView = {
  ok: true;
  kind: "download_only";
  path: string;
  rel: string;
  size: number;
  reason?: string;
  viewer_max_bytes?: number;
};

export type FileReadResponse = TextFileView | ImageFileView | PdfFileView | DownloadOnlyFileView;
