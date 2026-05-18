export type SessionSummary = {
  session_id: string;
  thread_id: string | null;
  pid: number;
  broker_pid: number;
  agent_backend: string;
  owned: boolean;
  transport: string | null;
  cwd: string;
  workspace_cwd?: string | null;
  start_ts: number;
  updated_ts: number;
  log_path: string | null;
  queue_len: number;
  busy: boolean;
  token?: Record<string, unknown> | null;
  terminal_prompt?: TerminalPrompt | null;
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

export type TerminalPromptChoice = {
  label: string;
  value: string;
  description: string;
  key_seq: string;
};

export type TerminalPrompt = {
  kind: string;
  message: string;
  choices: TerminalPromptChoice[];
};

export type SessionsResponse = {
  app_version: string;
  sessions: SessionSummary[];
  recent_cwds: string[];
  new_session_defaults: NewSessionDefaults;
  tmux_available: boolean;
  tmux_session_name: string | null;
};

export type ScheduleTarget =
  | { mode: "existing_session"; session_id: string }
  | { mode: "new_session_each_run"; session_template: Record<string, unknown> }
  | { mode: "create_once_reuse"; session_template: Record<string, unknown>; created_session_id?: string | null };

export type ScheduleRule = {
  kind: "once" | "daily" | "weekly" | "monthly" | "interval";
  next_run_at?: number | null;
  interval_seconds?: number | null;
  timezone?: string | null;
  time?: string | null;
  weekdays?: number[];
  day_of_month?: number | null;
};

export type Schedule = {
  schedule_id: string;
  name: string;
  enabled: boolean;
  status: string;
  target: ScheduleTarget;
  rule: ScheduleRule;
  message_template: string;
  alias_template?: string | null;
  busy_policy: "enqueue";
  timeout_minutes: number | null;
  created_at: number;
  updated_at: number;
  next_run_at?: number | null;
  last_run_at?: number | null;
  run_count: number;
};

export type ScheduleRun = {
  run_id: string;
  schedule_id: string;
  status: string;
  trigger: string;
  scheduled_for: number;
  created_at: number;
  started_at?: number | null;
  finished_at?: number | null;
  target_session_id?: string | null;
  created_session_id?: string | null;
  queue_item_id?: string | null;
  rendered_message: string;
  rendered_alias?: string | null;
  error?: string | null;
  assistant_preview?: string | null;
};

export type SchedulesResponse = {
  ok: true;
  schedules: Schedule[];
  runs: ScheduleRun[];
};

export type MeResponse = {
  ok: true;
  server_pid: number;
};

export type VersionStatusResponse = {
  ok: true;
  update_available: boolean;
  local_head: string;
  local_branch: string;
  remote: string;
  remote_ref: string;
  remote_head: string;
  checked_at: number;
};

export type CwdSuggestion = {
  value: string;
  label: string;
  kind: "directory" | "recent";
};

export type CwdSuggestionsResponse = {
  ok: true;
  query: string;
  suggestions: CwdSuggestion[];
};

export type RestartServiceResponse = {
  ok: true;
  scheduled: true;
  restart_pid: number;
  script: string;
  server_pid: number;
};

export type EditSessionResponse = {
  ok?: true;
  alias: string;
  priority_offset: number;
  snooze_until: number | null;
  dependency_session_id: string | null;
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
  last_user_message?: string;
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
      options?: AskUserOptionInput[];
      questions?: AskUserQuestion[];
      header?: string;
      answer?: string | string[];
      allow_freeform?: boolean;
      allow_multiple?: boolean;
      resolved?: boolean;
      cancelled?: boolean;
      was_custom?: boolean;
      timeout_ms?: number;
      is_error?: boolean;
    }
  | {
      type: "extension";
      ts?: number;
      extension_kind?: string;
      source?: string;
      title?: string;
      status?: string;
      summary?: string;
      text?: string;
      tool_call_id?: string;
      progress_current?: number;
      progress_total?: number;
      progress_label?: string;
      goal_objective?: string;
      goal_token_budget?: number;
      goal_tokens_used?: number;
      goal_tokens_remaining?: number;
      goal_elapsed_seconds?: number;
      goal_completion_report?: string;
      items?: TranscriptExtensionItem[];
    };

export type TranscriptExtensionItem = {
  label?: string;
  status?: string;
  detail?: string;
};

export type AskUserOptionInput =
  | string
  | {
      label?: string;
      value?: string;
      title?: string;
      description?: string;
      preview?: string;
    };

export type AskUserQuestion = {
  header?: string;
  question?: string;
  options?: AskUserOptionInput[];
  allow_multiple?: boolean;
  allowMultiple?: boolean;
  multiSelect?: boolean;
};

export type AskUserOption = {
  label: string;
  value: string;
  description: string;
};

export type UiAskUserQuestion = {
  header: string;
  question: string;
  options: AskUserOption[];
  allowMultiple: boolean;
};

export type UiTranscriptEvent = {
  id: string;
  kind: "user" | "assistant" | "tool" | "tool_result" | "ask_user" | "extension";
  ts: number | null;
  title: string;
  body: string;
  meta: string;
  toolCallId?: string;
  toolCallBody?: string;
  toolResultBody?: string;
  toolResultIsError?: boolean;
  extensionKind?: string;
  source?: string;
  status?: string;
  summary?: string;
  progressCurrent?: number;
  progressTotal?: number;
  progressLabel?: string;
  goalObjective?: string;
  goalTokenBudget?: number;
  goalTokensUsed?: number;
  goalTokensRemaining?: number;
  goalElapsedSeconds?: number;
  goalCompletionReport?: string;
  items?: TranscriptExtensionItem[];
  askQuestion?: string;
  askContext?: string;
  askOptions?: AskUserOption[];
  askQuestions?: UiAskUserQuestion[];
  askAnswer?: string | string[];
  askAllowFreeform?: boolean;
  askAllowMultiple?: boolean;
  askResolved?: boolean;
  askCancelled?: boolean;
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
  terminal_prompt?: TerminalPrompt | null;
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
  terminal_prompt?: TerminalPrompt | null;
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
  terminal_prompt?: TerminalPrompt | null;
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
  terminal_prompt?: TerminalPrompt | null;
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

export type CodexConfigResponse = {
  ok: true;
  path: string;
  exists: boolean;
  text: string;
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

export type ShareSessionRef = {
  session_id: string;
  nickname: string;
  added_ts: number;
  cwd: string;
  workspace_cwd?: string | null;
  agent_backend: string;
  busy: boolean;
  queue_len: number;
  updated_ts: number;
};

export type ShareSet = {
  share_id: string;
  label: string;
  password_hash: string;
  password_hint: string;
  expires_at: number;
  created_at: number;
  updated_at: number;
  allow_interrupt: boolean;
  allow_files: boolean;
  allow_attachment_downloads: boolean;
  session_ids: string[];
  sessions: ShareSessionRef[];
};

export type ShareLoginResponse = {
  ok: true;
  share_id: string;
  share_label: string;
  expires_at: number;
  allow_interrupt: boolean;
  allow_files: boolean;
  allow_attachment_downloads: boolean;
  sessions: ShareSessionRef[];
};

export type ShareCreateResponse = {
  ok: true;
  share_id: string;
  share_url: string;
  share_password: string;
  share_label: string;
  expires_at: number;
  sessions: ShareSessionRef[];
};

export type ShareListResponse = {
  ok: true;
  shares: ShareSet[];
};

export type ShareMessageSession = {
  session_id: string;
  title: string;
  alias: string;
};

export type ShareMessageResponse = {
  ok: true;
  share_id: string;
  share_label: string;
  session: ShareMessageSession;
  transcript: RawChatEvent[];
  tail: TailResponse;
};

export type ShareFilesResponse = {
  ok: true;
  share_id: string;
  share_label: string;
  session_id: string;
  files: Array<Record<string, unknown>>;
};
