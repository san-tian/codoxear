import type { JSX } from "preact";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { api } from "./lib/api";
import {
  TranscriptEventRow,
  coalesceAdjacentAssistantEvents,
  eventTextPreview,
  isCollapsibleEvent,
  mergeTranscriptEvents,
  normalizeEvents,
  previewFromEvents,
} from "./lib/transcript";
import type {
  ChangedFilesResponse,
  CodexConfigResponse,
  CwdSuggestion,
  DiagnosticsResponse,
  FileEntry,
  FileReadResponse,
  HarnessConfig,
  NewSessionDefaults,
  NotificationSubscriptionsResponse,
  QueueResponse,
  QueueItem,
  ResumeCandidate,
  ShareCreateResponse,
  ShareFilesResponse,
  ShareLoginResponse,
  ShareSet,
  SessionSummary,
  UiTranscriptEvent,
  VersionStatusResponse,
  VoiceSettingsResponse,
} from "./lib/types";

const INIT_PAGE_LIMIT = 120;
const OLDER_PAGE_LIMIT = 60;
const POLL_IDLE_MS = 1400;
const POLL_BUSY_MS = 700;
const SESSION_REFRESH_MS = 12000;
const NOTIFICATION_POLL_MS = 5000;
const VERSION_STATUS_POLL_MS = 10 * 60 * 1000;
const ATTACH_UPLOAD_MAX_BYTES = 16 * 1024 * 1024;
const SELECTED_SESSION_KEY = "codoxear.nova.selected";
const SHOW_TOOL_CALLS_KEY = "codoxear.showToolCalls";
const DESKTOP_NOTIFICATIONS_KEY = "codoxear.desktopNotificationsEnabled";
const BUSY_SUBMIT_MODE_KEY = "codoxear.nova.busySubmitMode";
const THEME_MODE_KEY = "codoxear.nova.theme";
const COMPACT_SESSION_CARDS_KEY = "codoxear.nova.compactSessionCards";
const SESSION_DRAFTS_KEY = "codoxear.nova.sessionDrafts";
const NEW_SESSION_PREFERENCES_KEY = "codoxear.nova.newSessionPreferences";
const SIDEBAR_WORKSPACE_ORDER_KEY = "codoxear.nova.sidebar.workspaceOrder";
const SIDEBAR_SESSION_ORDER_KEY = "codoxear.nova.sidebar.sessionOrder";
const SIDEBAR_WIDTH_KEY = "codoxear.nova.sidebar.width";
const DETAIL_RAIL_WIDTH_KEY = "codoxear.nova.detailRail.width";
const CHAT_BOTTOM_FOLLOW_THRESHOLD_PX = 96;
const CHAT_HISTORY_TOP_THRESHOLD_PX = 16;
const CHAT_HISTORY_JUMP_OFFSET_PX = 24;
const STOPPING_FEEDBACK_MS = 2500;
const SIDEBAR_MIN_WIDTH_PX = 240;
const SIDEBAR_MAX_WIDTH_PX = 520;
const SIDEBAR_RESIZER_WIDTH_PX = 10;
const MAIN_MIN_WIDTH_PX = 320;
const DETAIL_RAIL_WIDTH_PX = 360;
const DETAIL_RAIL_MIN_WIDTH_PX = 320;
const DETAIL_RAIL_MAX_WIDTH_PX = 720;
const DETAIL_RAIL_RESIZER_WIDTH_PX = 10;
const DETAIL_RAIL_STACK_BREAKPOINT_PX = 1080;
const MOBILE_SIDEBAR_BREAKPOINT_PX = 860;
const IMPORTANT_PRIORITY_OFFSET = 0.85;
const PENDING_PRIORITY_OFFSET = -0.85;
const SHELVE_FOR_NOW_SECONDS = 24 * 60 * 60;
const SHARE_INIT_LIMIT = 120;
const SHARE_OLDER_LIMIT = 60;

type BusySubmitMode = "queue" | "interrupt";
type ThemeMode = "dark" | "light";
type AgentBackend = "codex" | "pi";
type TokenSummary = { label: string; title: string };
type ShareDraftSession = { session_id: string; label: string; workspace: string; cwd: string; checked: boolean };
type ManagedShareSet = ShareSet & { share_password?: string };
type NewSessionBackendPreferences = {
  provider?: string;
  model?: string;
  reasoning?: string;
  fast?: boolean;
};
type NewSessionPreferences = {
  backend?: AgentBackend;
  tmux?: boolean;
  backends?: Partial<Record<AgentBackend, NewSessionBackendPreferences>>;
};
type SessionMarkerState = "normal" | "star" | "snooze" | "pending";
const SESSION_MARKER_STATES: SessionMarkerState[] = ["normal", "star", "pending", "snooze"];
type SidebarSessionOrder = Record<string, string[]>;
type SidebarDragEvent = JSX.TargetedDragEvent<HTMLElement>;
type SidebarContextMenuEvent = JSX.TargetedMouseEvent<HTMLElement>;
type SidebarPointerEvent = JSX.TargetedPointerEvent<HTMLDivElement>;
type SidebarDropPosition = "before" | "after";
type SidebarDropIndicator =
  | { kind: "workspace"; key: string; position: SidebarDropPosition }
  | { kind: "session"; key: string; position: SidebarDropPosition };
type SessionViewSnapshot = {
  transcript: UiTranscriptEvent[];
  liveCursor: string | null;
  historyCursor: string | null;
  hasOlder: boolean;
  busy: boolean;
  queueLen: number;
  tokenSummary: TokenSummary | null;
};

type ShareTarget = {
  shareId: string;
  sessionId: string;
};

const EMPTY_VOICE_SETTINGS: VoiceSettingsResponse = {
  ok: true,
  tts_enabled_for_narration: false,
  tts_enabled_for_final_response: true,
  tts_base_url: "",
  tts_api_key: "",
  summarization_model: "",
  tts_model: "",
  audio: {
    queue_depth: 0,
    active_listener_count: 0,
    stream_url: "",
    segment_count: 0,
    last_error: "",
    media_sequence: 0,
  },
  notifications: {
    enabled_devices: 0,
    total_devices: 0,
    vapid_public_key: "",
  },
};

function readLocalStorage(key: string) {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeLocalStorage(key: string, value: string | null) {
  try {
    if (value == null) window.localStorage.removeItem(key);
    else window.localStorage.setItem(key, value);
  } catch {
    // ignore storage failures
  }
}

function readBusySubmitMode(): BusySubmitMode {
  return readLocalStorage(BUSY_SUBMIT_MODE_KEY) === "interrupt" ? "interrupt" : "queue";
}

function readThemeMode(): ThemeMode {
  return readLocalStorage(THEME_MODE_KEY) === "light" ? "light" : "dark";
}

function readCompactSessionCards() {
  return readLocalStorage(COMPACT_SESSION_CARDS_KEY) === "1";
}

function isMobileViewportWidth(width = window.innerWidth) {
  return width <= MOBILE_SIDEBAR_BREAKPOINT_PX;
}

function clampSidebarWidth(width: number, viewportWidth = window.innerWidth) {
  const maxWidth = Math.max(SIDEBAR_MIN_WIDTH_PX, Math.min(SIDEBAR_MAX_WIDTH_PX, viewportWidth - MAIN_MIN_WIDTH_PX));
  return Math.max(SIDEBAR_MIN_WIDTH_PX, Math.min(width, maxWidth));
}

function clampDetailRailWidth(width: number, viewportWidth = window.innerWidth) {
  if (viewportWidth <= DETAIL_RAIL_STACK_BREAKPOINT_PX) return Math.max(DETAIL_RAIL_MIN_WIDTH_PX, Math.min(width, DETAIL_RAIL_MAX_WIDTH_PX));
  const maxWidth = Math.max(
    DETAIL_RAIL_MIN_WIDTH_PX,
    Math.min(DETAIL_RAIL_MAX_WIDTH_PX, viewportWidth - MAIN_MIN_WIDTH_PX - DETAIL_RAIL_RESIZER_WIDTH_PX),
  );
  return Math.max(DETAIL_RAIL_MIN_WIDTH_PX, Math.min(width, maxWidth));
}

function readStoredSidebarWidth() {
  const raw = readLocalStorage(SIDEBAR_WIDTH_KEY);
  if (!raw) return null;
  const width = Number(raw);
  return Number.isFinite(width) ? clampSidebarWidth(width) : null;
}

function readStoredDetailRailWidth() {
  const raw = readLocalStorage(DETAIL_RAIL_WIDTH_KEY);
  if (!raw) return DETAIL_RAIL_WIDTH_PX;
  const width = Number(raw);
  return Number.isFinite(width) ? clampDetailRailWidth(width) : DETAIL_RAIL_WIDTH_PX;
}

function readStoredStringList(key: string) {
  try {
    const parsed = JSON.parse(readLocalStorage(key) || "[]");
    return Array.isArray(parsed) ? parsed.filter((value): value is string => typeof value === "string" && !!value) : [];
  } catch {
    return [];
  }
}

function readStoredSessionOrder(): SidebarSessionOrder {
  try {
    const parsed = JSON.parse(readLocalStorage(SIDEBAR_SESSION_ORDER_KEY) || "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const out: SidebarSessionOrder = {};
    Object.entries(parsed).forEach(([key, value]) => {
      if (Array.isArray(value)) out[key] = value.filter((item): item is string => typeof item === "string" && !!item);
    });
    return out;
  } catch {
    return {};
  }
}

function readStoredSessionDrafts() {
  try {
    const parsed = JSON.parse(readLocalStorage(SESSION_DRAFTS_KEY) || "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {} as Record<string, string>;
    const out: Record<string, string> = {};
    Object.entries(parsed).forEach(([key, value]) => {
      if (typeof value === "string" && value) out[key] = value;
    });
    return out;
  } catch {
    return {} as Record<string, string>;
  }
}

function isAgentBackend(value: unknown): value is AgentBackend {
  return value === "codex" || value === "pi";
}

function readStoredNewSessionPreferences(): NewSessionPreferences {
  try {
    const parsed = JSON.parse(readLocalStorage(NEW_SESSION_PREFERENCES_KEY) || "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const raw = parsed as Record<string, unknown>;
    const out: NewSessionPreferences = {};
    if (isAgentBackend(raw.backend)) out.backend = raw.backend;
    if (typeof raw.tmux === "boolean") out.tmux = raw.tmux;
    if (raw.backends && typeof raw.backends === "object" && !Array.isArray(raw.backends)) {
      const backends: Partial<Record<AgentBackend, NewSessionBackendPreferences>> = {};
      (["codex", "pi"] as const).forEach((backend) => {
        const value = (raw.backends as Record<string, unknown>)[backend];
        if (!value || typeof value !== "object" || Array.isArray(value)) return;
        const entry = value as Record<string, unknown>;
        const clean: NewSessionBackendPreferences = {};
        if (typeof entry.provider === "string") clean.provider = entry.provider;
        if (typeof entry.model === "string") clean.model = entry.model;
        if (typeof entry.reasoning === "string") clean.reasoning = entry.reasoning;
        if (typeof entry.fast === "boolean") clean.fast = entry.fast;
        backends[backend] = clean;
      });
      out.backends = backends;
    }
    return out;
  } catch {
    return {};
  }
}

function applyStoredOrder<T>(items: T[], order: string[], keyForItem: (item: T) => string) {
  if (!order.length) return items;
  const byKey = new Map(items.map((item) => [keyForItem(item), item]));
  const used = new Set<string>();
  const out: T[] = [];
  order.forEach((key) => {
    const item = byKey.get(key);
    if (!item || used.has(key)) return;
    used.add(key);
    out.push(item);
  });
  items.forEach((item) => {
    const key = keyForItem(item);
    if (!used.has(key)) out.push(item);
  });
  return out;
}

function moveOrderedKey(keys: string[], source: string, target: string, position: SidebarDropPosition = "before") {
  const next = keys.slice();
  const from = next.indexOf(source);
  const to = next.indexOf(target);
  if (from < 0 || to < 0 || from === to) return next;
  const [item] = next.splice(from, 1);
  const targetIndex = next.indexOf(target);
  next.splice(position === "after" ? targetIndex + 1 : targetIndex, 0, item);
  return next;
}

function prependOrderedKey(keys: string[], key: string) {
  return [key, ...keys.filter((item) => item !== key)];
}

function appendOrderedKey(keys: string[], key: string) {
  return [...keys.filter((item) => item !== key), key];
}

function insertOrderedKeyAfterAnchors(keys: string[], key: string, anchors: string[]) {
  const next = keys.filter((item) => item !== key);
  const anchorSet = new Set(anchors.filter(Boolean));
  if (!anchorSet.size) return prependOrderedKey(next, key);
  let insertAt = -1;
  next.forEach((item, index) => {
    if (anchorSet.has(item)) insertAt = index;
  });
  if (insertAt < 0) return prependOrderedKey(next, key);
  next.splice(insertAt + 1, 0, key);
  return next;
}

function dropPositionFromEvent(event: SidebarDragEvent): SidebarDropPosition {
  const rect = event.currentTarget.getBoundingClientRect();
  return event.clientY > rect.top + rect.height / 2 ? "after" : "before";
}

function copyTextViaSelection(text: string) {
  if (typeof document.execCommand !== "function") {
    throw new Error("Selection copy unavailable");
  }
  const value = String(text ?? "");
  const active = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "");
  textarea.setAttribute("aria-hidden", "true");
  textarea.style.position = "fixed";
  textarea.style.top = "0";
  textarea.style.left = "0";
  textarea.style.width = "1px";
  textarea.style.height = "1px";
  textarea.style.padding = "0";
  textarea.style.border = "0";
  textarea.style.opacity = "0";
  textarea.style.pointerEvents = "none";
  document.body.appendChild(textarea);
  textarea.focus({ preventScroll: true });
  textarea.select();
  textarea.setSelectionRange(0, value.length);
  const ok = document.execCommand("copy");
  textarea.remove();
  active?.focus({ preventScroll: true });
  if (!ok) throw new Error("Selection copy failed");
}

async function copyToClipboard(text: string) {
  if (window.isSecureContext && navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
    await navigator.clipboard.writeText(String(text ?? ""));
    return;
  }
  copyTextViaSelection(text);
}

function bytesToBase64(bytes: Uint8Array) {
  let binary = "";
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}

async function fileToBase64(file: File) {
  return bytesToBase64(new Uint8Array(await file.arrayBuffer()));
}

function base64UrlToUint8Array(value: string) {
  const padding = "=".repeat((4 - (value.length % 4)) % 4);
  const base64 = (value + padding).replace(/-/g, "+").replace(/_/g, "/");
  const raw = atob(base64);
  return Uint8Array.from(raw, (char) => char.charCodeAt(0));
}

function notificationDeviceClass() {
  const ua = navigator.userAgent || "";
  const touch = Number(navigator.maxTouchPoints || 0) > 1;
  return /Android|iPhone|iPad|iPod/i.test(ua) || touch ? "mobile" : "desktop";
}

async function ensureVoiceServiceWorker() {
  if (!("serviceWorker" in navigator)) throw new Error("service workers unsupported");
  return navigator.serviceWorker.register("/service-worker.js", { scope: "/" });
}

function sessionIdFromHash() {
  const raw = window.location.hash.startsWith("#") ? window.location.hash.slice(1) : "";
  const params = new URLSearchParams(raw);
  const sessionId = params.get("session");
  return sessionId && sessionId.trim() ? sessionId.trim() : "";
}

function shareTargetFromPath(pathname = window.location.pathname): ShareTarget | null {
  const parts = pathname.split("/").filter(Boolean);
  if (parts[0] !== "share" || parts.length < 2) return null;
  const shareId = parts[1]?.trim() || "";
  if (!shareId) return null;
  if (parts[2] === "sessions" && parts[3]?.trim()) {
    return { shareId, sessionId: parts[3].trim() };
  }
  return { shareId, sessionId: "" };
}

function writeSessionHash(sessionId: string) {
  const params = new URLSearchParams(window.location.hash.startsWith("#") ? window.location.hash.slice(1) : "");
  if (sessionId) params.set("session", sessionId);
  else params.delete("session");
  const next = params.toString();
  history.replaceState(null, "", `${window.location.pathname}${window.location.search}${next ? `#${next}` : ""}`);
}

function sortSessions(items: SessionSummary[]) {
  return items.slice().sort((left, right) => {
    const priorityDelta = Number(right.final_priority || 0) - Number(left.final_priority || 0);
    if (priorityDelta) return priorityDelta;
    const updatedDelta = Number(right.updated_ts || right.start_ts || 0) - Number(left.updated_ts || left.start_ts || 0);
    if (updatedDelta) return updatedDelta;
    return String(left.session_id || "").localeCompare(String(right.session_id || ""));
  });
}

function sessionIsImportant(session: SessionSummary | null) {
  return Boolean(session && Number(session.priority_offset || 0) >= 0.5);
}

function sessionIsPending(session: SessionSummary | null) {
  return Boolean(session && Number(session.priority_offset || 0) <= -0.5 && !sessionIsShelved(session));
}

function sessionIsShelved(session: SessionSummary | null) {
  if (!session) return false;
  if (session.snoozed) return true;
  const snoozeUntil = Number(session.snooze_until || 0);
  return Number.isFinite(snoozeUntil) && snoozeUntil > Date.now() / 1000;
}

function transcriptEventVisibleWithToolsHidden(event: UiTranscriptEvent) {
  if (event.kind === "user" || event.kind === "extension") return true;
  if (event.kind !== "assistant") return false;
  return event.meta.trim() !== "narration";
}

function partitionSidebarOrder<T>(items: T[], isImportant: (item: T) => boolean, isShelved: (item: T) => boolean) {
  const important: T[] = [];
  const regular: T[] = [];
  const shelved: T[] = [];
  items.forEach((item) => {
    if (isShelved(item)) shelved.push(item);
    else if (isImportant(item)) important.push(item);
    else regular.push(item);
  });
  return [...important, ...regular, ...shelved];
}

function uniqueStringsInOrder(values: string[]) {
  const seen = new Set<string>();
  const out: string[] = [];
  values.forEach((value) => {
    if (!value || seen.has(value)) return;
    seen.add(value);
    out.push(value);
  });
  return out;
}

function sameStringList(left: string[], right: string[]) {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function sleep(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function baseName(path: string) {
  const parts = String(path || "").split("/").filter(Boolean);
  return parts.length ? parts[parts.length - 1] : path;
}

function sessionDisplayName(session: SessionSummary | null) {
  if (!session) return "No session selected";
  const alias = String(session.alias || "").trim();
  if (alias) return alias;
  const cwdName = baseName(session.cwd);
  if (cwdName) return cwdName;
  return session.session_id;
}

function absoluteShareUrl(path: string) {
  return new URL(path, window.location.origin).toString();
}

function shareInvitationText(result: ShareCreateResponse) {
  return [`URL: ${absoluteShareUrl(result.share_url)}`, `Password: ${result.share_password}`].join("\n");
}

function shareSetUrl(share: ShareSet) {
  return `/share/${encodeURIComponent(share.share_id)}/`;
}

function shareSetCopyText(share: ManagedShareSet) {
  const lines = [`URL: ${absoluteShareUrl(shareSetUrl(share))}`];
  if (share.share_password) lines.push(`Password: ${share.share_password}`);
  else if (share.password_hint) lines.push(`Password hint: ${share.password_hint}`);
  return lines.join("\n");
}

function formatShareExpiry(expiresAt: number) {
  if (!(expiresAt > 0)) return "";
  const delta = expiresAt - Date.now() / 1000;
  if (delta <= 0) return "expired";
  if (delta < 3600) return `${Math.max(1, Math.ceil(delta / 60))}m left`;
  if (delta < 86400) return `${Math.max(1, Math.ceil(delta / 3600))}h left`;
  return `${Math.max(1, Math.ceil(delta / 86400))}d left`;
}

function sessionIsStarting(session: SessionSummary | null) {
  return Boolean(session && session.owned && !session.log_path);
}

function sessionIsQueuedWaiting(session: SessionSummary | null) {
  return Boolean(session && session.queue_len && !session.busy && !sessionIsStarting(session));
}

function sessionIsRunning(session: SessionSummary | null, awaitingReply = false, stopSuppressed = false) {
  return Boolean(session && ((session.busy && !stopSuppressed) || sessionIsStarting(session) || awaitingReply));
}

function sessionStatusText(session: SessionSummary, awaitingReply = false, stopSuppressed = false) {
  if (session.queue_len) return `queue ${session.queue_len}`;
  if (sessionIsStarting(session)) return "starting";
  if (sessionIsRunning(session, awaitingReply, stopSuppressed)) return "working";
  return "idle";
}

function sessionMarkerState(session: SessionSummary | null): SessionMarkerState {
  if (!session) return "normal";
  if (sessionIsShelved(session)) return "snooze";
  if (sessionIsImportant(session)) return "star";
  if (sessionIsPending(session)) return "pending";
  return "normal";
}

function nextSessionMarkerState(state: SessionMarkerState): SessionMarkerState {
  if (state === "normal") return "star";
  if (state === "star") return "pending";
  if (state === "pending") return "snooze";
  return "normal";
}

function sessionMarkerLabel(state: SessionMarkerState) {
  if (state === "star") return "Star";
  if (state === "snooze") return "Snooze";
  if (state === "pending") return "Pending";
  return "Normal";
}

function sessionMarkerIcon(state: SessionMarkerState) {
  if (state === "snooze") return "snooze";
  if (state === "pending") return "pending";
  return "star";
}

function workspaceKeyForSession(session: SessionSummary) {
  return String(session.workspace_cwd || session.cwd || "").trim() || "__unknown_workspace__";
}

function workspaceTitle(cwd: string) {
  return cwd === "__unknown_workspace__" ? "Unknown workspace" : baseName(cwd) || cwd;
}

function defaultsForBackend(defaults: NewSessionDefaults | null, backend: "codex" | "pi") {
  if (!defaults) return null;
  return defaults.backends[backend];
}

function preferredNewSessionBackend(defaults: NewSessionDefaults | null, preferences: NewSessionPreferences): AgentBackend {
  if (preferences.backend) return preferences.backend;
  return defaults?.default_backend === "pi" ? "pi" : "codex";
}

function newSessionValuesForBackend(
  defaults: NewSessionDefaults | null,
  backend: AgentBackend,
  preferences: NewSessionPreferences,
  tmuxAvailable: boolean,
) {
  const backendDefaults = defaultsForBackend(defaults, backend);
  const stored = preferences.backends?.[backend] || {};
  const providerChoices = backendDefaults?.provider_choices || [];
  const reasoningChoices = backendDefaults?.reasoning_efforts || [];
  const storedProvider = typeof stored.provider === "string" ? stored.provider : "";
  const storedReasoning = typeof stored.reasoning === "string" ? stored.reasoning : "";
  const defaultFast = String(backendDefaults?.service_tier || "").toLowerCase() === "fast";
  return {
    provider: providerChoices.includes(storedProvider) ? storedProvider : String(backendDefaults?.provider_choice || ""),
    model: typeof stored.model === "string" ? stored.model : String(backendDefaults?.model || ""),
    reasoning: reasoningChoices.includes(storedReasoning) ? storedReasoning : String(backendDefaults?.reasoning_effort || "high"),
    fast: Boolean(backendDefaults?.supports_fast && (typeof stored.fast === "boolean" ? stored.fast : defaultFast)),
    tmux: tmuxAvailable ? (typeof preferences.tmux === "boolean" ? preferences.tmux : tmuxAvailable) : false,
  };
}

function providerChoiceToSettings(choice: string, backend: "codex" | "pi") {
  const value = String(choice || "").trim();
  if (backend === "pi") return { model_provider: value || null, preferred_auth_method: null };
  if (value === "chatgpt") return { model_provider: "openai", preferred_auth_method: "chatgpt" };
  if (value === "openai-api") return { model_provider: "openai", preferred_auth_method: "apikey" };
  return { model_provider: value || null, preferred_auth_method: value ? "apikey" : null };
}

function makeWorktreeSlug(text: string) {
  return (
    String(text || "")
      .trim()
      .replace(/[^A-Za-z0-9._-]+/g, "-")
      .replace(/^[.-]+|[.-]+$/g, "") || "worktree"
  );
}

function relativeAge(ts: number) {
  if (!(ts > 0)) return "";
  const delta = Math.max(0, Math.floor(Date.now() / 1000 - ts));
  if (delta < 60) return "just now";
  if (delta < 3600) return `${Math.max(1, Math.floor(delta / 60))}m ago`;
  if (delta < 86400) return `${Math.max(1, Math.floor(delta / 3600))}h ago`;
  return `${Math.max(1, Math.floor(delta / 86400))}d ago`;
}

function shellSingleQuote(text: string) {
  return `'${text.replace(/'/g, `'\\''`)}'`;
}

  function tmuxAttachCommandForSession(session: SessionSummary | null) {
  if (!session || session.transport !== "tmux" || !session.tmux_session) return "";
  const attachCommand = `tmux attach-session -t ${shellSingleQuote(session.tmux_session)}`;
  if (!session.tmux_window) return attachCommand;
  return `${attachCommand} \\; select-window -t ${shellSingleQuote(`${session.tmux_session}:${session.tmux_window}`)}`;
}

function sessionById(items: SessionSummary[], sessionId: string) {
  return items.find((session) => session.session_id === sessionId) || null;
}

function isNearScrollBottom(element: HTMLElement) {
  return element.scrollHeight - element.scrollTop - element.clientHeight <= CHAT_BOTTOM_FOLLOW_THRESHOLD_PX;
}

function isNearHistoryTop(element: HTMLElement) {
  return element.scrollTop <= CHAT_HISTORY_TOP_THRESHOLD_PX;
}

function olderHistoryJumpTarget(events: UiTranscriptEvent[]) {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    if (events[index].kind === "user") return events[index].id;
  }
  return events[events.length - 1]?.id || "";
}

function todoStatusLabel(status: string | undefined) {
  const value = String(status || "").replace(/_/g, " ").trim();
  return value || "pending";
}

function todoStatusClass(status: string | undefined) {
  const value = String(status || "").replace(/_/g, "-").trim();
  return value ? ` is-${value}` : "";
}

function isFloatingProgressEvent(event: UiTranscriptEvent) {
  const title = event.title.trim().toLocaleLowerCase();
  const source = String(event.source || "").trim().toLocaleLowerCase();
  if (event.extensionKind !== "progress") return false;
  return title === "todo" || title === "ralph loop" || source === "codex" || source === "ralph-loop";
}

function isFloatingTodoProgressEvent(event: UiTranscriptEvent) {
  const title = event.title.trim().toLocaleLowerCase();
  const source = String(event.source || "").trim().toLocaleLowerCase();
  return title === "todo" || source === "codex";
}

function FloatingProgress(props: { event: UiTranscriptEvent; collapsed: boolean; onToggle: () => void }) {
  const { event, collapsed, onToggle } = props;
  const items = event.items || [];
  const total = event.progressTotal;
  const current = event.progressCurrent;
  const hasProgress = !isFloatingTodoProgressEvent(event) && typeof total === "number" && total > 0 && typeof current === "number";
  const percent = hasProgress ? Math.max(0, Math.min(100, (current / total) * 100)) : 0;
  const title = event.title.trim() || "Progress";
  return (
    <div className={`floatingProgress${collapsed ? " is-collapsed" : ""}`} role="status" aria-label={title}>
      <div className="floatingProgressTop">
        <div className="floatingProgressHead">
          <span className="floatingProgressTitle">{title}</span>
          {event.status ? <span className={`floatingProgressStatus${todoStatusClass(event.status)}`}>{todoStatusLabel(event.status)}</span> : null}
        </div>
        <button className="floatingProgressToggle" type="button" aria-expanded={!collapsed} title={collapsed ? "Expand progress" : "Collapse progress"} onClick={onToggle}>
          {icon(collapsed ? "down" : "up")}
          <span>{collapsed ? "Expand" : "Collapse"}</span>
        </button>
      </div>
      {!collapsed ? (
        <>
          {hasProgress ? (
            <div className="floatingProgressBar" aria-label={`${current} of ${total} ${event.progressLabel || "items"}`}>
              <span style={{ width: `${percent}%` }} />
            </div>
          ) : null}
          <ol className="floatingProgressItems">
            {items.slice(0, 4).map((item, index) => (
              <li className={`floatingProgressItem${todoStatusClass(item.status)}`} key={`${item.label || "item"}-${index}`}>
                <span className="floatingProgressMark" />
                <span className="floatingProgressItemLabel">{item.label || "Untitled item"}</span>
              </li>
            ))}
            {items.length > 4 ? <li className="floatingProgressMore">+{items.length - 4} more</li> : null}
          </ol>
        </>
      ) : null}
    </div>
  );
}

function WorkingIndicator(props: { label: string; tone?: "working" | "waiting" }) {
  const { label, tone = "working" } = props;
  return (
    <article className={`msg event-working workingRow${tone === "waiting" ? " is-waiting" : ""}`} role="status" aria-live="polite" aria-label={label}>
      <div className="message-side is-hidden" />
      <div className="message-main">
        <div className="workingIndicator">
          <span className="workingDots" aria-hidden="true">
            <span className="workingDot" />
            <span className="workingDot" />
            <span className="workingDot" />
          </span>
          <span className="workingLabel">{label}</span>
        </div>
      </div>
    </article>
  );
}

function icon(name: string) {
  const common = {
    width: 16,
    height: 16,
    viewBox: "0 0 16 16",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.6,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };
  switch (name) {
    case "plus":
      return (
        <svg {...common}>
          <path d="M8 3v10" />
          <path d="M3 8h10" />
        </svg>
      );
    case "bell":
      return (
        <svg {...common}>
          <path d="M8 13.5c.8 0 1.5-.7 1.5-1.5h-3c0 .8.7 1.5 1.5 1.5Z" />
          <path d="M4.5 11h7l-1-1.7V7a2.5 2.5 0 1 0-5 0v2.3L4.5 11Z" />
        </svg>
      );
    case "volume":
      return (
        <svg {...common}>
          <path d="M3.5 6.5H6L9.5 4v8L6 9.5H3.5Z" />
          <path d="M11.5 6a3 3 0 0 1 0 4" />
        </svg>
      );
    case "settings":
      return (
        <svg {...common}>
          <circle cx="8" cy="8" r="2.2" />
          <path d="M8 2.5v1.3" />
          <path d="M8 12.2v1.3" />
          <path d="M12.2 8h1.3" />
          <path d="M2.5 8h1.3" />
          <path d="m11.2 4.8.9-.9" />
          <path d="m3.9 12.1.9-.9" />
          <path d="m11.2 11.2.9.9" />
          <path d="m3.9 3.9.9.9" />
        </svg>
      );
    case "logout":
      return (
        <svg {...common}>
          <path d="M6.5 3.2H4.8A1.8 1.8 0 0 0 3 5v6a1.8 1.8 0 0 0 1.8 1.8h1.7" />
          <path d="M9 11.5 12.5 8 9 4.5" />
          <path d="M12.2 8H6.5" />
        </svg>
      );
    case "menu":
      return (
        <svg {...common}>
          <path d="M3 4.5h10" />
          <path d="M3 8h10" />
          <path d="M3 11.5h10" />
        </svg>
      );
    case "close":
      return (
        <svg {...common}>
          <path d="M4 4 12 12" />
          <path d="M12 4 4 12" />
        </svg>
      );
    case "terminal":
      return (
        <svg {...common}>
          <path d="M3.5 4.5 6.5 7.5 3.5 10.5" />
          <path d="M8 11h4.5" />
        </svg>
      );
    case "wrench":
      return (
        <svg {...common}>
          <path d="M10.7 2.8a3.1 3.1 0 0 0 2.5 3.7L6.1 13.6a1.5 1.5 0 0 1-2.1-2.1l7.1-7.1A3.1 3.1 0 0 0 10.7 2.8Z" />
          <path d="M4.4 11.6 5.9 13.1" />
        </svg>
      );
    case "tmux":
      return (
        <svg {...common}>
          <rect x="2.8" y="3" width="10.4" height="10" rx="1.5" />
          <path d="M8 3v10" />
          <path d="M8 8h5.2" />
        </svg>
      );
    case "share":
      return (
        <svg {...common}>
          <circle cx="5" cy="8" r="1.7" />
          <circle cx="11.2" cy="4.4" r="1.7" />
          <circle cx="11.2" cy="11.6" r="1.7" />
          <path d="m6.5 7.1 3.2-1.8" />
          <path d="m6.5 8.9 3.2 1.8" />
        </svg>
      );
    case "edit":
      return (
        <svg {...common}>
          <path d="M9.8 3.2 12.8 6.2" />
          <path d="M11.6 2.4a1.2 1.2 0 0 1 1.7 1.7L6 11.4 3.2 12.8 4.6 10Z" />
        </svg>
      );
    case "star":
      return (
        <svg {...common}>
          <path d="m8 2.6 1.5 3 3.3.5-2.4 2.3.6 3.3L8 10.1 5 11.7l.6-3.3-2.4-2.3 3.3-.5Z" />
        </svg>
      );
    case "snooze":
      return (
        <svg {...common}>
          <circle cx="8" cy="8" r="5" />
          <path d="M8 5.2V8l2 1.2" />
          <path d="M4.5 2.8 3.2 4.1" />
          <path d="M11.5 2.8 12.8 4.1" />
        </svg>
      );
    case "pending":
      return (
        <svg {...common}>
          <circle cx="8" cy="8" r="4.8" />
          <path d="M5.5 8h5" />
        </svg>
      );
    case "file":
      return (
        <svg {...common}>
          <path d="M5 2.8h4l2 2V13H5Z" />
          <path d="M9 2.8V5h2" />
        </svg>
      );
    case "info":
      return (
        <svg {...common}>
          <circle cx="8" cy="8" r="5.5" />
          <path d="M8 7v3" />
          <path d="M8 5.2h.01" />
        </svg>
      );
    case "stop":
      return (
        <svg {...common}>
          <rect x="4.2" y="4.2" width="7.6" height="7.6" rx="1.3" />
        </svg>
      );
    case "harness":
      return (
        <svg {...common}>
          <path d="M3.5 8h9" />
          <path d="M8 3.5v9" />
          <circle cx="8" cy="8" r="4.8" />
        </svg>
      );
    case "paperclip":
      return (
        <svg {...common}>
          <path d="M5.8 8.8 9.7 5a2 2 0 1 1 2.8 2.8L7.8 12.5A3 3 0 1 1 3.5 8.2l4.3-4.3" />
        </svg>
      );
    case "queue":
      return (
        <svg {...common}>
          <path d="M4 4.5h8" />
          <path d="M4 8h8" />
          <path d="M4 11.5h5" />
        </svg>
      );
    case "send":
      return (
        <svg {...common}>
          <path d="m2.8 8 10-4-2.2 8-2.5-2L5.5 12 6.6 8.6Z" />
        </svg>
      );
    case "up":
      return (
        <svg {...common}>
          <path d="m4.5 9.5 3.5-3.5 3.5 3.5" />
        </svg>
      );
    case "down":
      return (
        <svg {...common}>
          <path d="m4.5 6.5 3.5 3.5 3.5-3.5" />
        </svg>
      );
    case "trash":
      return (
        <svg {...common}>
          <path d="M3.8 4.8h8.4" />
          <path d="M6.3 4.8v-1h3.4v1" />
          <path d="m5.2 4.8.5 7h4.6l.5-7" />
        </svg>
      );
    default:
      return (
        <svg {...common}>
          <circle cx="8" cy="8" r="5" />
        </svg>
      );
  }
}

function toRelativePath(session: SessionSummary, rawPath: string) {
  const cwd = String(session.cwd || "").replace(/\/+$/, "");
  if (!rawPath.startsWith("/")) return rawPath;
  if (!cwd || !rawPath.startsWith(`${cwd}/`)) return rawPath;
  return rawPath.slice(cwd.length + 1);
}

function buildFileEntries(session: SessionSummary | null, changedFiles: ChangedFilesResponse | null) {
  const out = new Map<string, FileEntry>();
  if (session) {
    for (const rawPath of Array.isArray(session.files) ? session.files : []) {
      if (typeof rawPath !== "string" || !rawPath.trim()) continue;
      const requestPath = toRelativePath(session, rawPath);
      const displayPath = requestPath;
      out.set(displayPath, {
        request_path: requestPath,
        display_path: displayPath,
        summary: "Tracked in session file history",
        source: "tracked",
      });
    }
  }
  if (changedFiles) {
    for (const entry of Array.isArray(changedFiles.entries) ? changedFiles.entries : []) {
      if (!entry || typeof entry.path !== "string" || !entry.path.trim()) continue;
      const summaryBits: string[] = [];
      if (typeof entry.additions === "number") summaryBits.push(`+${entry.additions}`);
      if (typeof entry.deletions === "number") summaryBits.push(`-${entry.deletions}`);
      summaryBits.push("Changed in git working tree");
      out.set(entry.path, {
        request_path: entry.path,
        display_path: entry.path,
        summary: summaryBits.join(" · "),
        source: "git",
      });
    }
  }
  return Array.from(out.values()).sort((left, right) => left.display_path.localeCompare(right.display_path));
}

function normalizeQueueItems(response: QueueResponse) {
  if (Array.isArray(response.items)) {
    return response.items
      .filter((item: QueueItem) => item && typeof item.id === "string" && typeof item.text === "string")
      .map((item: QueueItem) => ({ id: item.id, text: item.text, sending: !!item.sending }));
  }
  if (Array.isArray(response.queue)) {
    return response.queue
      .filter((text: string) => typeof text === "string" && text.trim())
      .map((text: string, index: number) => ({ id: `legacy-${index}`, text, sending: false }));
  }
  return [];
}

function formatTokenCount(value: number) {
  return Math.round(value).toLocaleString();
}

function renderTokenSummary(token: Record<string, unknown> | null | undefined): TokenSummary | null {
  if (!token || typeof token !== "object") return null;
  const contextWindow = Number(token.context_window);
  const tokensInContext = Number(token.tokens_in_context);
  const percentRemaining = Number(token.percent_remaining);
  const baselineTokens = Number(token.baseline_tokens);
  const asOf = typeof token.as_of === "string" && token.as_of.trim() ? token.as_of.trim() : null;
  if (Number.isFinite(contextWindow) && Number.isFinite(tokensInContext) && contextWindow > 0) {
    const percentNote = Number.isFinite(percentRemaining)
      ? ` ${Math.round(percentRemaining)}% remaining excludes a reserved ${Number.isFinite(baselineTokens) ? formatTokenCount(baselineTokens) : "baseline"}-token buffer.`
      : "";
    const asOfNote = asOf ? ` Last update: ${asOf}.` : "";
    return {
      label: `Ctx est ${formatTokenCount(tokensInContext)}/${formatTokenCount(contextWindow)}`,
      title: `Usage comes from the latest token update. Context window size may be inferred from model metadata for some backends.${percentNote}${asOfNote}`,
    };
  }
  return null;
}

function LoadingScreen({ text }: { text: string }) {
  return (
    <div className="loginWrap">
      <div className="login">
        <div className="title">Codoxear Nova</div>
        <div className="muted">{text}</div>
      </div>
    </div>
  );
}

function LoginScreen(props: {
  password: string;
  errorText: string;
  onPasswordChange: (value: string) => void;
  onSubmit: () => void | Promise<void>;
}) {
  return (
    <div className="loginWrap">
      <form
        className="login"
        onSubmit={(event) => {
          event.preventDefault();
          void props.onSubmit();
        }}
      >
        <div className="title">Codoxear Nova</div>
        <div className="muted">Use the existing Codoxear password for this host.</div>
        <input
          type="password"
          value={props.password}
          onInput={(event) => props.onPasswordChange((event.currentTarget as HTMLInputElement).value)}
          placeholder="Password"
          autoComplete="current-password"
        />
        <button className="primary" type="submit">
          Enter
        </button>
      </form>
      {props.errorText ? <div className="error-toast">{props.errorText}</div> : null}
    </div>
  );
}

function ShareLoginScreen(props: {
  label: string;
  password: string;
  errorText: string;
  onPasswordChange: (value: string) => void;
  onSubmit: () => void | Promise<void>;
}) {
  return (
    <div className="loginWrap">
      <form
        className="login"
        onSubmit={(event) => {
          event.preventDefault();
          void props.onSubmit();
        }}
      >
        <div className="title">{props.label || "Shared sessions"}</div>
        <div className="muted">Enter the temporary share password.</div>
        <input
          type="password"
          value={props.password}
          onInput={(event) => props.onPasswordChange((event.currentTarget as HTMLInputElement).value)}
          placeholder="Share password"
          autoComplete="current-password"
        />
        <button className="primary" type="submit">
          Open share
        </button>
      </form>
      {props.errorText ? <div className="error-toast">{props.errorText}</div> : null}
    </div>
  );
}

function ShareWorkspace(props: {
  share: ShareSet;
  sessionId: string;
  transcript: UiTranscriptEvent[];
  files: ShareFilesResponse | null;
  selectedFilePath: string;
  selectedFile: FileReadResponse | null;
  loading: boolean;
  loadingOlder: boolean;
  hasOlder: boolean;
  busy: boolean;
  queueLen: number;
  errorText: string;
  sendText: string;
  canSend: boolean;
  onSessionChange: (sessionId: string) => void;
  onSendTextChange: (value: string) => void;
  onSend: () => void | Promise<void>;
  onInterrupt: () => void | Promise<void>;
  onLoadOlder: () => void | Promise<void>;
  onSelectFile: (path: string) => void;
  onCopyText: (event: UiTranscriptEvent) => void | Promise<void>;
}) {
  const session = props.share.sessions.find((item) => item.session_id === props.sessionId) || props.share.sessions[0] || null;
  const shareFiles = props.share.allow_files ? props.files?.files || [] : [];
  const showFileRail = props.share.allow_files && (shareFiles.length > 0 || Boolean(props.selectedFile));
  return (
    <div className="app shareApp">
      <aside className="sidebar shareSidebar">
        <header>
          <div className="title">
            <span className="sidebarLogoDot" />
            Shared sessions
          </div>
        </header>
        <div className="sessions shareSessions">
          {props.share.sessions.map((item) => (
            <button
              key={item.session_id}
              className={`workspaceSelect shareSession${item.session_id === props.sessionId ? " active" : ""}`}
              type="button"
              onClick={() => props.onSessionChange(item.session_id)}
            >
              <div className="workspaceHeader">
                <div className="workspaceTitleRow">
                  <div className="workspaceTitle">{item.nickname || item.session_id}</div>
                </div>
                <div className="workspaceMeta">{item.session_id}</div>
              </div>
            </button>
          ))}
        </div>
      </aside>
      <div className="main shareMain">
        <div className="topbar">
          <div className="pill">
            <div className="titleWrap">
              <div className="titleRow">
                <div>{session?.nickname || session?.session_id || props.share.label}</div>
                <div className="topMeta">
                  <span className="status-chip">{props.share.label}</span>
                </div>
              </div>
            </div>
          </div>
          <div className="actions topActions">
            <button className="icon-btn" type="button" title="Interrupt" disabled={!props.share.allow_interrupt || !props.sessionId || props.loading} onClick={() => void props.onInterrupt()}>
              {icon("stop")}
            </button>
          </div>
        </div>
        <div className="workspaceBody">
          <div className="workspaceMain">
            <div className="chatWrap">
              <div className="chat">
                <div className="chatInner">
                  {props.hasOlder ? (
                    <button className="olderBtn" type="button" disabled={props.loadingOlder} onClick={() => void props.onLoadOlder()}>
                      {props.loadingOlder ? "Loading older messages…" : "Load older messages"}
                    </button>
                  ) : null}
                  {props.transcript.map((event, index) => (
                    <TranscriptEventRow
                      key={event.id}
                      event={event}
                      events={props.transcript}
                      index={index}
                      collapsed={false}
                      onToggle={() => {}}
                      onCopyText={props.onCopyText}
                      attachmentHref={(path) =>
                        `/share/${encodeURIComponent(props.share.share_id)}/sessions/${encodeURIComponent(props.sessionId)}/file/download?path=${encodeURIComponent(path)}`
                      }
                    />
                  ))}
                  {!props.transcript.length ? <div className="emptyState">No transcript yet for this shared session.</div> : null}
                </div>
              </div>
            </div>
          </div>
          {showFileRail ? (
          <aside className="detailRail shareDetailRail">
            <section className="detailSection fileSection">
              <div className="detailSectionHeader">Files</div>
              <div className="fileList">
                {shareFiles.map((entry) => {
                  const path = String(entry.path || entry.rel || "");
                  if (!path) return null;
                  return (
                    <button key={path} className={`fileEntry${props.selectedFilePath === path ? " active" : ""}`} type="button" onClick={() => props.onSelectFile(path)}>
                      <div className="fileEntryPath">{path}</div>
                      <div className="fileEntryMeta">{String(entry.kind || "file")}</div>
                    </button>
                  );
                })}
              </div>
              {props.selectedFile ? (
                <div className="filePreview">
                  <div className="filePreviewHeader">{props.selectedFile.rel}</div>
                  {"text" === props.selectedFile.kind ? <pre className="filePreviewText">{props.selectedFile.text}</pre> : null}
                  {"image" === props.selectedFile.kind ? (
                    <div className="filePreviewMedia">
                      <img src={props.selectedFile.image_url} alt={props.selectedFile.rel} />
                    </div>
                  ) : null}
                  {"pdf" === props.selectedFile.kind ? (
                    <a className="filePreviewLink" href={props.selectedFile.pdf_url} target="_blank" rel="noreferrer">
                      Open PDF
                    </a>
                  ) : null}
                  {"download_only" === props.selectedFile.kind ? <div className="muted">{props.selectedFile.reason || "Binary file"}</div> : null}
                </div>
              ) : null}
            </section>
          </aside>
          ) : null}
        </div>
        <div className="composer">
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void props.onSend();
            }}
          >
            <div className="inputWrap">
              <textarea value={props.sendText} onInput={(event) => props.onSendTextChange((event.currentTarget as HTMLTextAreaElement).value)} aria-label="Reply in shared session" />
              {!props.sendText ? <div className="ph">Reply in shared session</div> : null}
            </div>
            <button className="icon-btn primary" type="submit" title={props.loading ? "Sending…" : "Send"} disabled={!props.canSend || props.loading}>
              {icon("send")}
            </button>
          </form>
        </div>
        {props.errorText ? <div className="toast muted">{props.errorText}</div> : null}
      </div>
    </div>
  );
}

export function App() {
  const shareTarget = shareTargetFromPath();
  const shareMode = Boolean(shareTarget);
  const [authState, setAuthState] = useState<"loading" | "login" | "ready">("loading");
  const [loadingText, setLoadingText] = useState("Loading workspace…");
  const [versionStatus, setVersionStatus] = useState<VersionStatusResponse | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [collapsedWorkspaces, setCollapsedWorkspaces] = useState<Record<string, boolean>>({});
  const [selectedSessionId, setSelectedSessionId] = useState("");
  const [transcript, setTranscript] = useState<UiTranscriptEvent[]>([]);
  const [collapsedEvents, setCollapsedEvents] = useState<Record<string, boolean>>({});
  const [collapsedFloatingProgressEvents, setCollapsedFloatingProgressEvents] = useState<Record<string, boolean>>({});
  const [busy, setBusy] = useState(false);
  const [sending, setSending] = useState(false);
  const [closingSession, setClosingSession] = useState(false);
  const [interruptingSessionId, setInterruptingSessionId] = useState("");
  const [interruptingFeedbackUntil, setInterruptingFeedbackUntil] = useState(0);
  const [sessionMarkerBusyId, setSessionMarkerBusyId] = useState("");
  const [queueLen, setQueueLen] = useState(0);
  const [tokenSummary, setTokenSummary] = useState<TokenSummary | null>(null);
  const [liveCursor, setLiveCursor] = useState<string | null>(null);
  const [historyCursor, setHistoryCursor] = useState<string | null>(null);
  const [hasOlder, setHasOlder] = useState(false);
  const [historyTopBoundaryReached, setHistoryTopBoundaryReached] = useState(true);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [errorText, setErrorText] = useState("");
  const [sessionDrafts, setSessionDrafts] = useState<Record<string, string>>(() => readStoredSessionDrafts());
  const [busySubmitMode, setBusySubmitMode] = useState<BusySubmitMode>(() => readBusySubmitMode());
  const [toastText, setToastText] = useState("");
  const [loginPassword, setLoginPassword] = useState("");
  const [shareAuthState, setShareAuthState] = useState<"loading" | "login" | "ready">(shareMode ? "loading" : "ready");
  const [shareLabel, setShareLabel] = useState("Shared sessions");
  const [sharePassword, setSharePassword] = useState("");
  const [sharePasswordError, setSharePasswordError] = useState("");
  const [shareSet, setShareSet] = useState<ShareSet | null>(null);
  const [shareSessionId, setShareSessionId] = useState(shareTarget?.sessionId || "");
  const [shareTranscript, setShareTranscript] = useState<UiTranscriptEvent[]>([]);
  const [shareHistoryCursor, setShareHistoryCursor] = useState<string | null>(null);
  const [shareLiveCursor, setShareLiveCursor] = useState<string | null>(null);
  const [shareHasOlder, setShareHasOlder] = useState(false);
  const [shareLoadingOlder, setShareLoadingOlder] = useState(false);
  const [shareBusy, setShareBusy] = useState(false);
  const [shareQueueLen, setShareQueueLen] = useState(0);
  const [shareErrorText, setShareErrorText] = useState("");
  const [shareLoading, setShareLoading] = useState(false);
  const [shareSendText, setShareSendText] = useState("");
  const [shareFiles, setShareFiles] = useState<ShareFilesResponse | null>(null);
  const [shareSelectedFilePath, setShareSelectedFilePath] = useState("");
  const [shareSelectedFile, setShareSelectedFile] = useState<FileReadResponse | null>(null);
  const [showTools, setShowTools] = useState(() => readLocalStorage(SHOW_TOOL_CALLS_KEY) !== "0");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [mobileViewport, setMobileViewport] = useState(() => isMobileViewportWidth());
  const [mobileSidebarOpen, setMobileSidebarOpen] = useState(false);
  const [sidebarWidth, setSidebarWidth] = useState<number | null>(() => readStoredSidebarWidth());
  const [sidebarResizing, setSidebarResizing] = useState(false);
  const [detailRailWidth, setDetailRailWidth] = useState(() => readStoredDetailRailWidth());
  const [detailRailResizing, setDetailRailResizing] = useState(false);
  const [workspaceOrder, setWorkspaceOrder] = useState<string[]>(() => readStoredStringList(SIDEBAR_WORKSPACE_ORDER_KEY));
  const [sessionOrderByWorkspace, setSessionOrderByWorkspace] = useState<SidebarSessionOrder>(() => readStoredSessionOrder());
  const [draggingWorkspaceKey, setDraggingWorkspaceKey] = useState("");
  const [draggingSession, setDraggingSession] = useState<{ workspaceKey: string; sessionId: string } | null>(null);
  const [sidebarDropIndicator, setSidebarDropIndicator] = useState<SidebarDropIndicator | null>(null);
  const [showFilesPanel, setShowFilesPanel] = useState(false);
  const [showDetailsPanel, setShowDetailsPanel] = useState(false);
  const [newSessionOpen, setNewSessionOpen] = useState(false);
  const [newSessionBusy, setNewSessionBusy] = useState(false);
  const [newSessionDefaults, setNewSessionDefaults] = useState<NewSessionDefaults | null>(null);
  const [newSessionPreferences, setNewSessionPreferences] = useState<NewSessionPreferences>(() => readStoredNewSessionPreferences());
  const [recentCwds, setRecentCwds] = useState<string[]>([]);
  const [tmuxAvailable, setTmuxAvailable] = useState(false);
  const [newSessionBackend, setNewSessionBackend] = useState<AgentBackend>("codex");
  const [newSessionCwd, setNewSessionCwd] = useState("");
  const [newSessionCwdSuggestions, setNewSessionCwdSuggestions] = useState<CwdSuggestion[]>([]);
  const [newSessionCwdSuggestionsOpen, setNewSessionCwdSuggestionsOpen] = useState(false);
  const [newSessionCwdSuggestionIndex, setNewSessionCwdSuggestionIndex] = useState(-1);
  const [newSessionCwdSuggestionError, setNewSessionCwdSuggestionError] = useState("");
  const [newSessionCwdSuggestionsLoading, setNewSessionCwdSuggestionsLoading] = useState(false);
  const [newSessionProvider, setNewSessionProvider] = useState("");
  const [newSessionModel, setNewSessionModel] = useState("");
  const [newSessionReasoning, setNewSessionReasoning] = useState("high");
  const [newSessionFast, setNewSessionFast] = useState(false);
  const [newSessionTmux, setNewSessionTmux] = useState(false);
  const [newSessionResumeSelection, setNewSessionResumeSelection] = useState<ResumeCandidate | null>(null);
  const [newSessionResumeCandidates, setNewSessionResumeCandidates] = useState<ResumeCandidate[]>([]);
  const [newSessionWorktree, setNewSessionWorktree] = useState(false);
  const [newSessionWorktreeBranch, setNewSessionWorktreeBranch] = useState("");
  const [newSessionError, setNewSessionError] = useState("");
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameSessionId, setRenameSessionId] = useState("");
  const [renameName, setRenameName] = useState("");
  const [renameBusy, setRenameBusy] = useState(false);
  const [renameError, setRenameError] = useState("");
  const [shareCreateOpen, setShareCreateOpen] = useState(false);
  const [shareCreateLabel, setShareCreateLabel] = useState("");
  const [shareCreateExpiresHours, setShareCreateExpiresHours] = useState(24);
  const [shareCreateSessions, setShareCreateSessions] = useState<ShareDraftSession[]>([]);
  const [shareCreateBusy, setShareCreateBusy] = useState(false);
  const [shareCreateError, setShareCreateError] = useState("");
  const [shareCreateResult, setShareCreateResult] = useState<ShareCreateResponse | null>(null);
  const [managedShares, setManagedShares] = useState<ManagedShareSet[]>([]);
  const [managedSharesLoading, setManagedSharesLoading] = useState(false);
  const [deletingShareId, setDeletingShareId] = useState("");
  const [sessionContextMenu, setSessionContextMenu] = useState<{ sessionId: string; x: number; y: number } | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [themeMode, setThemeMode] = useState<ThemeMode>(() => readThemeMode());
  const [compactSessionCards, setCompactSessionCards] = useState(() => readCompactSessionCards());
  const [voiceSettings, setVoiceSettings] = useState<VoiceSettingsResponse | null>(null);
  const [codexConfig, setCodexConfig] = useState<CodexConfigResponse | null>(null);
  const [voiceSettingsLoading, setVoiceSettingsLoading] = useState(false);
  const [voiceSettingsSaving, setVoiceSettingsSaving] = useState(false);
  const [codexConfigSaving, setCodexConfigSaving] = useState(false);
  const [serviceRestarting, setServiceRestarting] = useState(false);
  const [settingsLoadError, setSettingsLoadError] = useState("");
  const [voiceBaseUrl, setVoiceBaseUrl] = useState("");
  const [voiceApiKey, setVoiceApiKey] = useState("");
  const [voiceNarrationEnabled, setVoiceNarrationEnabled] = useState(false);
  const [codexConfigText, setCodexConfigText] = useState("");
  const [notificationSnapshot, setNotificationSnapshot] = useState<NotificationSubscriptionsResponse | null>(null);
  const [desktopNotificationsEnabled, setDesktopNotificationsEnabled] = useState(() => readLocalStorage(DESKTOP_NOTIFICATIONS_KEY) === "1");
  const [mobilePushEnabled, setMobilePushEnabled] = useState(false);
  const [mobilePushEndpoint, setMobilePushEndpoint] = useState("");
  const [attachedFiles, setAttachedFiles] = useState(0);
  const [attachBusy, setAttachBusy] = useState(false);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsResponse | null>(null);
  const [detailsLoading, setDetailsLoading] = useState(false);
  const [fileEntries, setFileEntries] = useState<FileEntry[]>([]);
  const [fileSearchQuery, setFileSearchQuery] = useState("");
  const [fileSearchEntries, setFileSearchEntries] = useState<FileEntry[]>([]);
  const [fileSearchLoading, setFileSearchLoading] = useState(false);
  const [filesLoading, setFilesLoading] = useState(false);
  const [fileOpenLoading, setFileOpenLoading] = useState(false);
  const [fileOpenError, setFileOpenError] = useState("");
  const [activeFilePath, setActiveFilePath] = useState("");
  const [activeFile, setActiveFile] = useState<FileReadResponse | null>(null);
  const [queueOpen, setQueueOpen] = useState(false);
  const [queueItems, setQueueItems] = useState<QueueItem[]>([]);
  const [queueDrafts, setQueueDrafts] = useState<Record<string, string>>({});
  const [queueLoading, setQueueLoading] = useState(false);
  const [harnessOpen, setHarnessOpen] = useState(false);
  const [harnessLoading, setHarnessLoading] = useState(false);
  const [harnessSaving, setHarnessSaving] = useState(false);
  const [harnessDraft, setHarnessDraft] = useState<HarnessConfig | null>(null);
  const [sessionLastLines, setSessionLastLines] = useState<Record<string, string>>({});

  const pollTimerRef = useRef<number | null>(null);
  const sessionRefreshTimerRef = useRef<number | null>(null);
  const notificationPollTimerRef = useRef<number | null>(null);
  const liveCursorRef = useRef<string | null>(null);
  const historyCursorRef = useRef<string | null>(null);
  const selectedSessionRef = useRef("");
  const sessionViewCacheRef = useRef<Record<string, SessionViewSnapshot>>({});
  const fastPollUntilRef = useRef(0);
  const openRequestRef = useRef(0);
  const notificationFeedSinceRef = useRef(0);
  const shownNotificationIdsRef = useRef<Set<string>>(new Set());
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const composerInputRef = useRef<HTMLTextAreaElement | null>(null);
  const appRef = useRef<HTMLDivElement | null>(null);
  const sidebarRef = useRef<HTMLElement | null>(null);
  const newSessionCwdInputRef = useRef<HTMLInputElement | null>(null);
  const activeFilePathRef = useRef("");
  const filesLoadedSessionRef = useRef("");
  const chatScrollRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef(true);
  const lastAutoScrollKeyRef = useRef("");
  const newSessionCwdRequestRef = useRef(0);
  const sidebarResizeStartXRef = useRef(0);
  const sidebarResizeStartWidthRef = useRef(0);
  const detailRailResizeStartXRef = useRef(0);
  const detailRailResizeStartWidthRef = useRef(DETAIL_RAIL_WIDTH_PX);

  const selectedSession = useMemo(
    () => sessions.find((session) => session.session_id === selectedSessionId) || null,
    [sessions, selectedSessionId],
  );
  const updateBadgeTitle = versionStatus?.update_available
    ? `New upstream version available from ${versionStatus.remote}/${versionStatus.remote_ref}`
    : "";
  const selectedShareSessionCount = shareCreateSessions.filter((session) => session.checked).length;

  async function loadShareSession(nextSessionId = shareSessionId) {
    if (!shareTarget || !nextSessionId) return;
    setShareLoading(true);
    setShareErrorText("");
    try {
      const tail = await api.fetchShareTail(shareTarget.shareId, nextSessionId, SHARE_INIT_LIMIT);
      const normalized = coalesceAdjacentAssistantEvents(normalizeEvents(tail.events || []));
      setShareTranscript(normalized);
      setShareLiveCursor(tail.live_cursor || null);
      setShareHistoryCursor(tail.history_cursor || null);
      setShareHasOlder(Boolean(tail.has_older));
      setShareBusy(Boolean(tail.busy));
      setShareQueueLen(Number(tail.queue_len || 0));
      const files = await api.fetchShareFiles(shareTarget.shareId, nextSessionId);
      setShareFiles(files);
      setShareSelectedFilePath("");
      setShareSelectedFile(null);
    } catch (error) {
      const status = typeof error === "object" && error && "status" in error ? Number((error as { status?: number }).status) : 0;
      if (status === 401) {
        setShareAuthState("login");
      } else {
        setShareErrorText(error instanceof Error ? error.message : "Unable to load shared session");
      }
    } finally {
      setShareLoading(false);
    }
  }

  async function handleShareLogin() {
    if (!shareTarget) return;
    const password = sharePassword.trim();
    if (!password) {
      setSharePasswordError("Password required");
      return;
    }
    setSharePasswordError("");
    try {
      const response: ShareLoginResponse = await api.loginShareLink(shareTarget.shareId, password);
      applyShareInfo(response);
      const nextSessionId = shareTarget.sessionId || response.sessions[0]?.session_id || "";
      setShareSessionId(nextSessionId);
      setShareAuthState("ready");
      setSharePassword("");
      if (nextSessionId) await loadShareSession(nextSessionId);
    } catch (error) {
      setSharePasswordError(error instanceof Error ? error.message : "Unable to open share");
    }
  }

  function applyShareInfo(response: ShareLoginResponse) {
    setShareSet({
      share_id: response.share_id,
      label: response.share_label,
      password_hash: "",
      password_hint: "",
      expires_at: response.expires_at,
      created_at: 0,
      updated_at: 0,
      allow_interrupt: Boolean(response.allow_interrupt),
      allow_files: Boolean(response.allow_files),
      allow_attachment_downloads: Boolean(response.allow_attachment_downloads),
      session_ids: response.sessions.map((item) => item.session_id),
      sessions: response.sessions,
    });
    setShareLabel(response.share_label);
  }

  async function loadShareOlder() {
    if (!shareTarget || !shareSessionId || !shareHistoryCursor || shareLoadingOlder) return;
    setShareLoadingOlder(true);
    try {
      const history = await api.fetchShareHistory(shareTarget.shareId, shareSessionId, shareHistoryCursor, SHARE_OLDER_LIMIT);
      setShareTranscript((current) =>
        coalesceAdjacentAssistantEvents(mergeTranscriptEvents(normalizeEvents(history.events || []), current)),
      );
      setShareHistoryCursor(history.history_cursor || null);
      setShareHasOlder(Boolean(history.has_older));
    } catch (error) {
      setShareErrorText(error instanceof Error ? error.message : "Unable to load older messages");
    } finally {
      setShareLoadingOlder(false);
    }
  }

  async function selectShareFile(path: string) {
    if (!shareTarget || !shareSessionId || !path) return;
    setShareSelectedFilePath(path);
    setShareErrorText("");
    try {
      const file = await api.readShareFile(shareTarget.shareId, shareSessionId, path);
      setShareSelectedFile(file);
    } catch (error) {
      setShareErrorText(error instanceof Error ? error.message : "Unable to open file");
    }
  }

  async function handleShareSend() {
    if (!shareTarget || !shareSessionId) return;
    const text = shareSendText.trim();
    if (!text) return;
    setShareLoading(true);
    setShareErrorText("");
    const localEvent = {
      role: "user" as const,
      text,
      ts: Date.now() / 1000,
      localId: `share-local-${Date.now()}`,
    };
    setShareTranscript((current) => current.concat(normalizeEvents([localEvent])));
    setShareSendText("");
    try {
      await api.sendShareMessage(shareTarget.shareId, shareSessionId, text);
      await loadShareSession(shareSessionId);
    } catch (error) {
      setShareErrorText(error instanceof Error ? error.message : "Unable to send message");
    } finally {
      setShareLoading(false);
    }
  }

  async function handleShareInterrupt() {
    if (!shareTarget || !shareSessionId || !shareSet?.allow_interrupt) return;
    setShareLoading(true);
    setShareErrorText("");
    try {
      await api.interruptShareSession(shareTarget.shareId, shareSessionId);
      await loadShareSession(shareSessionId);
    } catch (error) {
      setShareErrorText(error instanceof Error ? error.message : "Unable to interrupt session");
    } finally {
      setShareLoading(false);
    }
  }

  useEffect(() => {
    if (!shareMode || !shareTarget) return;
    let cancelled = false;
    const init = async () => {
      setShareAuthState("loading");
      try {
        const info = await api.fetchShareInfo(shareTarget.shareId);
        if (cancelled) return;
        applyShareInfo(info);
        const sessionId = shareTarget.sessionId || info.sessions[0]?.session_id || shareSessionId;
        if (!sessionId) {
          setShareAuthState("login");
          return;
        }
        const tail = await api.fetchShareTail(shareTarget.shareId, sessionId, SHARE_INIT_LIMIT);
        if (cancelled) return;
        setShareSessionId(sessionId);
        setShareTranscript(coalesceAdjacentAssistantEvents(normalizeEvents(tail.events || [])));
        setShareLiveCursor(tail.live_cursor || null);
        setShareHistoryCursor(tail.history_cursor || null);
        setShareHasOlder(Boolean(tail.has_older));
        setShareBusy(Boolean(tail.busy));
        setShareQueueLen(Number(tail.queue_len || 0));
        setShareAuthState("ready");
        try {
          const files = await api.fetchShareFiles(shareTarget.shareId, sessionId);
          if (!cancelled) setShareFiles(files);
        } catch {
          if (!cancelled) setShareFiles(null);
        }
      } catch (error) {
        if (cancelled) return;
        setShareAuthState("login");
        if (error instanceof Error && !String(error.message).includes("401")) {
          setSharePasswordError(error.message);
        }
      }
    };
    void init();
    return () => {
      cancelled = true;
    };
  }, [shareMode, shareTarget?.shareId]);

  useEffect(() => {
    if (!shareMode || shareAuthState !== "ready" || !shareTarget || !shareSessionId || !shareLiveCursor) return;
    let cancelled = false;
    const timer = window.setInterval(async () => {
      try {
        const live = await api.fetchShareLive(shareTarget.shareId, shareSessionId, shareLiveCursor);
        if (cancelled) return;
        if (live.events?.length) {
          setShareTranscript((current) =>
            coalesceAdjacentAssistantEvents(mergeTranscriptEvents(current, normalizeEvents(live.events || []))),
          );
        }
        setShareLiveCursor(live.live_cursor || shareLiveCursor);
        setShareBusy(Boolean(live.busy));
        setShareQueueLen(Number(live.queue_len || 0));
      } catch (error) {
        if (!cancelled) setShareErrorText(error instanceof Error ? error.message : "Unable to refresh share");
      }
    }, shareBusy ? POLL_BUSY_MS : POLL_IDLE_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [shareMode, shareAuthState, shareTarget?.shareId, shareSessionId, shareLiveCursor, shareBusy]);
  const contextMenuSession = useMemo(
    () => sessions.find((session) => session.session_id === sessionContextMenu?.sessionId) || null,
    [sessionContextMenu, sessions],
  );
  const renameTargetSession = useMemo(
    () => sessionById(sessions, renameSessionId) || selectedSession,
    [renameSessionId, selectedSession, sessions],
  );
  const awaitingAssistantReply = useMemo(() => {
    if (!selectedSession || queueLen) return false;
    let latestUserTs = 0;
    let latestAssistantTs = 0;
    for (const event of transcript) {
      const ts = Number(event.ts || 0);
      if (!Number.isFinite(ts) || ts <= 0) continue;
      if (event.kind === "user") latestUserTs = Math.max(latestUserTs, ts);
      if (event.kind === "assistant") latestAssistantTs = Math.max(latestAssistantTs, ts);
    }
    return latestUserTs > latestAssistantTs;
  }, [queueLen, selectedSession, transcript]);
  const selectedSessionAwaitingReplyId =
    awaitingAssistantReply && selectedSessionId && interruptingSessionId !== selectedSessionId ? selectedSessionId : "";
  const workspaceGroups = useMemo(() => {
    const groups = new Map<
      string,
      { key: string; cwd: string; sessions: SessionSummary[]; queueLen: number; busyCount: number; updatedTs: number }
    >();
    for (const session of sessions) {
      const key = workspaceKeyForSession(session);
      const group = groups.get(key) || { key, cwd: key, sessions: [], queueLen: 0, busyCount: 0, updatedTs: 0 };
      const awaitingReply = session.session_id === selectedSessionAwaitingReplyId;
      const stopSuppressed = session.session_id === interruptingSessionId;
      group.sessions.push(session);
      group.queueLen += Number(session.queue_len || 0);
      if (sessionIsRunning(session, awaitingReply, stopSuppressed)) group.busyCount += 1;
      group.updatedTs = Math.max(group.updatedTs, Number(session.updated_ts || session.start_ts || 0));
      groups.set(key, group);
    }
    const fallbackSortedGroups = Array.from(groups.values()).sort((left, right) => {
      const leftSelected = left.sessions.some((session) => session.session_id === selectedSessionId) ? 1 : 0;
      const rightSelected = right.sessions.some((session) => session.session_id === selectedSessionId) ? 1 : 0;
      if (leftSelected !== rightSelected) return rightSelected - leftSelected;
      if (left.updatedTs !== right.updatedTs) return right.updatedTs - left.updatedTs;
      return workspaceTitle(left.cwd).localeCompare(workspaceTitle(right.cwd));
    });
    const orderedGroups = applyStoredOrder(fallbackSortedGroups, workspaceOrder, (group) => group.key).map((group) => ({
      ...group,
      sessions: partitionSidebarOrder(
        applyStoredOrder(group.sessions, sessionOrderByWorkspace[group.key] || [], (session) => session.session_id),
        (session) => sessionIsImportant(session),
        (session) => sessionIsShelved(session),
      ),
    }));
    return partitionSidebarOrder(
      orderedGroups,
      (group) => group.sessions.some((session) => sessionIsImportant(session) && !sessionIsShelved(session)),
      (group) => group.sessions.length > 0 && group.sessions.every((session) => sessionIsShelved(session)),
    );
  }, [interruptingSessionId, selectedSessionAwaitingReplyId, selectedSessionId, sessionOrderByWorkspace, sessions, workspaceOrder]);
  const groupedSessions = workspaceGroups;
  const currentNewSessionDefaults = useMemo(
    () => defaultsForBackend(newSessionDefaults, newSessionBackend),
    [newSessionDefaults, newSessionBackend],
  );
  const newSessionProviders = useMemo(
    () => (currentNewSessionDefaults?.provider_choices || []).slice(),
    [currentNewSessionDefaults],
  );
  const newSessionReasoningChoices = useMemo(
    () => (currentNewSessionDefaults?.reasoning_efforts || []).slice(),
    [currentNewSessionDefaults],
  );
  const newSessionCwdSuggestionChoices = useMemo(() => newSessionCwdSuggestions.slice(0, 12), [newSessionCwdSuggestions]);
  const visibleTranscript = useMemo(
    () =>
      coalesceAdjacentAssistantEvents(
        showTools
          ? transcript
          : transcript.filter(transcriptEventVisibleWithToolsHidden),
      ),
    [showTools, transcript],
  );
  const floatingProgressEvent = useMemo(() => {
    for (let index = visibleTranscript.length - 1; index >= 0; index -= 1) {
      const event = visibleTranscript[index];
      if (isFloatingProgressEvent(event)) return event.status === "completed" ? null : event;
    }
    return null;
  }, [visibleTranscript]);
  const floatingProgressCollapsed = Boolean(floatingProgressEvent && collapsedFloatingProgressEvents[floatingProgressEvent.id]);
  const visibleFileEntries = useMemo(
    () => (fileSearchQuery.trim() ? fileSearchEntries : fileEntries),
    [fileEntries, fileSearchEntries, fileSearchQuery],
  );
  const tmuxCommand = useMemo(() => tmuxAttachCommandForSession(selectedSession), [selectedSession]);
  const queuePreviewItems = useMemo(() => queueItems.slice(0, 3), [queueItems]);
  const selectedSessionInterrupting = Boolean(selectedSessionId && interruptingSessionId === selectedSessionId);
  const selectedSessionStoppingFeedback = Boolean(selectedSessionInterrupting && Date.now() < interruptingFeedbackUntil);
  const effectiveAwaitingAssistantReply = selectedSessionInterrupting ? false : awaitingAssistantReply;
  const selectedSessionBusy = Boolean(busy || selectedSession?.busy);
  const effectiveSelectedSessionBusy = selectedSessionInterrupting ? false : selectedSessionBusy;
  const composerSessionBusy = Boolean(effectiveAwaitingAssistantReply || effectiveSelectedSessionBusy);
  const displayedVoiceSettings = voiceSettings || EMPTY_VOICE_SETTINGS;
  const settingsInitialLoading = voiceSettingsLoading && !voiceSettings && !codexConfig;
  const queuedWaitingStatus = Boolean(
    selectedSession &&
      queueLen &&
      !closingSession &&
      !sending &&
      sessionIsQueuedWaiting(selectedSession) &&
      !(effectiveAwaitingAssistantReply || effectiveSelectedSessionBusy),
  );
  const topSessionStatus = useMemo(() => {
    if (!selectedSession) return "No session";
    if (closingSession) return "closing";
    if (selectedSessionStoppingFeedback) return "stopping";
    if (sending) return "sending";
    if (effectiveAwaitingAssistantReply || effectiveSelectedSessionBusy) return "working";
    if (queueLen) return `queue ${queueLen}`;
    if (sessionIsStarting(selectedSession)) return "starting";
    return "idle";
  }, [closingSession, effectiveAwaitingAssistantReply, effectiveSelectedSessionBusy, queueLen, selectedSession, selectedSessionStoppingFeedback, sending]);
  const topSessionStatusClass =
    topSessionStatus === "stopping"
      ? "status-chip stopping"
      : topSessionStatus === "working" || topSessionStatus === "starting"
      ? "status-chip working"
      : queuedWaitingStatus
        ? "status-chip waiting"
        : "status-chip";
  const workingIndicatorLabel =
    topSessionStatus === "stopping"
      ? "Stopping"
      : topSessionStatus === "sending"
        ? "Sending"
        : topSessionStatus === "starting"
          ? "Starting"
          : topSessionStatus === "working"
            ? "Working"
            : queuedWaitingStatus
              ? "Waiting"
              : "";
  const workingIndicatorTone = queuedWaitingStatus ? "waiting" : "working";
  const composerText = selectedSessionId ? sessionDrafts[selectedSessionId] || "" : "";
  const appStyle = useMemo(
    () =>
      ({
        ...(sidebarWidth == null ? {} : { "--sidebar-w": `${sidebarWidth}px` }),
        "--rail-w": `${detailRailWidth}px`,
      }) as JSX.CSSProperties,
    [detailRailWidth, sidebarWidth],
  );
  const sidebarToggleTitle = mobileViewport
    ? mobileSidebarOpen
      ? "Hide sidebar"
      : "Show sidebar"
    : sidebarCollapsed
      ? "Show sidebar"
      : "Hide sidebar";
  const sidebarTogglePressed = mobileViewport ? mobileSidebarOpen : sidebarCollapsed;
  const mobileSidebarToggleLabel = mobileSidebarOpen ? "Close" : "Sessions";

  function sidebarRailWidth(viewportWidth = window.innerWidth) {
    return showFilesPanel || showDetailsPanel
      ? viewportWidth > DETAIL_RAIL_STACK_BREAKPOINT_PX
        ? detailRailWidth + DETAIL_RAIL_RESIZER_WIDTH_PX
        : 0
      : 0;
  }

  function currentSidebarLayoutWidth(viewportWidth = window.innerWidth) {
    const shellWidth = appRef.current?.getBoundingClientRect().width || viewportWidth;
    return Math.max(shellWidth - SIDEBAR_RESIZER_WIDTH_PX - sidebarRailWidth(shellWidth), SIDEBAR_MIN_WIDTH_PX + MAIN_MIN_WIDTH_PX);
  }

  function askUserDefaultsOpen(event: UiTranscriptEvent) {
    return event.kind === "ask_user" && !event.askResolved && !event.askAnswer && !event.askCancelled;
  }

  function setSessionDraft(sessionId: string, text: string) {
    if (!sessionId) return;
    setSessionDrafts((current) => {
      const existing = current[sessionId] || "";
      if (text) {
        if (existing === text) return current;
        return { ...current, [sessionId]: text };
      }
      if (!(sessionId in current)) return current;
      const next = { ...current };
      delete next[sessionId];
      return next;
    });
  }

  function transcriptEventCollapsed(event: UiTranscriptEvent, collapsedState = collapsedEvents) {
    if (!isCollapsibleEvent(event.kind)) return false;
    return askUserDefaultsOpen(event) ? collapsedState[event.id] === true : collapsedState[event.id] !== false;
  }

  function toggleTranscriptEvent(event: UiTranscriptEvent) {
    setCollapsedEvents((current) => ({ ...current, [event.id]: !transcriptEventCollapsed(event, current) }));
  }

  function toggleFloatingProgress(eventId: string) {
    setCollapsedFloatingProgressEvents((current) => {
      if (current[eventId]) {
        const next = { ...current };
        delete next[eventId];
        return next;
      }
      return { ...current, [eventId]: true };
    });
  }

  function toggleWorkspaceGroup(workspaceKey: string) {
    setCollapsedWorkspaces((current) => ({ ...current, [workspaceKey]: !current[workspaceKey] }));
  }

  function closeMobileSidebar() {
    setMobileSidebarOpen(false);
  }

  function cacheSessionSnapshot(sessionId: string, patch: Partial<SessionViewSnapshot>) {
    if (!sessionId) return;
    const current = sessionViewCacheRef.current[sessionId] || {
      transcript: [],
      liveCursor: null,
      historyCursor: null,
      hasOlder: false,
      busy: false,
      queueLen: 0,
      tokenSummary: null,
    };
    sessionViewCacheRef.current[sessionId] = { ...current, ...patch };
  }

  function rememberActiveSessionSnapshot(sessionId = selectedSessionRef.current) {
    if (!sessionId) return;
    cacheSessionSnapshot(sessionId, {
      transcript,
      liveCursor: liveCursorRef.current,
      historyCursor: historyCursorRef.current,
      hasOlder,
      busy,
      queueLen,
      tokenSummary,
    });
  }

  function toggleSidebarVisibility() {
    if (mobileViewport) {
      setMobileSidebarOpen((current) => !current);
      return;
    }
    setSidebarCollapsed((current) => !current);
  }

  function handleSidebarResizeStart(event: SidebarPointerEvent) {
    event.preventDefault();
    if (sidebarCollapsed) return;
    sidebarResizeStartXRef.current = event.clientX;
    sidebarResizeStartWidthRef.current = clampSidebarWidth(
      sidebarRef.current?.getBoundingClientRect().width || sidebarWidth || 320,
      currentSidebarLayoutWidth(),
    );
    setSidebarResizing(true);
  }

  function handleDetailRailResizeStart(event: SidebarPointerEvent) {
    event.preventDefault();
    if (mobileViewport || window.innerWidth <= DETAIL_RAIL_STACK_BREAKPOINT_PX) return;
    detailRailResizeStartXRef.current = event.clientX;
    detailRailResizeStartWidthRef.current = clampDetailRailWidth(detailRailWidth);
    setDetailRailResizing(true);
  }

  function handleWorkspaceDragStart(event: SidebarDragEvent, workspaceKey: string) {
    setDraggingWorkspaceKey(workspaceKey);
    setSidebarDropIndicator(null);
    event.dataTransfer!.effectAllowed = "move";
    event.dataTransfer!.setData("text/plain", workspaceKey);
  }

  function handleWorkspaceDragOver(event: SidebarDragEvent, targetWorkspaceKey: string) {
    if (!draggingWorkspaceKey) return;
    if (draggingWorkspaceKey === targetWorkspaceKey) {
      setSidebarDropIndicator(null);
      return;
    }
    event.preventDefault();
    setSidebarDropIndicator({ kind: "workspace", key: targetWorkspaceKey, position: dropPositionFromEvent(event) });
  }

  function handleWorkspaceDrop(event: SidebarDragEvent, targetWorkspaceKey: string) {
    if (!draggingWorkspaceKey || draggingWorkspaceKey === targetWorkspaceKey) return;
    event.preventDefault();
    const position =
      sidebarDropIndicator?.kind === "workspace" && sidebarDropIndicator.key === targetWorkspaceKey
        ? sidebarDropIndicator.position
        : dropPositionFromEvent(event);
    setWorkspaceOrder(moveOrderedKey(workspaceGroups.map((group) => group.key), draggingWorkspaceKey, targetWorkspaceKey, position));
    setDraggingWorkspaceKey("");
    setSidebarDropIndicator(null);
  }

  function handleSessionDragStart(event: SidebarDragEvent, workspaceKey: string, sessionId: string) {
    event.stopPropagation();
    setDraggingSession({ workspaceKey, sessionId });
    setSidebarDropIndicator(null);
    event.dataTransfer!.effectAllowed = "move";
    event.dataTransfer!.setData("text/plain", sessionId);
  }

  function handleSessionDragOver(event: SidebarDragEvent, workspaceKey: string, targetSessionId: string) {
    if (!draggingSession || draggingSession.workspaceKey !== workspaceKey) return;
    if (draggingSession.sessionId === targetSessionId) {
      setSidebarDropIndicator(null);
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    setSidebarDropIndicator({ kind: "session", key: targetSessionId, position: dropPositionFromEvent(event) });
  }

  function handleSessionDrop(event: SidebarDragEvent, workspaceKey: string, targetSessionId: string) {
    if (!draggingSession) return;
    event.preventDefault();
    event.stopPropagation();
    if (draggingSession.workspaceKey !== workspaceKey || draggingSession.sessionId === targetSessionId) return;
    const group = workspaceGroups.find((item) => item.key === workspaceKey);
    if (!group) return;
    const position =
      sidebarDropIndicator?.kind === "session" && sidebarDropIndicator.key === targetSessionId
        ? sidebarDropIndicator.position
        : dropPositionFromEvent(event);
    const nextOrder = moveOrderedKey(group.sessions.map((session) => session.session_id), draggingSession.sessionId, targetSessionId, position);
    setSessionOrderByWorkspace((current) => ({ ...current, [workspaceKey]: nextOrder }));
    setDraggingSession(null);
    setSidebarDropIndicator(null);
  }

  function openSessionContextMenu(event: SidebarContextMenuEvent, session: SessionSummary) {
    event.preventDefault();
    event.stopPropagation();
    setSessionContextMenu({ sessionId: session.session_id, x: event.clientX, y: event.clientY });
  }

  function focusSidebarSession(sessionId: string) {
    setSessionContextMenu(null);
    if (selectedSessionRef.current !== sessionId) selectSession(sessionId);
  }

  function pushToast(text: string) {
    setToastText(text);
    window.setTimeout(() => {
      setToastText((current) => (current === text ? "" : current));
    }, 2200);
  }

  function selectSession(sessionId: string) {
    const changed = selectedSessionRef.current !== sessionId;
    const previousSessionId = selectedSessionRef.current;
    if (changed) rememberActiveSessionSnapshot(previousSessionId);
    selectedSessionRef.current = sessionId;
    if (changed) {
      const cached = sessionViewCacheRef.current[sessionId];
      stickToBottomRef.current = true;
      lastAutoScrollKeyRef.current = "";
      setHistoryTopBoundaryReached(true);
      if (cached) {
        liveCursorRef.current = cached.liveCursor;
        historyCursorRef.current = cached.historyCursor;
        setLiveCursor(cached.liveCursor);
        setHistoryCursor(cached.historyCursor);
        setHasOlder(cached.hasOlder);
        setTranscript(cached.transcript);
        setBusy(cached.busy);
        setQueueLen(cached.queueLen);
        setTokenSummary(cached.tokenSummary);
      } else {
        liveCursorRef.current = null;
        historyCursorRef.current = null;
        setLiveCursor(null);
        setHistoryCursor(null);
        setHasOlder(false);
        setTranscript([]);
        setBusy(false);
        setQueueLen(Number(sessions.find((session) => session.session_id === sessionId)?.queue_len || 0));
        setTokenSummary(null);
      }
    }
    if (mobileViewport) closeMobileSidebar();
    setSelectedSessionId(sessionId);
    if (sessionId) {
      writeLocalStorage(SELECTED_SESSION_KEY, sessionId);
      writeSessionHash(sessionId);
    } else {
      writeLocalStorage(SELECTED_SESSION_KEY, null);
      writeSessionHash("");
    }
  }

  function pinSidebarOrderForSessions(nextSessions: SessionSummary[]) {
    const nextWorkspaceKeys = uniqueStringsInOrder(nextSessions.map((session) => workspaceKeyForSession(session)));
    setWorkspaceOrder((current) => {
      const next = uniqueStringsInOrder([
        ...current.filter((workspaceKey) => nextWorkspaceKeys.includes(workspaceKey)),
        ...nextWorkspaceKeys,
      ]);
      return sameStringList(current, next) ? current : next;
    });
    setSessionOrderByWorkspace((current) => {
      const sessionsByWorkspace = new Map<string, string[]>();
      nextSessions.forEach((session) => {
        const workspaceKey = workspaceKeyForSession(session);
        sessionsByWorkspace.set(workspaceKey, [...(sessionsByWorkspace.get(workspaceKey) || []), session.session_id]);
      });
      const next: SidebarSessionOrder = {};
      let changed = Object.keys(current).some((workspaceKey) => !sessionsByWorkspace.has(workspaceKey) && (current[workspaceKey] || []).length > 0);
      sessionsByWorkspace.forEach((sessionIds, workspaceKey) => {
        next[workspaceKey] = uniqueStringsInOrder([
          ...(current[workspaceKey] || []).filter((sessionId) => sessionIds.includes(sessionId)),
          ...sessionIds,
        ]);
        if (!sameStringList(current[workspaceKey] || [], next[workspaceKey])) changed = true;
      });
      return changed ? next : current;
    });
  }

  function promoteOpenedSession(session: SessionSummary, previousSessions: SessionSummary[], nextSessions: SessionSummary[]) {
    const workspaceKey = workspaceKeyForSession(session);
    const workspaceAlreadyOpen = previousSessions.some(
      (item) => item.session_id !== session.session_id && workspaceKeyForSession(item) === workspaceKey,
    );
    if (!workspaceAlreadyOpen) setWorkspaceOrder((current) => prependOrderedKey(current, workspaceKey));
    const siblingSessionIds = nextSessions
      .filter((item) => item.session_id !== session.session_id && workspaceKeyForSession(item) === workspaceKey)
      .map((item) => item.session_id);
    setSessionOrderByWorkspace((current) => ({
      ...current,
      [workspaceKey]: appendOrderedKey(
        [
          ...(current[workspaceKey] || []).filter((sessionId) => siblingSessionIds.includes(sessionId)),
          ...siblingSessionIds.filter((sessionId) => !(current[workspaceKey] || []).includes(sessionId)),
        ],
        session.session_id,
      ),
    }));
  }

  function focusComposerInput() {
    window.setTimeout(() => composerInputRef.current?.focus(), 0);
  }

  function updateChatScrollFollowState() {
    const element = chatScrollRef.current;
    if (!element) {
      setHistoryTopBoundaryReached(true);
      return;
    }
    stickToBottomRef.current = isNearScrollBottom(element);
    setHistoryTopBoundaryReached(isNearHistoryTop(element));
  }

  function scrollChatEventIntoView(eventId: string, offsetPx = CHAT_HISTORY_JUMP_OFFSET_PX) {
    const element = chatScrollRef.current;
    if (!element || !eventId) return;
    const target = Array.from(element.querySelectorAll<HTMLElement>("[data-event-id]")).find((node) => node.dataset.eventId === eventId);
    if (!target) return;
    const elementTop = element.getBoundingClientRect().top;
    const targetTop = target.getBoundingClientRect().top;
    const nextTop = Math.max(0, element.scrollTop + (targetTop - elementTop) - offsetPx);
    element.scrollTo({ top: nextTop });
    stickToBottomRef.current = isNearScrollBottom(element);
    setHistoryTopBoundaryReached(isNearHistoryTop(element));
  }

  function applyRuntime(data: {
    live_cursor?: string | null;
    history_cursor?: string | null;
    has_older?: boolean;
    busy: boolean;
    queue_len: number;
    turn_aborted?: boolean;
    token?: Record<string, unknown> | null;
  }, sessionId = selectedSessionRef.current, transcriptEvents?: UiTranscriptEvent[]) {
    liveCursorRef.current = data.live_cursor ?? null;
    setLiveCursor(liveCursorRef.current);
    let nextHistoryCursor = historyCursorRef.current;
    let nextHasOlder = sessionViewCacheRef.current[sessionId]?.hasOlder ?? hasOlder;
    if (Object.prototype.hasOwnProperty.call(data, "history_cursor")) {
      historyCursorRef.current = data.history_cursor ?? null;
      nextHistoryCursor = historyCursorRef.current;
      setHistoryCursor(historyCursorRef.current);
    }
    if (Object.prototype.hasOwnProperty.call(data, "has_older")) {
      nextHasOlder = Boolean(data.has_older);
      setHasOlder(nextHasOlder);
    }
    const nextBusy = Boolean(data.busy);
    const nextQueueLen = Number.isFinite(Number(data.queue_len)) ? Number(data.queue_len) : 0;
    const nextTokenSummary = renderTokenSummary(data.token);
    setBusy(nextBusy);
    setQueueLen(nextQueueLen);
    setTokenSummary(nextTokenSummary);
    if (sessionId) {
      cacheSessionSnapshot(sessionId, {
        ...(transcriptEvents ? { transcript: transcriptEvents } : {}),
        liveCursor: liveCursorRef.current,
        historyCursor: nextHistoryCursor,
        hasOlder: nextHasOlder,
        busy: nextBusy,
        queueLen: nextQueueLen,
        tokenSummary: nextTokenSummary,
      });
    }
    if (sessionId && (!data.busy || data.turn_aborted)) {
      setInterruptingSessionId((current) => (current === sessionId ? "" : current));
      setInterruptingFeedbackUntil(0);
    }
  }

  async function refreshSessions({ preserveSelection = true }: { preserveSelection?: boolean } = {}) {
    const payload = await api.fetchSessions();
    const ordered = sortSessions(payload.sessions || []);
    const orderedSessionIds = new Set(ordered.map((session) => session.session_id));
    Object.keys(sessionViewCacheRef.current).forEach((sessionId) => {
      if (!orderedSessionIds.has(sessionId)) delete sessionViewCacheRef.current[sessionId];
    });
    setSessions(ordered);
    pinSidebarOrderForSessions(ordered);
    setRecentCwds(Array.isArray(payload.recent_cwds) ? payload.recent_cwds : []);
    setNewSessionDefaults(payload.new_session_defaults || null);
    setTmuxAvailable(Boolean(payload.tmux_available));
    setErrorText("");
    if (payload.new_session_defaults) {
      if (!newSessionDefaults) {
        const backend = preferredNewSessionBackend(payload.new_session_defaults, newSessionPreferences);
        const values = newSessionValuesForBackend(payload.new_session_defaults, backend, newSessionPreferences, Boolean(payload.tmux_available));
        setNewSessionBackend(backend);
        setNewSessionProvider(values.provider);
        setNewSessionModel(values.model);
        setNewSessionReasoning(values.reasoning);
        setNewSessionFast(values.fast);
        setNewSessionTmux(values.tmux);
      }
    }
    if (!ordered.length) {
      selectSession("");
      sessionViewCacheRef.current = {};
      setTranscript([]);
      return;
    }
    const stored = readLocalStorage(SELECTED_SESSION_KEY) || sessionIdFromHash();
    const current = preserveSelection ? selectedSessionRef.current || stored : stored;
    const nextSelected = ordered.some((item) => item.session_id === current) ? current : ordered[0].session_id;
    selectSession(nextSelected);
  }

  async function refreshVersionStatus() {
    try {
      setVersionStatus(await api.fetchVersionStatus());
    } catch {
      setVersionStatus(null);
    }
  }

  function schedulePoll(delayMs: number) {
    if (pollTimerRef.current !== null) window.clearTimeout(pollTimerRef.current);
    pollTimerRef.current = window.setTimeout(() => {
      void pollLive();
    }, delayMs);
  }

  async function openSession(sessionId: string) {
    if (!sessionId) return;
    const requestId = openRequestRef.current + 1;
    openRequestRef.current = requestId;
    if (!sessionViewCacheRef.current[sessionId]) setLoadingText("Loading session…");
    try {
      const data = await api.fetchTail(sessionId, INIT_PAGE_LIMIT);
      if (requestId !== openRequestRef.current || selectedSessionRef.current !== sessionId) return;
      const nextEvents = normalizeEvents(data.events || []);
      setTranscript(nextEvents);
      setSessionLastLines((current) => ({ ...current, [sessionId]: previewFromEvents(nextEvents) || current[sessionId] || "" }));
      applyRuntime(data, sessionId, nextEvents);
      if (showDetailsPanel) void loadDiagnostics(sessionId);
      if (showFilesPanel) void loadFiles(sessionId);
      schedulePoll(0);
    } catch (error) {
      if (requestId !== openRequestRef.current || selectedSessionRef.current !== sessionId) return;
      const status = error && typeof error === "object" && "status" in error ? Number((error as { status?: number }).status) : 0;
      if (status === 401) {
        setAuthState("login");
        return;
      }
      setErrorText(error instanceof Error ? error.message : "Unable to open session");
    }
  }

  async function pollLive() {
    const sessionId = selectedSessionRef.current;
    if (!sessionId) return;
    try {
      if (!liveCursorRef.current) {
        await openSession(sessionId);
        return;
      }
      const data = await api.fetchLive(sessionId, liveCursorRef.current);
      if (selectedSessionRef.current !== sessionId) return;
      const nextEvents = normalizeEvents(data.events || []);
      if (nextEvents.length) {
        setTranscript((current) => {
          const merged = mergeTranscriptEvents(current, nextEvents);
          setSessionLastLines((prev) => ({ ...prev, [sessionId]: previewFromEvents(merged) || prev[sessionId] || "" }));
          cacheSessionSnapshot(sessionId, { transcript: merged });
          return merged;
        });
      }
      applyRuntime(data, sessionId);
      if (nextEvents.length || data.turn_end || data.turn_start || data.turn_aborted) {
        void refreshSessions();
      }
      const delay = data.busy || Date.now() < fastPollUntilRef.current ? POLL_BUSY_MS : POLL_IDLE_MS;
      schedulePoll(delay);
    } catch (error) {
      const status = error && typeof error === "object" && "status" in error ? Number((error as { status?: number }).status) : 0;
      if (status === 401) {
        setAuthState("login");
        return;
      }
      if (status === 409) {
        await openSession(sessionId);
        return;
      }
      schedulePoll(2500);
    }
  }

  async function loadOlder() {
    if (!selectedSessionId || !historyCursorRef.current || loadingOlder) return;
    const sessionId = selectedSessionId;
    setLoadingOlder(true);
    try {
      const data = await api.fetchHistory(sessionId, historyCursorRef.current, OLDER_PAGE_LIMIT);
      if (selectedSessionRef.current !== sessionId) return;
      const older = normalizeEvents(data.events || []);
      const jumpTargetId = olderHistoryJumpTarget(older);
      setTranscript((current) => {
        const merged = mergeTranscriptEvents(older, current);
        cacheSessionSnapshot(sessionId, { transcript: merged });
        return merged;
      });
      historyCursorRef.current = data.history_cursor ?? null;
      setHistoryCursor(historyCursorRef.current);
      setHasOlder(Boolean(data.has_older));
      setBusy(Boolean(data.busy));
      const nextQueueLen = Number.isFinite(Number(data.queue_len)) ? Number(data.queue_len) : 0;
      setQueueLen(nextQueueLen);
      const nextTokenSummary = renderTokenSummary(data.token);
      setTokenSummary(nextTokenSummary);
      cacheSessionSnapshot(sessionId, {
        historyCursor: historyCursorRef.current,
        hasOlder: Boolean(data.has_older),
        busy: Boolean(data.busy),
        queueLen: nextQueueLen,
        tokenSummary: nextTokenSummary,
      });
      if (jumpTargetId) {
        window.requestAnimationFrame(() => {
          scrollChatEventIntoView(jumpTargetId);
        });
      }
    } catch (error) {
      const status = error && typeof error === "object" && "status" in error ? Number((error as { status?: number }).status) : 0;
      if (status === 409) {
        await openSession(sessionId);
      } else {
        setErrorText(error instanceof Error ? error.message : "Unable to load older messages");
      }
    } finally {
      setLoadingOlder(false);
    }
  }

  async function copyTmuxAttachCommand() {
    if (!tmuxCommand) {
      pushToast("tmux unavailable for this session");
      return;
    }
    try {
      await copyToClipboard(tmuxCommand);
      pushToast("Copied tmux attach command");
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to copy tmux command");
    }
  }

  async function copyTranscriptEvent(event: UiTranscriptEvent) {
    const text = String(event.body || "");
    if (!text.trim()) {
      pushToast("Nothing to copy");
      return;
    }
    try {
      await copyToClipboard(text);
      pushToast(event.kind === "user" ? "Copied input" : "Copied reply");
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to copy message");
    }
  }

  async function copyShareInvitation() {
    if (!shareCreateResult) return;
    try {
      await copyToClipboard(shareInvitationText(shareCreateResult));
      pushToast("Share invite copied");
    } catch (error) {
      setShareCreateError(error instanceof Error ? error.message : "Unable to copy share invite");
    }
  }

  async function refreshManagedShares() {
    setManagedSharesLoading(true);
    try {
      const response = await api.fetchShareLinks();
      setManagedShares((current) => {
        const passwordById = new Map(current.map((share) => [share.share_id, share.share_password || ""]));
        if (shareCreateResult?.share_password) passwordById.set(shareCreateResult.share_id, shareCreateResult.share_password);
        return (response.shares || []).map((share) => ({
          ...share,
          share_password: passwordById.get(share.share_id) || undefined,
        }));
      });
    } catch (error) {
      setShareCreateError(error instanceof Error ? error.message : "Unable to load share links");
    } finally {
      setManagedSharesLoading(false);
    }
  }

  async function copyManagedShare(share: ManagedShareSet) {
    try {
      await copyToClipboard(shareSetCopyText(share));
      pushToast(share.share_password ? "Share invite copied" : "Share URL copied");
    } catch (error) {
      setShareCreateError(error instanceof Error ? error.message : "Unable to copy share");
    }
  }

  async function deleteManagedShare(shareId: string) {
    setDeletingShareId(shareId);
    setShareCreateError("");
    try {
      await api.deleteShareLink(shareId);
      setManagedShares((current) => current.filter((share) => share.share_id !== shareId));
      if (shareCreateResult?.share_id === shareId) setShareCreateResult(null);
      pushToast("Share closed");
    } catch (error) {
      setShareCreateError(error instanceof Error ? error.message : "Unable to close share");
    } finally {
      setDeletingShareId("");
    }
  }

  function openShareCreateDialog() {
    if (!selectedSession) return;
    const candidates = sortSessions(sessions).map((session) => {
      const workspaceCwd = workspaceKeyForSession(session);
      return {
        session_id: session.session_id,
        label: sessionDisplayName(session),
        workspace: workspaceTitle(workspaceCwd),
        cwd: workspaceCwd === "__unknown_workspace__" ? "" : workspaceCwd,
        checked: session.session_id === selectedSession.session_id,
      };
    });
    setShareCreateSessions(candidates);
    setShareCreateLabel(sessionDisplayName(selectedSession));
    setShareCreateExpiresHours(24);
    setShareCreateError("");
    setShareCreateResult(null);
    setShareCreateOpen(true);
    void refreshManagedShares();
  }

  function toggleShareDraftSession(sessionId: string) {
    setShareCreateSessions((current) =>
      current.map((session) => (session.session_id === sessionId ? { ...session, checked: !session.checked } : session)),
    );
  }

  async function handleCreateShareLink() {
    const selected = shareCreateSessions.filter((session) => session.checked);
    if (!selected.length) {
      setShareCreateError("Select at least one session");
      return;
    }
    const expiresInHours = Math.max(1, Math.min(24 * 30, Math.round(Number(shareCreateExpiresHours) || 24)));
    setShareCreateBusy(true);
    setShareCreateError("");
    try {
      const nicknames = Object.fromEntries(selected.map((session) => [session.session_id, session.label]));
      const response = await api.createShareLink({
        label: shareCreateLabel.trim() || selected[0]?.label || "Shared sessions",
        session_ids: selected.map((session) => session.session_id),
        nicknames,
        expires_in_hours: expiresInHours,
        allow_interrupt: true,
        allow_files: true,
        allow_attachment_downloads: true,
      });
      setShareCreateExpiresHours(expiresInHours);
      setShareCreateResult(response);
      setManagedShares((current) => {
        const nextShare: ManagedShareSet = {
          share_id: response.share_id,
          label: response.share_label,
          password_hash: "",
          password_hint: response.share_password.slice(0, 4),
          expires_at: response.expires_at,
          created_at: Date.now() / 1000,
          updated_at: Date.now() / 1000,
          allow_interrupt: true,
          allow_files: true,
          allow_attachment_downloads: true,
          session_ids: response.sessions.map((session) => session.session_id),
          sessions: response.sessions,
          share_password: response.share_password,
        };
        return [nextShare, ...current.filter((share) => share.share_id !== response.share_id)];
      });
      pushToast("Share link created");
    } catch (error) {
      setShareCreateError(error instanceof Error ? error.message : "Unable to create share link");
    } finally {
      setShareCreateBusy(false);
    }
  }

  async function loadDiagnostics(sessionId = selectedSessionRef.current) {
    if (!sessionId) return;
    setDetailsLoading(true);
    try {
      const data = await api.fetchDiagnostics(sessionId);
      if (selectedSessionRef.current !== sessionId) return;
      setDiagnostics(data);
      setErrorText("");
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to load diagnostics");
    } finally {
      setDetailsLoading(false);
    }
  }

  async function loadFiles(sessionId = selectedSessionRef.current, options: { showLoading?: boolean } = {}) {
    if (!sessionId) return;
    const session = sessions.find((item) => item.session_id === sessionId) || null;
    const showLoading = Boolean(options.showLoading || filesLoadedSessionRef.current !== sessionId);
    if (showLoading) setFilesLoading(true);
    setFileOpenError("");
    let changedFiles: ChangedFilesResponse | null = null;
    try {
      changedFiles = await api.fetchChangedFiles(sessionId);
    } catch {
      changedFiles = null;
    }
    if (selectedSessionRef.current !== sessionId) {
      if (showLoading) setFilesLoading(false);
      return;
    }
    const entries = buildFileEntries(session, changedFiles);
    filesLoadedSessionRef.current = sessionId;
    setFileEntries(entries);
    if (showLoading) setFilesLoading(false);
    if (entries.length && !activeFilePathRef.current) {
      void openFile(entries[0].request_path);
    }
  }

  async function openFile(path: string) {
    if (!selectedSessionRef.current || !path) return;
    const sessionId = selectedSessionRef.current;
    activeFilePathRef.current = path;
    setActiveFilePath(path);
    setFileOpenError("");
    setFileOpenLoading(true);
    try {
      const data = await api.readSessionFile(sessionId, path);
      if (selectedSessionRef.current !== sessionId) return;
      setActiveFile(data);
      setErrorText("");
    } catch (error) {
      const message = error instanceof Error ? error.message : "Unable to open file";
      if (selectedSessionRef.current !== sessionId) return;
      setActiveFile(null);
      setFileOpenError(message);
      setErrorText(message);
    } finally {
      if (selectedSessionRef.current === sessionId) {
        setFileOpenLoading(false);
      }
    }
  }

  async function searchFiles(query = fileSearchQuery) {
    const sessionId = selectedSessionRef.current;
    const trimmed = query.trim();
    if (!sessionId || !trimmed) {
      setFileSearchEntries([]);
      return;
    }
    setFileSearchLoading(true);
    try {
      const data = await api.searchSessionFiles(sessionId, trimmed, 120);
      if (selectedSessionRef.current !== sessionId) return;
      setFileSearchEntries(
        (Array.isArray(data.matches) ? data.matches : [])
          .filter((item) => item && typeof item.path === "string" && item.path.trim())
          .map((item) => ({
            request_path: item.path.trim(),
            display_path: item.path.trim(),
            summary: data.truncated ? "Search result · truncated" : "Search result",
            source: "search" as const,
          })),
      );
      setErrorText("");
    } catch (error) {
      setFileSearchEntries([]);
      setErrorText(error instanceof Error ? error.message : "Unable to search files");
    } finally {
      setFileSearchLoading(false);
    }
  }

  async function loadQueue(sessionId = selectedSessionRef.current) {
    if (!sessionId) return;
    setQueueLoading(true);
    try {
      const response = await api.fetchQueue(sessionId);
      if (selectedSessionRef.current !== sessionId) return;
      const items = normalizeQueueItems(response);
      setQueueItems(items);
      setQueueLen(items.length);
      setQueueDrafts(Object.fromEntries(items.map((item) => [item.id, item.text])));
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to load queue");
    } finally {
      setQueueLoading(false);
    }
  }

  async function saveQueueItem(itemId: string) {
    if (!selectedSessionRef.current) return;
    const nextText = String(queueDrafts[itemId] || "").trim();
    if (!nextText) {
      await deleteQueueItem(itemId);
      return;
    }
    try {
      await api.updateQueueItem(selectedSessionRef.current, itemId, nextText);
      pushToast("Queue updated");
      await loadQueue(selectedSessionRef.current);
      await refreshSessions();
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to update queue item");
    }
  }

  async function deleteQueueItem(itemId: string) {
    if (!selectedSessionRef.current) return;
    try {
      await api.deleteQueueItem(selectedSessionRef.current, itemId);
      pushToast("Queue item deleted");
      await loadQueue(selectedSessionRef.current);
      await refreshSessions();
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to delete queue item");
    }
  }

  async function moveQueueItem(itemId: string, toIndex: number) {
    if (!selectedSessionRef.current) return;
    try {
      await api.moveQueueItem(selectedSessionRef.current, itemId, toIndex);
      await loadQueue(selectedSessionRef.current);
      await refreshSessions();
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to move queue item");
    }
  }

  async function loadHarness(sessionId = selectedSessionRef.current) {
    if (!sessionId) return;
    setHarnessLoading(true);
    try {
      const data = await api.fetchHarness(sessionId);
      if (selectedSessionRef.current !== sessionId) return;
      setHarnessDraft({
        enabled: Boolean(data.enabled),
        request: String(data.request || ""),
        cooldown_minutes: Number(data.cooldown_minutes || 5),
        remaining_injections: Number(data.remaining_injections || 0),
      });
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to load harness config");
    } finally {
      setHarnessLoading(false);
    }
  }

  async function saveHarness() {
    if (!selectedSessionRef.current || !harnessDraft) return;
    setHarnessSaving(true);
    try {
      const saved = await api.saveHarness(selectedSessionRef.current, harnessDraft);
      setHarnessDraft({
        enabled: Boolean(saved.enabled),
        request: String(saved.request || ""),
        cooldown_minutes: Number(saved.cooldown_minutes || 5),
        remaining_injections: Number(saved.remaining_injections || 0),
      });
      setSessions((current) =>
        current.map((session) =>
          session.session_id === selectedSessionRef.current
            ? {
                ...session,
                harness_enabled: Boolean(saved.enabled),
                harness_cooldown_minutes: Number(saved.cooldown_minutes || 5),
                harness_remaining_injections: Number(saved.remaining_injections || 0),
              }
            : session,
        ),
      );
      pushToast("Harness saved");
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to save harness");
    } finally {
      setHarnessSaving(false);
    }
  }

  async function handleSend() {
    if (sending) return;
    const sessionId = selectedSessionRef.current;
    const text = composerText.trim();
    if (!text || !sessionId) return;
    const shouldQueue = composerSessionBusy && busySubmitMode === "queue";
    const shouldInterrupt = composerSessionBusy && busySubmitMode === "interrupt";
    let localEvent: UiTranscriptEvent | null = null;
    setSessionDraft(sessionId, "");
    setSending(true);
    setErrorText("");
    setInterruptingSessionId((current) => (current === sessionId ? "" : current));
    setInterruptingFeedbackUntil(0);
    if (!shouldQueue) {
      localEvent = {
        id: `local-user-${Date.now()}`,
        kind: "user",
        ts: Date.now() / 1000,
        title: "User",
        body: text,
        meta: "sending",
      };
      setTranscript((current) => {
        const nextTranscript = current.concat(localEvent as UiTranscriptEvent);
        cacheSessionSnapshot(sessionId, { transcript: nextTranscript });
        return nextTranscript;
      });
      setSessionLastLines((current) => ({ ...current, [sessionId]: eventTextPreview(localEvent as UiTranscriptEvent) }));
    }
    try {
      if (shouldQueue) {
        const response = await api.enqueueMessage(sessionId, text);
        setQueueLen(Number(response.queue_len || queueLen + 1));
        pushToast(`Queued${response.queue_len ? ` (${response.queue_len})` : ""}`);
        void loadQueue(sessionId);
        await refreshSessions();
      } else {
        if (shouldInterrupt) await api.interrupt(sessionId);
        await api.sendMessage(sessionId, text);
        if (localEvent) {
          setTranscript((current) => {
            const nextTranscript = current.map((event) => (event.id === localEvent.id ? { ...event, meta: "sent" } : event));
            cacheSessionSnapshot(sessionId, { transcript: nextTranscript });
            return nextTranscript;
          });
        }
        pushToast(shouldInterrupt ? "Interrupted and sent" : "Sent");
      }
      fastPollUntilRef.current = Date.now() + 5000;
      schedulePoll(0);
    } catch (error) {
      setSessionDraft(sessionId, text);
      if (localEvent) {
        setTranscript((current) => {
          const nextTranscript = current.map((event) => (event.id === localEvent.id ? { ...event, meta: "send failed" } : event));
          cacheSessionSnapshot(sessionId, { transcript: nextTranscript });
          return nextTranscript;
        });
      }
      setErrorText(error instanceof Error ? error.message : "Unable to send message");
    } finally {
      setSending(false);
      focusComposerInput();
    }
  }

  async function handleAskUserRespond(event: UiTranscriptEvent, text: string) {
    const sessionId = selectedSessionRef.current;
    if (!sessionId) throw new Error("No session selected");
    await api.sendMessage(sessionId, text);
    fastPollUntilRef.current = Date.now() + 5000;
    schedulePoll(0);
    pushToast(event.askQuestion ? "Answer sent" : "Response sent");
  }

  function handleComposerKeyDown(event: KeyboardEvent) {
    if (event.key !== "Enter" || event.shiftKey || event.isComposing) return;
    event.preventDefault();
    void handleSend();
  }

  async function handleCloseSession(targetSession = selectedSession) {
    const sessionId = targetSession?.session_id || selectedSessionRef.current;
    if (!sessionId || closingSession) return;
    if (!window.confirm(`Close ${sessionDisplayName(targetSession || selectedSession)}?`)) return;
    setSessionContextMenu(null);
    setClosingSession(true);
    try {
      await api.deleteSession(sessionId);
      delete sessionViewCacheRef.current[sessionId];
      setSessionDraft(sessionId, "");
      pushToast("Session closed");
      if (selectedSessionRef.current === sessionId) {
        selectSession("");
        setTranscript([]);
      }
      await refreshSessions({ preserveSelection: false });
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to close session");
    } finally {
      setClosingSession(false);
    }
  }

  async function handleQueueAction() {
    const sessionId = selectedSessionRef.current;
    if (!sessionId) return;
    const text = composerText.trim();
    if (!text) {
      openQueueModal();
      return;
    }
    try {
      await api.enqueueMessage(sessionId, text);
      setSessionDraft(sessionId, "");
      pushToast("Queued");
      await refreshSessions();
      setQueueOpen(true);
      await loadQueue(sessionId);
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to queue message");
    }
  }

  function openQueueModal() {
    if (!selectedSessionRef.current) return;
    setQueueOpen(true);
    void loadQueue(selectedSessionRef.current);
  }

  async function handleInterrupt(targetSession = selectedSession) {
    const sessionId = targetSession?.session_id || selectedSessionRef.current;
    if (!sessionId || (interruptingSessionId === sessionId && Date.now() < interruptingFeedbackUntil)) return;
    setSessionContextMenu(null);
    setInterruptingSessionId(sessionId);
    const feedbackUntil = Date.now() + STOPPING_FEEDBACK_MS;
    setInterruptingFeedbackUntil(feedbackUntil);
    window.setTimeout(() => {
      setInterruptingFeedbackUntil((current) => (current === feedbackUntil ? 0 : current));
    }, STOPPING_FEEDBACK_MS + 100);
    try {
      await api.interrupt(sessionId);
      fastPollUntilRef.current = Date.now() + 4000;
      pushToast("Stop signal sent");
      if (selectedSessionRef.current === sessionId) schedulePoll(0);
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to interrupt session");
      setInterruptingSessionId((current) => (current === sessionId ? "" : current));
      setInterruptingFeedbackUntil(0);
      return;
    }
  }

  async function handleLogin() {
    try {
      await api.login(loginPassword);
      setErrorText("");
      setAuthState("loading");
      setLoadingText("Loading workspace…");
      await refreshSessions({ preserveSelection: false });
      setAuthState("ready");
    } catch (error) {
      setAuthState("login");
      setErrorText(error instanceof Error ? error.message : "Unable to login");
    }
  }

  async function handleLogout() {
    await api.logout();
    setAuthState("login");
    setSessions([]);
    sessionViewCacheRef.current = {};
    selectSession("");
    setTranscript([]);
  }

  function openRenameDialog(targetSession = selectedSession) {
    if (!targetSession) return;
    setRenameSessionId(targetSession.session_id);
    setRenameName(String(targetSession.alias || sessionDisplayName(targetSession) || ""));
    setRenameError("");
    setSessionContextMenu(null);
    setRenameOpen(true);
  }

  async function handleSessionMarker(session: SessionSummary, nextState: SessionMarkerState) {
    if (!session || sessionMarkerBusyId === session.session_id) return;
    const priorityOffset = nextState === "star" ? IMPORTANT_PRIORITY_OFFSET : nextState === "pending" ? PENDING_PRIORITY_OFFSET : 0;
    const nextSnoozeUntil = nextState === "snooze" ? Math.floor(Date.now() / 1000) + SHELVE_FOR_NOW_SECONDS : null;
    const workspaceKey = workspaceKeyForSession(session);
    const currentWorkspace = groupedSessions.find((group) => group.key === workspaceKey) || null;
    const workspaceHadImportantBefore = currentWorkspace ? currentWorkspace.sessions.some((item) => sessionIsImportant(item)) : false;
    const starredWorkspaceKeys = groupedSessions
      .filter((group) => group.key !== workspaceKey && group.sessions.some((item) => sessionIsImportant(item)))
      .map((group) => group.key);
    const starredSessionIds = currentWorkspace
      ? currentWorkspace.sessions.filter((item) => item.session_id !== session.session_id && sessionIsImportant(item)).map((item) => item.session_id)
      : [];
    setSessionContextMenu(null);
    setSessionMarkerBusyId(session.session_id);
    try {
      const result = await api.editSession(session.session_id, {
        name: String(session.alias || ""),
        priority_offset: priorityOffset,
        snooze_until: nextSnoozeUntil,
        dependency_session_id: session.dependency_session_id ?? null,
      });
      const resultSnoozeUntil = result.snooze_until ?? null;
      const nextSessions = sessions.map((item) =>
        item.session_id === session.session_id
          ? {
              ...item,
              alias: String(result.alias || ""),
              priority_offset: Number(result.priority_offset || 0),
              snooze_until: resultSnoozeUntil,
              dependency_session_id: result.dependency_session_id ?? null,
              blocked: Boolean(result.dependency_session_id),
              snoozed: Boolean(resultSnoozeUntil && resultSnoozeUntil > Date.now() / 1000),
            }
          : item,
      );
      const nextWorkspaceKeys = uniqueStringsInOrder(nextSessions.map((item) => workspaceKeyForSession(item)));
      const nextImportantWorkspaceKeySet = new Set(
        nextSessions.filter((item) => sessionIsImportant(item) && !sessionIsShelved(item)).map((item) => workspaceKeyForSession(item)),
      );
      const nextShelvedWorkspaceKeySet = new Set(
        nextWorkspaceKeys.filter((key) => {
          const workspaceSessions = nextSessions.filter((item) => workspaceKeyForSession(item) === key);
          return workspaceSessions.length > 0 && workspaceSessions.every((item) => sessionIsShelved(item));
        }),
      );
      setSessions(nextSessions);
      if (nextState === "star") {
        if (!workspaceHadImportantBefore) {
          setWorkspaceOrder((current) => insertOrderedKeyAfterAnchors(current, workspaceKey, starredWorkspaceKeys));
        }
        setSessionOrderByWorkspace((current) => ({
          ...current,
          [workspaceKey]: insertOrderedKeyAfterAnchors(
            currentWorkspace?.sessions.map((item) => item.session_id) || current[workspaceKey] || [session.session_id],
            session.session_id,
            starredSessionIds,
          ),
        }));
      }
      setWorkspaceOrder((current) => {
        const normalized = uniqueStringsInOrder([
          ...current.filter((key) => nextWorkspaceKeys.includes(key)),
          ...nextWorkspaceKeys,
        ]);
        return partitionSidebarOrder(
          normalized,
          (key) => nextImportantWorkspaceKeySet.has(key),
          (key) => nextShelvedWorkspaceKeySet.has(key),
        );
      });
      await refreshSessions({ preserveSelection: true });
      pushToast(`Session marker: ${sessionMarkerLabel(nextState)}`);
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to update session marker");
    } finally {
      setSessionMarkerBusyId("");
    }
  }

  async function handleRenameSession() {
    const sessionId = renameSessionId || selectedSessionRef.current;
    if (!sessionId) return;
    setRenameBusy(true);
    try {
      const name = String(renameName || "").trim();
      const result = await api.renameSession(sessionId, name);
      setSessions((current) =>
        current.map((session) => (session.session_id === sessionId ? { ...session, alias: String(result.alias || "") } : session)),
      );
      await refreshSessions({ preserveSelection: true });
      setRenameOpen(false);
      setRenameSessionId("");
      pushToast(name ? "Session renamed" : "Session name cleared");
    } catch (error) {
      setRenameError(error instanceof Error ? error.message : "Unable to rename session");
    } finally {
      setRenameBusy(false);
    }
  }

  async function loadResumeCandidates(cwd: string, backend: "codex" | "pi") {
    const trimmed = String(cwd || "").trim();
    if (!trimmed) {
      setNewSessionResumeCandidates([]);
      setNewSessionResumeSelection(null);
      setNewSessionError("");
      return;
    }
    try {
      const data = await api.fetchResumeCandidates(trimmed, backend);
      setNewSessionResumeCandidates(Array.isArray(data.sessions) ? data.sessions : []);
      setNewSessionResumeSelection((current) => {
        if (!current) return null;
        return (data.sessions || []).find((item) => item.session_id === current.session_id) || null;
      });
      setNewSessionError("");
    } catch (error) {
      setNewSessionResumeCandidates([]);
      setNewSessionResumeSelection(null);
      setNewSessionError(error instanceof Error ? error.message : "Unable to inspect cwd");
    }
  }

  async function loadCwdSuggestions(query: string) {
    const requestId = newSessionCwdRequestRef.current + 1;
    newSessionCwdRequestRef.current = requestId;
    setNewSessionCwdSuggestionsLoading(true);
    try {
      const data = await api.fetchCwdSuggestions(query, 12);
      if (requestId !== newSessionCwdRequestRef.current) return;
      const suggestions = Array.isArray(data.suggestions)
        ? data.suggestions.filter(
            (item): item is CwdSuggestion =>
              Boolean(item) && typeof item.value === "string" && item.value.trim().length > 0 && typeof item.label === "string",
          )
        : [];
      setNewSessionCwdSuggestions(suggestions);
      setNewSessionCwdSuggestionIndex(suggestions.length ? 0 : -1);
      setNewSessionCwdSuggestionError("");
    } catch (error) {
      if (requestId !== newSessionCwdRequestRef.current) return;
      setNewSessionCwdSuggestions([]);
      setNewSessionCwdSuggestionIndex(-1);
      setNewSessionCwdSuggestionError(error instanceof Error ? error.message : "Unable to load folders");
    } finally {
      if (requestId === newSessionCwdRequestRef.current) setNewSessionCwdSuggestionsLoading(false);
    }
  }

  function applyNewSessionCwdSuggestion(suggestion: CwdSuggestion) {
    setNewSessionCwd(suggestion.value);
    setNewSessionCwdSuggestionsOpen(true);
    setNewSessionCwdSuggestionIndex(0);
    window.setTimeout(() => newSessionCwdInputRef.current?.focus(), 0);
  }

  function handleNewSessionCwdKeyDown(event: KeyboardEvent) {
    if (!newSessionCwdSuggestionChoices.length) {
      if (event.key === "Escape") setNewSessionCwdSuggestionsOpen(false);
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setNewSessionCwdSuggestionsOpen(true);
      setNewSessionCwdSuggestionIndex((current) => (current + 1) % newSessionCwdSuggestionChoices.length);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setNewSessionCwdSuggestionsOpen(true);
      setNewSessionCwdSuggestionIndex((current) => (current <= 0 ? newSessionCwdSuggestionChoices.length - 1 : current - 1));
      return;
    }
    if ((event.key === "Enter" || event.key === "Tab") && newSessionCwdSuggestionsOpen && newSessionCwdSuggestionIndex >= 0) {
      event.preventDefault();
      applyNewSessionCwdSuggestion(newSessionCwdSuggestionChoices[newSessionCwdSuggestionIndex]);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      setNewSessionCwdSuggestionsOpen(false);
    }
  }

  function openNewSessionDialog() {
    const preferences = readStoredNewSessionPreferences();
    const backend = preferences.backend || newSessionBackend;
    const values = newSessionValuesForBackend(newSessionDefaults, backend, preferences, tmuxAvailable);
    setNewSessionPreferences(preferences);
    setNewSessionBackend(backend);
    setNewSessionCwd(selectedSession?.cwd || recentCwds[0] || "");
    setNewSessionCwdSuggestions([]);
    setNewSessionCwdSuggestionsOpen(false);
    setNewSessionCwdSuggestionIndex(-1);
    setNewSessionCwdSuggestionError("");
    setNewSessionProvider(values.provider);
    setNewSessionModel(values.model);
    setNewSessionReasoning(values.reasoning);
    setNewSessionFast(values.fast);
    setNewSessionTmux(values.tmux);
    setNewSessionResumeSelection(null);
    setNewSessionResumeCandidates([]);
    setNewSessionWorktree(false);
    setNewSessionWorktreeBranch("");
    setNewSessionError("");
    setNewSessionOpen(true);
  }

  async function followCreatedSession(brokerPid: number) {
    for (let attempt = 0; attempt < 24; attempt += 1) {
      const payload = await api.fetchSessions();
      const ordered = sortSessions(payload.sessions || []);
      setSessions(ordered);
      const found = ordered.find((item) => Number(item.broker_pid || 0) === Number(brokerPid || 0));
      if (found) {
        promoteOpenedSession(found, sessions, ordered);
        selectSession(found.session_id);
        pushToast("Session started");
        focusComposerInput();
        return;
      }
      await sleep(250);
    }
    await refreshSessions({ preserveSelection: true });
    pushToast("Session started; waiting for first log write");
    focusComposerInput();
  }

  async function handleCreateSession() {
    if (!newSessionDefaults) return;
    const cwd = String(newSessionCwd || "").trim();
    if (!cwd) {
      setNewSessionError("cwd required");
      return;
    }
    const backend = newSessionBackend;
    const defaults = defaultsForBackend(newSessionDefaults, backend);
    const providerChoice = String(newSessionProvider || defaults?.provider_choice || "").trim();
    const providerSettings = providerChoiceToSettings(providerChoice, backend);
    const model = String(newSessionModel || "").trim() || null;
    const reasoningEffort = String(newSessionReasoning || defaults?.reasoning_effort || "high").trim().toLowerCase();
    const createInTmux = tmuxAvailable && newSessionTmux;
    const worktreeBranch =
      newSessionWorktree && !newSessionResumeSelection
        ? (String(newSessionWorktreeBranch || "").trim() || makeWorktreeSlug(baseName(cwd)))
        : null;
    setNewSessionBusy(true);
    try {
      const result = await api.createSession({
        cwd,
        agent_backend: backend,
        model_provider: providerSettings.model_provider,
        preferred_auth_method: providerSettings.preferred_auth_method,
        model,
        reasoning_effort: reasoningEffort,
        service_tier: backend === "codex" && newSessionFast ? "fast" : null,
        create_in_tmux: createInTmux,
        resume_session_id: newSessionResumeSelection?.session_id || null,
        worktree_branch: worktreeBranch,
      });
      setNewSessionPreferences((current) => ({
        ...current,
        backend,
        tmux: tmuxAvailable ? createInTmux : current.tmux,
        backends: {
          ...(current.backends || {}),
          [backend]: {
            provider: providerChoice,
            model: String(newSessionModel || "").trim(),
            reasoning: reasoningEffort,
            fast: Boolean(newSessionFast),
          },
        },
      }));
      pushToast("Session starting…");
      setNewSessionOpen(false);
      void followCreatedSession(result.broker_pid).catch((error) => {
        setErrorText(error instanceof Error ? error.message : "Unable to refresh started session");
      });
    } catch (error) {
      setNewSessionError(error instanceof Error ? error.message : "Unable to start session");
    } finally {
      setNewSessionBusy(false);
    }
  }

  async function loadVoiceAndNotifications() {
    setVoiceSettingsLoading(true);
    setSettingsLoadError("");
    try {
      const [voiceResult, notificationsResult, configResult] = await Promise.allSettled([
        api.fetchVoiceSettings(),
        api.fetchNotificationSubscriptions(),
        api.fetchCodexConfig(),
      ]);
      const errors: string[] = [];
      let loadedVoice: VoiceSettingsResponse | null = null;
      let loadedNotifications: NotificationSubscriptionsResponse | null = null;

      if (voiceResult.status === "fulfilled") {
        loadedVoice = voiceResult.value;
        setVoiceSettings(loadedVoice);
        setVoiceBaseUrl(String(loadedVoice.tts_base_url || ""));
        setVoiceApiKey(String(loadedVoice.tts_api_key || ""));
        setVoiceNarrationEnabled(Boolean(loadedVoice.tts_enabled_for_narration));
      } else {
        errors.push(voiceResult.reason instanceof Error ? voiceResult.reason.message : "Unable to load voice settings");
      }

      if (notificationsResult.status === "fulfilled") {
        loadedNotifications = notificationsResult.value;
        setNotificationSnapshot(loadedNotifications);
      } else {
        errors.push(notificationsResult.reason instanceof Error ? notificationsResult.reason.message : "Unable to load notification settings");
      }

      if (configResult.status === "fulfilled") {
        setCodexConfig(configResult.value);
        setCodexConfigText(String(configResult.value.text || ""));
      } else {
        errors.push(configResult.reason instanceof Error ? configResult.reason.message : "Unable to load config.toml");
      }

      if (loadedVoice && loadedNotifications) {
        void syncMobilePushState(loadedNotifications, loadedVoice.notifications.vapid_public_key);
      }
      setSettingsLoadError(errors.join(" · "));
      setErrorText(errors.length ? errors[0] : "");
    } finally {
      setVoiceSettingsLoading(false);
    }
  }

  async function reloadCodexConfig() {
    try {
      const config = await api.fetchCodexConfig();
      setCodexConfig(config);
      setCodexConfigText(String(config.text || ""));
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to reload config.toml");
    }
  }

  async function saveVoiceAndNotifications() {
    setVoiceSettingsSaving(true);
    try {
      const saved = await api.saveVoiceSettings({
        tts_enabled_for_narration: voiceNarrationEnabled,
        tts_enabled_for_final_response: true,
        tts_base_url: voiceBaseUrl.trim(),
        tts_api_key: voiceApiKey.trim(),
      });
      setVoiceSettings(saved);
      pushToast("Voice settings saved");
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to save voice settings");
    } finally {
      setVoiceSettingsSaving(false);
    }
  }

  async function saveCodexConfig() {
    setCodexConfigSaving(true);
    try {
      const saved = await api.saveCodexConfig(codexConfigText);
      setCodexConfig(saved);
      setCodexConfigText(String(saved.text || ""));
      pushToast("config.toml saved");
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to save config.toml");
    } finally {
      setCodexConfigSaving(false);
    }
  }

  async function waitForRestartedService(previousServerPid: number) {
    const deadline = Date.now() + 45000;
    while (Date.now() < deadline) {
      await new Promise((resolve) => window.setTimeout(resolve, 1000));
      try {
        const status = await api.me();
        if (status.server_pid !== previousServerPid) {
          window.location.reload();
          return;
        }
      } catch {
        // Ignore transient fetch failures while the daemon is restarting.
      }
    }
    setServiceRestarting(false);
    setErrorText("Service restart was triggered, but the UI could not confirm recovery yet. Refresh this page in a few seconds.");
  }

  async function restartLocalService() {
    setServiceRestarting(true);
    setErrorText("");
    try {
      const restart = await api.restartService();
      pushToast("Service restarting… the page will reload when the new server is ready");
      void waitForRestartedService(restart.server_pid);
    } catch (error) {
      setServiceRestarting(false);
      setErrorText(error instanceof Error ? error.message : "Unable to restart service");
    }
  }

  async function syncMobilePushState(snapshot = notificationSnapshot, publicKey = voiceSettings?.notifications.vapid_public_key || snapshot?.vapid_public_key || "") {
    if (notificationDeviceClass() !== "mobile" || !("serviceWorker" in navigator) || !("PushManager" in window)) {
      setMobilePushEndpoint("");
      setMobilePushEnabled(false);
      return;
    }
    try {
      const registration = await ensureVoiceServiceWorker();
      const subscription = await registration.pushManager.getSubscription();
      const endpoint = subscription?.endpoint || "";
      setMobilePushEndpoint(endpoint);
      const current = endpoint
        ? (snapshot?.subscriptions || []).find((item) => item && typeof item.endpoint === "string" && item.endpoint === endpoint)
        : null;
      setMobilePushEnabled(Boolean(current && current.notifications_enabled));
      if (!endpoint && !publicKey) setMobilePushEnabled(false);
    } catch {
      setMobilePushEndpoint("");
      setMobilePushEnabled(false);
    }
  }

  async function setMobilePush(nextEnabled: boolean) {
    if (typeof Notification === "undefined") throw new Error("notifications unsupported");
    if (nextEnabled && Notification.permission !== "granted") {
      const permission = await Notification.requestPermission();
      if (permission !== "granted") throw new Error(`notification permission ${permission}`);
    }
    if (!("serviceWorker" in navigator) || !("PushManager" in window)) throw new Error("mobile push unsupported");
    const publicKey = voiceSettings?.notifications.vapid_public_key || notificationSnapshot?.vapid_public_key || "";
    const registration = await ensureVoiceServiceWorker();
    let subscription = await registration.pushManager.getSubscription();
    if (nextEnabled && !publicKey) throw new Error("missing VAPID public key");
    if (!subscription && nextEnabled) {
      subscription = await registration.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: base64UrlToUint8Array(publicKey),
      });
    }
    if (!subscription) {
      setMobilePushEnabled(false);
      return;
    }
    const snapshot = mobilePushEndpoint
      ? await api.toggleNotificationSubscription(subscription.endpoint, nextEnabled)
      : await api.upsertNotificationSubscription({
          subscription: subscription.toJSON(),
          user_agent: navigator.userAgent,
          device_label: "current-device",
          device_class: notificationDeviceClass(),
        });
    const finalSnapshot = mobilePushEndpoint ? snapshot : await api.toggleNotificationSubscription(subscription.endpoint, nextEnabled);
    setNotificationSnapshot(finalSnapshot);
    await syncMobilePushState(finalSnapshot, publicKey);
    pushToast(nextEnabled ? "Mobile push on" : "Mobile push off");
  }

  async function handleAttachFile(file: File | null | undefined) {
    const sessionId = selectedSessionRef.current;
    if (!sessionId || !file || attachBusy) return;
    if (file.size > ATTACH_UPLOAD_MAX_BYTES) {
      setErrorText(`file too large (max ${Math.round(ATTACH_UPLOAD_MAX_BYTES / 1024 / 1024)} MB)`);
      return;
    }
    setAttachBusy(true);
    try {
      const data_b64 = await fileToBase64(file);
      const result = await api.injectFile(sessionId, {
        filename: file.name || "file",
        data_b64,
        attachment_index: attachedFiles + 1,
      });
      setAttachedFiles((current) => current + 1);
      pushToast(`Attached ${file.name || result.path}`);
      fastPollUntilRef.current = Date.now() + 4000;
      schedulePoll(0);
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to attach file");
    } finally {
      setAttachBusy(false);
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  }

  async function maybeNotifyDesktop(item: Record<string, unknown>) {
    if (!desktopNotificationsEnabled) return;
    if (typeof Notification === "undefined" || Notification.permission !== "granted") return;
    const messageId = typeof item.message_id === "string" ? item.message_id : "";
    if (messageId && shownNotificationIdsRef.current.has(messageId)) return;
    const title = typeof item.session_display_name === "string" && item.session_display_name.trim() ? item.session_display_name : "Codoxear";
    const body = typeof item.notification_text === "string" ? item.notification_text.trim() : "";
    if (!body) return;
    if (messageId) shownNotificationIdsRef.current.add(messageId);
    new Notification(title, { body, tag: messageId || `desktop-${Date.now()}` });
  }

  async function pollNotificationFeed() {
    if (!desktopNotificationsEnabled) return;
    try {
      const response = await api.fetchNotificationFeed(notificationFeedSinceRef.current);
      const items = Array.isArray(response.items) ? response.items : [];
      let nextSince = notificationFeedSinceRef.current;
      for (const item of items) {
        const updatedTs = Number((item && item.updated_ts) || 0);
        if (Number.isFinite(updatedTs) && updatedTs > nextSince) nextSince = updatedTs;
        void maybeNotifyDesktop(item);
      }
      notificationFeedSinceRef.current = nextSince;
    } catch {
      // keep polling; server-side notification support is best-effort here
    }
  }

  useEffect(() => {
    selectedSessionRef.current = selectedSessionId;
    if (!selectedSessionId) return;
    setAttachedFiles(0);
    writeLocalStorage(SELECTED_SESSION_KEY, selectedSessionId);
    writeSessionHash(selectedSessionId);
  }, [selectedSessionId]);

  useEffect(() => {
    writeLocalStorage(SHOW_TOOL_CALLS_KEY, showTools ? "1" : "0");
  }, [showTools]);

  useEffect(() => {
    writeLocalStorage(BUSY_SUBMIT_MODE_KEY, busySubmitMode);
  }, [busySubmitMode]);

  useEffect(() => {
    document.documentElement.dataset.theme = themeMode;
    writeLocalStorage(THEME_MODE_KEY, themeMode === "light" ? "light" : null);
  }, [themeMode]);

  useEffect(() => {
    writeLocalStorage(COMPACT_SESSION_CARDS_KEY, compactSessionCards ? "1" : null);
  }, [compactSessionCards]);

  useEffect(() => {
    const syncViewport = () => setMobileViewport(isMobileViewportWidth());
    syncViewport();
    window.addEventListener("resize", syncViewport);
    return () => window.removeEventListener("resize", syncViewport);
  }, []);

  useEffect(() => {
    if (!mobileViewport) setMobileSidebarOpen(false);
  }, [mobileViewport]);

  useEffect(() => {
    if (!mobileSidebarOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeMobileSidebar();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [mobileSidebarOpen]);

  useEffect(() => {
    writeLocalStorage(SIDEBAR_WORKSPACE_ORDER_KEY, JSON.stringify(workspaceOrder));
  }, [workspaceOrder]);

  useEffect(() => {
    writeLocalStorage(SIDEBAR_SESSION_ORDER_KEY, JSON.stringify(sessionOrderByWorkspace));
  }, [sessionOrderByWorkspace]);

  useEffect(() => {
    writeLocalStorage(SESSION_DRAFTS_KEY, Object.keys(sessionDrafts).length ? JSON.stringify(sessionDrafts) : null);
  }, [sessionDrafts]);

  useEffect(() => {
    writeLocalStorage(NEW_SESSION_PREFERENCES_KEY, JSON.stringify(newSessionPreferences));
  }, [newSessionPreferences]);

  useEffect(() => {
    writeLocalStorage(SIDEBAR_WIDTH_KEY, sidebarWidth == null ? null : String(Math.round(sidebarWidth)));
  }, [sidebarWidth]);

  useEffect(() => {
    writeLocalStorage(DETAIL_RAIL_WIDTH_KEY, String(Math.round(detailRailWidth)));
  }, [detailRailWidth]);

  useEffect(() => {
    writeLocalStorage(DESKTOP_NOTIFICATIONS_KEY, desktopNotificationsEnabled ? "1" : null);
  }, [desktopNotificationsEnabled]);

  useEffect(() => {
    if (!sidebarResizing) return;
    const onPointerMove = (event: PointerEvent) => {
      const delta = event.clientX - sidebarResizeStartXRef.current;
      setSidebarWidth(clampSidebarWidth(sidebarResizeStartWidthRef.current + delta, currentSidebarLayoutWidth()));
    };
    const onPointerUp = () => setSidebarResizing(false);
    const { style } = document.body;
    const prevCursor = style.cursor;
    const prevUserSelect = style.userSelect;
    style.cursor = "col-resize";
    style.userSelect = "none";
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      style.cursor = prevCursor;
      style.userSelect = prevUserSelect;
    };
  }, [sidebarResizing]);

  useEffect(() => {
    if (sidebarWidth == null) return;
    const onResize = () => setSidebarWidth((current) => (current == null ? null : clampSidebarWidth(current, currentSidebarLayoutWidth())));
    onResize();
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [showDetailsPanel, showFilesPanel, sidebarWidth]);

  useEffect(() => {
    if (!detailRailResizing) return;
    const onPointerMove = (event: PointerEvent) => {
      const delta = detailRailResizeStartXRef.current - event.clientX;
      setDetailRailWidth(clampDetailRailWidth(detailRailResizeStartWidthRef.current + delta));
    };
    const onPointerUp = () => setDetailRailResizing(false);
    const { style } = document.body;
    const prevCursor = style.cursor;
    const prevUserSelect = style.userSelect;
    style.cursor = "col-resize";
    style.userSelect = "none";
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      style.cursor = prevCursor;
      style.userSelect = prevUserSelect;
    };
  }, [detailRailResizing]);

  useEffect(() => {
    const onResize = () => setDetailRailWidth((current) => clampDetailRailWidth(current));
    onResize();
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  useEffect(() => {
    if (!selectedSessionId || queueLen <= 0) {
      setQueueItems([]);
      setQueueDrafts({});
      return;
    }
    void loadQueue(selectedSessionId);
  }, [selectedSessionId, queueLen]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        await api.me();
        if (cancelled) return;
        await refreshSessions({ preserveSelection: false });
        if (cancelled) return;
        setAuthState("ready");
      } catch (error) {
        if (cancelled) return;
        const status = error && typeof error === "object" && "status" in error ? Number((error as { status?: number }).status) : 0;
        setAuthState(status === 401 ? "login" : "loading");
        setLoadingText(status === 401 ? "Authentication required" : "Unable to load workspace");
        setErrorText(status === 401 ? "" : error instanceof Error ? error.message : "Unable to load workspace");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (authState !== "ready" || !selectedSessionId) return;
    setDiagnostics(null);
    setFileEntries([]);
    setFileSearchQuery("");
    setFileSearchEntries([]);
    setFileOpenLoading(false);
    setFileOpenError("");
    activeFilePathRef.current = "";
    filesLoadedSessionRef.current = "";
    setActiveFile(null);
    setActiveFilePath("");
    setCollapsedEvents({});
    void openSession(selectedSessionId);
  }, [authState, selectedSessionId]);

  useEffect(() => {
    const lastEvent = visibleTranscript[visibleTranscript.length - 1] || null;
    const scrollKey = [
      selectedSessionId,
      visibleTranscript.length,
      lastEvent?.id || "",
      lastEvent?.body.length || 0,
      lastEvent?.meta.length || 0,
      workingIndicatorLabel,
    ].join(":");
    if (!scrollKey || scrollKey === lastAutoScrollKeyRef.current) return;
    lastAutoScrollKeyRef.current = scrollKey;
    if ((!visibleTranscript.length && !workingIndicatorLabel) || !stickToBottomRef.current) return;
    window.requestAnimationFrame(() => {
      const element = chatScrollRef.current;
      if (!element) return;
      element.scrollTop = element.scrollHeight;
      stickToBottomRef.current = true;
      setHistoryTopBoundaryReached(isNearHistoryTop(element));
    });
  }, [selectedSessionId, visibleTranscript, workingIndicatorLabel]);

  useEffect(() => {
    const raf = window.requestAnimationFrame(() => {
      updateChatScrollFollowState();
    });
    return () => {
      window.cancelAnimationFrame(raf);
    };
  }, [selectedSessionId, hasOlder, visibleTranscript.length, loadingOlder, workingIndicatorLabel]);

  useEffect(() => {
    if (authState !== "ready") return;
    sessionRefreshTimerRef.current = window.setInterval(() => {
      void refreshSessions();
    }, SESSION_REFRESH_MS);
    return () => {
      if (sessionRefreshTimerRef.current !== null) window.clearInterval(sessionRefreshTimerRef.current);
    };
  }, [authState]);

  useEffect(() => {
    if (authState !== "ready") return;
    void refreshVersionStatus();
    const timer = window.setInterval(() => {
      void refreshVersionStatus();
    }, VERSION_STATUS_POLL_MS);
    return () => window.clearInterval(timer);
  }, [authState]);

  useEffect(() => {
    if (!showDetailsPanel || !selectedSessionId) return;
    void loadDiagnostics(selectedSessionId);
  }, [showDetailsPanel, selectedSessionId]);

  useEffect(() => {
    if (!showFilesPanel || !selectedSessionId) return;
    void loadFiles(selectedSessionId, { showLoading: filesLoadedSessionRef.current !== selectedSessionId });
  }, [showFilesPanel, selectedSessionId, sessions]);

  useEffect(() => {
    if (!newSessionOpen) return;
    const timer = window.setTimeout(() => {
      void loadCwdSuggestions(newSessionCwd);
    }, 120);
    return () => window.clearTimeout(timer);
  }, [newSessionOpen, newSessionCwd]);

  useEffect(() => {
    if (!newSessionOpen) return;
    const timer = window.setTimeout(() => {
      void loadResumeCandidates(newSessionCwd, newSessionBackend);
    }, 180);
    return () => window.clearTimeout(timer);
  }, [newSessionOpen, newSessionCwd, newSessionBackend]);

  useEffect(() => {
    if (!settingsOpen || authState !== "ready") return;
    void loadVoiceAndNotifications();
  }, [settingsOpen, authState]);

  useEffect(() => {
    if (authState !== "ready" || !desktopNotificationsEnabled) return;
    void pollNotificationFeed();
    notificationPollTimerRef.current = window.setInterval(() => {
      void pollNotificationFeed();
    }, NOTIFICATION_POLL_MS);
    return () => {
      if (notificationPollTimerRef.current !== null) window.clearInterval(notificationPollTimerRef.current);
    };
  }, [authState, desktopNotificationsEnabled]);

  useEffect(() => {
    return () => {
      if (pollTimerRef.current !== null) window.clearTimeout(pollTimerRef.current);
      if (sessionRefreshTimerRef.current !== null) window.clearInterval(sessionRefreshTimerRef.current);
      if (notificationPollTimerRef.current !== null) window.clearInterval(notificationPollTimerRef.current);
    };
  }, []);

  if (shareMode) {
    if (shareAuthState === "loading") return <LoadingScreen text="Loading shared sessions…" />;
    if (shareAuthState === "login" || !shareSet) {
      return (
        <ShareLoginScreen
          label={shareLabel}
          password={sharePassword}
          errorText={sharePasswordError}
          onPasswordChange={setSharePassword}
          onSubmit={handleShareLogin}
        />
      );
    }
    return (
      <ShareWorkspace
        share={shareSet}
        sessionId={shareSessionId}
        transcript={shareTranscript}
        files={shareFiles}
        selectedFilePath={shareSelectedFilePath}
        selectedFile={shareSelectedFile}
        loading={shareLoading}
        loadingOlder={shareLoadingOlder}
        hasOlder={shareHasOlder}
        busy={shareBusy}
        queueLen={shareQueueLen}
        errorText={shareErrorText}
        sendText={shareSendText}
        canSend={Boolean(shareSessionId && shareSendText.trim())}
        onSessionChange={(sessionId) => {
          setShareSessionId(sessionId);
          void loadShareSession(sessionId);
          history.replaceState(null, "", `/share/${shareSet.share_id}/sessions/${sessionId}/`);
        }}
        onSendTextChange={setShareSendText}
        onSend={handleShareSend}
        onInterrupt={handleShareInterrupt}
        onLoadOlder={loadShareOlder}
        onSelectFile={(path) => void selectShareFile(path)}
        onCopyText={copyTranscriptEvent}
      />
    );
  }

  if (authState === "loading") return <LoadingScreen text={loadingText} />;
  if (authState === "login") {
    return (
      <LoginScreen
        password={loginPassword}
        errorText={errorText}
        onPasswordChange={setLoginPassword}
        onSubmit={handleLogin}
      />
    );
  }
  const contextMenuMarkerState = sessionMarkerState(contextMenuSession);

  return (
    <>
      <div
        ref={appRef}
        className={`app${showFilesPanel || showDetailsPanel ? " withRail" : ""}${sidebarCollapsed ? " sidebarCollapsed" : ""}${sidebarResizing ? " sidebarResizing" : ""}${detailRailResizing ? " detailRailResizing" : ""}${mobileViewport ? " mobileLayout" : ""}${mobileSidebarOpen ? " mobileSidebarOpen" : ""}${compactSessionCards ? " compactSessionCards" : ""}`}
        style={appStyle}
        onClick={() => setSessionContextMenu(null)}
      >
        <aside className="sidebar" ref={sidebarRef} id="nova-sidebar">
          <header>
            <div className="title">
              <span className="sidebarLogoDot" />
              <span>Codoxear Nova</span>
              {versionStatus?.update_available ? (
                <span className="updateBadge" title={updateBadgeTitle} aria-label={updateBadgeTitle}>
                  NEW
                </span>
              ) : null}
            </div>
            <div className="actions">
              {mobileViewport ? (
                <button className="icon-btn mobileSidebarCloseBtn" type="button" title="Close sidebar" aria-label="Close sidebar" onClick={closeMobileSidebar}>
                  {icon("close")}
                </button>
              ) : null}
              <button
                className="icon-btn"
                type="button"
                title="New session"
                onClick={() => {
                  closeMobileSidebar();
                  openNewSessionDialog();
                }}
              >
                {icon("plus")}
              </button>
              <button
                className={`icon-btn${desktopNotificationsEnabled ? " active" : ""}`}
                type="button"
                title={desktopNotificationsEnabled ? "Desktop notifications on" : "Desktop notifications off"}
                onClick={async () => {
                  if (typeof Notification === "undefined") {
                    pushToast("Notifications unsupported in this browser");
                    return;
                  }
                  if (!desktopNotificationsEnabled && Notification.permission !== "granted") {
                    const permission = await Notification.requestPermission();
                    if (permission !== "granted") {
                      pushToast(`Notification permission ${permission}`);
                      return;
                    }
                  }
                  setDesktopNotificationsEnabled((current) => !current);
                  pushToast(desktopNotificationsEnabled ? "Desktop notifications off" : "Desktop notifications on");
                }}
              >
                {icon("bell")}
              </button>
              <button
                className="icon-btn"
                type="button"
                title="Voice settings"
                onClick={() => {
                  closeMobileSidebar();
                  setSettingsOpen(true);
                }}
              >
                {icon("volume")}
              </button>
            </div>
          </header>
          <div className="sessions">
            {workspaceGroups.map((group) => {
              const hasSelected = group.sessions.some((session) => session.session_id === selectedSessionId);
              const collapsed = Boolean(collapsedWorkspaces[group.key] && !hasSelected);
              const groupDropClass =
                sidebarDropIndicator?.kind === "workspace" && sidebarDropIndicator.key === group.key ? ` is-drop-${sidebarDropIndicator.position}` : "";
              return (
                <section
                  key={group.key}
                  className={`workspaceGroup${hasSelected ? " active" : ""}${draggingWorkspaceKey === group.key ? " is-dragging" : ""}${groupDropClass}`}
                  draggable
                  onDragStart={(event) => handleWorkspaceDragStart(event, group.key)}
                  onDragOver={(event) => handleWorkspaceDragOver(event, group.key)}
                  onDrop={(event) => handleWorkspaceDrop(event, group.key)}
                  onDragEnd={() => {
                    setDraggingWorkspaceKey("");
                    setSidebarDropIndicator(null);
                  }}
                >
                  <button className="workspaceGroupHeader" type="button" onClick={() => toggleWorkspaceGroup(group.key)} aria-expanded={!collapsed}>
                    <span className="workspaceDisclosure">{collapsed ? "▸" : "▾"}</span>
                    <span className="workspaceGroupText">
                      <span className="workspaceGroupTitle">{workspaceTitle(group.cwd)}</span>
                      <span className="workspaceGroupPath">{group.cwd === "__unknown_workspace__" ? "No cwd" : group.cwd}</span>
                    </span>
                    <span className="workspaceGroupMeta">
                      {group.sessions.length} {group.sessions.length === 1 ? "session" : "sessions"}
                      {group.busyCount ? ` · ${group.busyCount} running` : ""}
                      {group.queueLen ? ` · q${group.queueLen}` : ""}
                    </span>
                  </button>
                  {!collapsed ? (
                    <div className="workspaceSessions">
                      {group.sessions.map((session) => {
                        const markerState = sessionMarkerState(session);
                        const nextMarkerState = nextSessionMarkerState(markerState);
                        const importantMarked = markerState === "star";
                        const pendingMarked = markerState === "pending";
                        const shelvedMarked = markerState === "snooze";
                        const hasDraft = Boolean((sessionDrafts[session.session_id] || "").trim());
                        const markerBusy = sessionMarkerBusyId === session.session_id;
                        const sessionAwaitingReply = session.session_id === selectedSessionAwaitingReplyId;
                        const sessionStopSuppressed = session.session_id === interruptingSessionId;
                        const sessionDropClass =
                          sidebarDropIndicator?.kind === "session" && sidebarDropIndicator.key === session.session_id
                            ? ` is-drop-${sidebarDropIndicator.position}`
                            : "";
                        return (
                          <div
                            key={session.session_id}
                            className={`workspace${selectedSessionId === session.session_id ? " active" : ""}${draggingSession?.sessionId === session.session_id ? " is-dragging" : ""}${importantMarked ? " is-important" : ""}${pendingMarked ? " is-pending" : ""}${shelvedMarked ? " is-shelved" : ""}${sessionDropClass}`}
                            draggable
                            onDragStart={(event) => handleSessionDragStart(event, group.key, session.session_id)}
                            onDragOver={(event) => handleSessionDragOver(event, group.key, session.session_id)}
                            onDrop={(event) => handleSessionDrop(event, group.key, session.session_id)}
                            onDragEnd={() => {
                              setDraggingSession(null);
                              setSidebarDropIndicator(null);
                            }}
                            onContextMenu={(event) => openSessionContextMenu(event, session)}
                          >
                            <button className="workspaceSelect" type="button" onClick={() => selectSession(session.session_id)}>
                              <div className="workspaceHeader">
                                <div className="workspaceTitleRow">
                                  <div className="workspaceTitle">{sessionDisplayName(session)}</div>
                                </div>
                                <div className="workspacePath">{session.git_branch ? session.git_branch : String(session.agent_backend || "codex").toUpperCase()}</div>
                                <div className="workspaceMeta">
                                  {String(session.agent_backend || "codex").toUpperCase()} ·{" "}
                                  {sessionStatusText(session, sessionAwaitingReply, sessionStopSuppressed)} ·{" "}
                                  {relativeAge(session.updated_ts)}
                                </div>
                              </div>
                              <div className="lastLine">{sessionLastLines[session.session_id] || session.session_id}</div>
                            </button>
                            <div className="workspaceMarkers">
                              <div className="workspaceStateMarkers" role="group" aria-label="Session state markers">
                                {hasDraft ? <span className="workspaceDraftMark" title="Draft saved" aria-label="Draft saved" /> : null}
                                <span
                                  className={`status-dot ${
                                    sessionIsQueuedWaiting(session)
                                      ? "waiting"
                                      : sessionIsRunning(session, sessionAwaitingReply, sessionStopSuppressed)
                                        ? "running"
                                        : "idle"
                                  }`}
                                  title={sessionStatusText(session, sessionAwaitingReply, sessionStopSuppressed)}
                                  aria-label={sessionStatusText(session, sessionAwaitingReply, sessionStopSuppressed)}
                                />
                              </div>
                              <button
                                className={`workspaceMarkerBtn marker-${markerState}${markerState !== "normal" ? " active" : ""}`}
                                type="button"
                                title={`${sessionMarkerLabel(markerState)} marker. Click for ${sessionMarkerLabel(nextMarkerState)}.`}
                                aria-label={`${sessionMarkerLabel(markerState)} marker. Click for ${sessionMarkerLabel(nextMarkerState)}.`}
                                aria-pressed={markerState !== "normal"}
                                disabled={markerBusy}
                                draggable={false}
                                onPointerDown={(event) => event.stopPropagation()}
                                onClick={(event) => {
                                  event.stopPropagation();
                                  void handleSessionMarker(session, nextMarkerState);
                                }}
                              >
                                {icon(sessionMarkerIcon(markerState))}
                              </button>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  ) : null}
                </section>
              );
            })}
          </div>
          <footer>
            <button
              type="button"
              onClick={() => {
                closeMobileSidebar();
                setSettingsOpen(true);
              }}
            >
              {icon("settings")}
              Settings
            </button>
            <button
              type="button"
              onClick={() => {
                closeMobileSidebar();
                void handleLogout();
              }}
            >
              {icon("logout")}
              Log out
            </button>
          </footer>
        </aside>

        {mobileViewport && mobileSidebarOpen ? (
          <button className="sidebarBackdrop" type="button" aria-label="Close sidebar" onClick={closeMobileSidebar} />
        ) : null}

        <div
          className="sidebarResizer"
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize sidebar"
          onPointerDown={handleSidebarResizeStart}
        />

        <div className="main">
          <div className="topbar">
            <div className="pill">
              <button
                className={mobileViewport ? "actionBtn topbarSidebarBtn" : "icon-btn"}
                type="button"
                title={sidebarToggleTitle}
                aria-controls="nova-sidebar"
                aria-expanded={mobileViewport ? mobileSidebarOpen : !sidebarCollapsed}
                aria-pressed={sidebarTogglePressed}
                onClick={toggleSidebarVisibility}
              >
                {icon("menu")}
                {mobileViewport ? <span>{mobileSidebarToggleLabel}</span> : null}
              </button>
              <div className="titleWrap">
                <div className="titleRow">
                  <div id="threadTitle">{sessionDisplayName(selectedSession)}</div>
                  <div className="topMeta">
                    <span className="status-chip">{selectedSession ? baseName(workspaceKeyForSession(selectedSession)) : "No workspace"}</span>
                    <span className={topSessionStatusClass}>{topSessionStatus}</span>
                    {tokenSummary ? <span className="status-chip" title={tokenSummary.title}>{tokenSummary.label}</span> : null}
                  </div>
                </div>
              </div>
            </div>
            <div className="actions topActions">
              <button
                className={mobileViewport ? "actionBtn mobileActionBtn" : "icon-btn"}
                type="button"
                title={selectedSession ? "Rename session" : "No session selected"}
                disabled={!selectedSession}
                onClick={() => openRenameDialog()}
              >
                {icon("edit")}
                {mobileViewport ? <span>Rename</span> : null}
              </button>
              <button
                className={mobileViewport ? `actionBtn mobileActionBtn${showTools ? " active" : ""}` : `icon-btn${showTools ? " active" : ""}`}
                type="button"
                title={showTools ? "Hide tools and narration" : "Show tools and narration"}
                onClick={() => setShowTools((current) => !current)}
              >
                {icon("wrench")}
                {mobileViewport ? <span>Tools</span> : null}
              </button>
              <button
                className={mobileViewport ? "actionBtn mobileActionBtn danger" : "icon-btn danger"}
                type="button"
                title={selectedSession ? "Close session" : "No session selected"}
                disabled={!selectedSession || closingSession}
                onClick={() => void handleCloseSession()}
              >
                {icon("trash")}
                {mobileViewport ? <span>Close</span> : null}
              </button>
              <button
                className={mobileViewport ? "actionBtn mobileActionBtn" : "icon-btn"}
                type="button"
                title={selectedSession ? "Share session set" : "No session selected"}
                disabled={!selectedSession}
                onClick={openShareCreateDialog}
              >
                {icon("share")}
                {mobileViewport ? <span>Share</span> : null}
              </button>
              <button
                className={mobileViewport ? `actionBtn mobileActionBtn${tmuxCommand ? " active" : ""}` : `icon-btn${tmuxCommand ? " active" : ""}`}
                type="button"
                title={tmuxCommand || "No tmux attach command for this session"}
                disabled={!tmuxCommand}
                onClick={() => void copyTmuxAttachCommand()}
              >
                {icon("terminal")}
                {mobileViewport ? <span>Tmux</span> : null}
              </button>
              <button
                className={mobileViewport ? `actionBtn mobileActionBtn${showFilesPanel ? " active" : ""}` : `icon-btn${showFilesPanel ? " active" : ""}`}
                type="button"
                title="Files"
                onClick={() => setShowFilesPanel((current) => !current)}
              >
                {icon("file")}
                {mobileViewport ? <span>Files</span> : null}
              </button>
              <button
                className={mobileViewport ? `actionBtn mobileActionBtn${showDetailsPanel ? " active" : ""}` : `icon-btn${showDetailsPanel ? " active" : ""}`}
                type="button"
                title="Details"
                onClick={() => setShowDetailsPanel((current) => !current)}
              >
                {icon("info")}
                {mobileViewport ? <span>Details</span> : null}
              </button>
              <button
                className={mobileViewport ? "actionBtn mobileActionBtn danger" : "actionBtn danger stopActionBtn"}
                type="button"
                title={selectedSession ? "Stop current run" : "No session selected"}
                disabled={!selectedSession || selectedSessionStoppingFeedback}
                onClick={() => void handleInterrupt()}
              >
                {icon("stop")}
                <span>{selectedSessionStoppingFeedback ? "Stopping" : "Stop"}</span>
              </button>
              <button
                className={mobileViewport ? `actionBtn mobileActionBtn${selectedSession?.harness_enabled ? " active" : ""}` : `icon-btn${selectedSession?.harness_enabled ? " active" : ""}`}
                type="button"
                title="Harness mode"
                onClick={() => {
                  setHarnessOpen(true);
                  void loadHarness();
                }}
              >
                {icon("harness")}
                {mobileViewport ? <span>Harness</span> : null}
              </button>
            </div>
          </div>

          <div className="toast muted">{toastText}</div>

          <div className={`workspaceBody${floatingProgressEvent ? " hasFloatingProgress" : ""}`}>
            <div className="workspaceMain">
              <div className="chatWrap">
                <div className="chat" ref={chatScrollRef} onScroll={updateChatScrollFollowState}>
                  <div className="chatInner">
                    {hasOlder && historyTopBoundaryReached ? (
                      <button className="olderBtn" type="button" disabled={loadingOlder} onClick={() => void loadOlder()}>
                        {loadingOlder ? "Loading older messages…" : "Load older messages"}
                      </button>
                    ) : null}
                    {visibleTranscript.map((event, index) => {
                      const collapsed = transcriptEventCollapsed(event);
                      return (
                        <TranscriptEventRow
                          key={event.id}
                          event={event}
                          events={visibleTranscript}
                          index={index}
                          collapsed={collapsed}
                          onToggle={toggleTranscriptEvent}
                          onAskUserRespond={handleAskUserRespond}
                          onCopyText={copyTranscriptEvent}
                        />
                      );
                    })}
                    {workingIndicatorLabel ? <WorkingIndicator label={workingIndicatorLabel} tone={workingIndicatorTone} /> : null}
                    {!visibleTranscript.length && !workingIndicatorLabel ? <div className="emptyState">No transcript yet for this session.</div> : null}
                  </div>
                </div>
              </div>
              {floatingProgressEvent ? <FloatingProgress event={floatingProgressEvent} collapsed={floatingProgressCollapsed} onToggle={() => toggleFloatingProgress(floatingProgressEvent.id)} /> : null}
            </div>

            {showFilesPanel || showDetailsPanel ? (
              <div
                className="detailRailResizer"
                role="separator"
                aria-orientation="vertical"
                aria-label="Resize detail panel"
                onPointerDown={handleDetailRailResizeStart}
              />
            ) : null}

            {showFilesPanel || showDetailsPanel ? (
              <aside className="detailRail">
                {showDetailsPanel ? (
                  <section className="detailSection">
                    <div className="detailSectionHeader">Diagnostics</div>
                    {detailsLoading ? <div className="muted">Loading diagnostics…</div> : null}
                    {!detailsLoading && diagnostics ? (
                      <div className="detailsGrid">
                        {[
                          ["Session", diagnostics.session_id],
                          ["Thread", diagnostics.thread_id || "-"],
                          ["Busy", diagnostics.busy ? "busy" : "idle"],
                          ["Queue", String(diagnostics.queue_len)],
                          ["CWD", diagnostics.cwd],
                          ["tmux", diagnostics.tmux_session ? `${diagnostics.tmux_session}${diagnostics.tmux_window ? `:${diagnostics.tmux_window}` : ""}` : "-"],
                          ["Branch", diagnostics.git_branch || "-"],
                          ["Provider", diagnostics.provider_choice || diagnostics.model_provider || "-"],
                          ["Model", diagnostics.model || "-"],
                          ["Reasoning", diagnostics.reasoning_effort || "-"],
                          ["Service tier", diagnostics.service_tier || "-"],
                          ["Priority", diagnostics.final_priority.toFixed(4)],
                        ].map(([label, value]) => (
                          <div className="detailsRow" key={label}>
                            <div className="detailsLabel">{label}</div>
                            <div className="detailsValue">{value}</div>
                          </div>
                        ))}
                      </div>
                    ) : null}
                  </section>
                ) : null}

                {showFilesPanel ? (
                  <section className="detailSection fileSection">
                    <div className="detailSectionHeader">Files</div>
                    <form
                      className="fileSearch"
                      onSubmit={(event) => {
                        event.preventDefault();
                        void searchFiles();
                      }}
                    >
                      <input
                        value={fileSearchQuery}
                        placeholder="Search or type a relative path…"
                        onInput={(event) => {
                          const next = (event.currentTarget as HTMLInputElement).value;
                          setFileSearchQuery(next);
                          if (!next.trim()) setFileSearchEntries([]);
                        }}
                      />
                      <button className="secondaryBtn" type="submit" disabled={fileSearchLoading || !fileSearchQuery.trim()}>
                        {fileSearchLoading ? "Searching…" : "Search"}
                      </button>
                      <button className="secondaryBtn" type="button" disabled={fileOpenLoading || !fileSearchQuery.trim()} onClick={() => void openFile(fileSearchQuery.trim())}>
                        {fileOpenLoading ? "Opening..." : "Open path"}
                      </button>
                    </form>
                    {filesLoading ? <div className="muted">Loading files…</div> : null}
                    {fileOpenError ? <div className="fileOpenError">{fileOpenError}</div> : null}
                    {fileOpenLoading ? <div className="muted">Opening {activeFilePath}...</div> : null}
                    <div className="fileList">
                      {visibleFileEntries.map((entry) => (
                        <button
                          key={entry.display_path}
                          className={`fileEntry${activeFilePath === entry.request_path ? " active" : ""}`}
                          type="button"
                          onClick={() => void openFile(entry.request_path)}
                        >
                          <div className="fileEntryPath">{entry.display_path}</div>
                          <div className="fileEntryMeta">{entry.summary}</div>
                        </button>
                      ))}
                      {!filesLoading && !visibleFileEntries.length ? (
                        <div className="muted">{fileSearchQuery.trim() ? "No matching files. Use Open path to try this exact path." : "No tracked or changed files yet."}</div>
                      ) : null}
                    </div>
                    {activeFile ? (
                      <div className="filePreview">
                        <div className="filePreviewHeader">{activeFile.rel}</div>
                        {"text" === activeFile.kind ? <pre className="filePreviewText">{activeFile.text}</pre> : null}
                        {"image" === activeFile.kind ? (
                          <div className="filePreviewMedia">
                            <img src={activeFile.image_url} alt={activeFile.rel} />
                          </div>
                        ) : null}
                        {"pdf" === activeFile.kind ? (
                          <a className="filePreviewLink" href={activeFile.pdf_url} target="_blank" rel="noreferrer">
                            Open PDF
                          </a>
                        ) : null}
                        {"download_only" === activeFile.kind ? (
                          <div className="muted">{activeFile.reason || "Binary file; open from the tracked path."}</div>
                        ) : null}
                      </div>
                    ) : null}
                  </section>
                ) : null}
              </aside>
            ) : null}
          </div>
          <div className="composer">
            {queueLen > 0 ? (
              <button className="queuePreview" type="button" onClick={() => openQueueModal()}>
                <span className="queuePreviewHeader">
                  <span>Queued messages</span>
                  <span>{queueLoading ? "loading" : `${queueLen}`}</span>
                </span>
                <span className="queuePreviewList">
                  {queuePreviewItems.map((item, index) => (
                    <span className="queuePreviewItem" key={item.id}>
                      <span>{index + 1}</span>
                      <span>{item.text}</span>
                    </span>
                  ))}
                  {!queuePreviewItems.length ? <span className="queuePreviewEmpty">Loading queued messages...</span> : null}
                  {queueLen > queuePreviewItems.length ? <span className="queuePreviewMore">+{queueLen - queuePreviewItems.length} more</span> : null}
                </span>
              </button>
            ) : null}
            <form
              className={composerSessionBusy ? "isBusy" : undefined}
              onSubmit={(event) => {
                event.preventDefault();
                void handleSend();
              }}
            >
              <button className="icon-btn" type="button" title={attachBusy ? "Attaching…" : "Attach file"} disabled={attachBusy || !selectedSessionId} onClick={() => fileInputRef.current?.click()}>
                {icon("paperclip")}
              </button>
              <input
                ref={fileInputRef}
                className="hiddenFileInput"
                type="file"
                onChange={(event) => {
                  const file = (event.currentTarget as HTMLInputElement).files?.[0] || null;
                  void handleAttachFile(file);
                }}
              />
              <div className="inputWrap">
                <textarea
                  ref={composerInputRef}
                  value={composerText}
                  onInput={(event) => setSessionDraft(selectedSessionId, (event.currentTarget as HTMLTextAreaElement).value)}
                  onKeyDown={handleComposerKeyDown}
                  aria-label="Enter your instructions here"
                />
                {!composerText ? <div className="ph">Enter your instructions here</div> : null}
              </div>
              {composerSessionBusy ? (
                <div className="composerMode" role="group" aria-label="Busy send mode">
                  <button
                    className={`modeBtn${busySubmitMode === "queue" ? " active" : ""}`}
                    type="button"
                    aria-pressed={busySubmitMode === "queue"}
                    title="Queue while session is working"
                    onClick={() => setBusySubmitMode("queue")}
                  >
                    {icon("queue")}
                    <span>Queue</span>
                  </button>
                  <button
                    className={`modeBtn${busySubmitMode === "interrupt" ? " active" : ""}`}
                    type="button"
                    aria-pressed={busySubmitMode === "interrupt"}
                    title="Interrupt current work before sending"
                    onClick={() => setBusySubmitMode("interrupt")}
                  >
                    {icon("stop")}
                    <span>Interrupt</span>
                  </button>
                </div>
              ) : null}
              <button className="icon-btn" type="button" title="Queued messages" onClick={() => void handleQueueAction()}>
                {icon("queue")}
              </button>
              <button
                className="icon-btn primary"
                type="submit"
                title={sending ? "Sending…" : composerSessionBusy && busySubmitMode === "queue" ? "Queue" : composerSessionBusy ? "Interrupt and send" : "Send"}
                disabled={sending || !selectedSessionId}
              >
                {icon("send")}
              </button>
            </form>
          </div>
        </div>
      </div>

      {sessionContextMenu && contextMenuSession ? (
        <div
          className="sessionContextMenu"
          style={{ left: `${sessionContextMenu.x}px`, top: `${sessionContextMenu.y}px` }}
          onClick={(event) => event.stopPropagation()}
          onContextMenu={(event) => event.preventDefault()}
      >
          <div className="sessionContextMenuSection">
            <div className="sessionContextMenuLabel">Marker</div>
            <div className="sessionMarkerChoices" role="group" aria-label="Set session marker">
              {SESSION_MARKER_STATES.map((state) => {
                const selected = contextMenuMarkerState === state;
                return (
                  <button
                    key={state}
                    className={`sessionMarkerChoice marker-${state}${selected ? " active" : ""}`}
                    type="button"
                    aria-pressed={selected}
                    disabled={selected || sessionMarkerBusyId === contextMenuSession.session_id}
                    onClick={() => void handleSessionMarker(contextMenuSession, state)}
                  >
                    {sessionMarkerLabel(state)}
                  </button>
                );
              })}
            </div>
          </div>
          <button type="button" onClick={() => openRenameDialog(contextMenuSession)}>
            Rename
          </button>
          <button
            type="button"
            onClick={() => {
              focusSidebarSession(contextMenuSession.session_id);
              setShowDetailsPanel(true);
            }}
          >
            Details
          </button>
          <button
            type="button"
            onClick={() => {
              focusSidebarSession(contextMenuSession.session_id);
              setShowFilesPanel(true);
            }}
          >
            Files
          </button>
          <button type="button" onClick={() => void handleInterrupt(contextMenuSession)}>
            Interrupt
          </button>
          <button
            type="button"
            disabled={!tmuxAttachCommandForSession(contextMenuSession)}
            onClick={async () => {
              await copyToClipboard(tmuxAttachCommandForSession(contextMenuSession));
              setSessionContextMenu(null);
              pushToast("tmux command copied");
            }}
          >
            Copy tmux command
          </button>
          <button className="dangerText" type="button" onClick={() => void handleCloseSession(contextMenuSession)}>
            Close session
          </button>
        </div>
      ) : null}

      {newSessionOpen ? (
        <div className="modalBackdrop" onClick={() => setNewSessionOpen(false)}>
          <div className="modalCard" onClick={(event) => event.stopPropagation()}>
            <div className="modalHeader">
              <div>New session</div>
              <button className="icon-btn" type="button" onClick={() => setNewSessionOpen(false)}>
                {icon("info")}
              </button>
            </div>
            <div className="formGrid">
              <div className="backendTabs">
                {(["codex", "pi"] as const).map((backend) => (
                  <button
                    key={backend}
                    className={`backendTab${newSessionBackend === backend ? " active" : ""}`}
                    type="button"
                    onClick={() => {
                      const values = newSessionValuesForBackend(newSessionDefaults, backend, newSessionPreferences, tmuxAvailable);
                      setNewSessionBackend(backend);
                      setNewSessionProvider(values.provider);
                      setNewSessionModel(values.model);
                      setNewSessionReasoning(values.reasoning);
                      setNewSessionFast(values.fast);
                      setNewSessionTmux(values.tmux);
                    }}
                  >
                    {backend.toUpperCase()}
                  </button>
                ))}
              </div>
              <label className="field">
                <span>CWD</span>
                <div
                  className="cwdAutocomplete"
                  onFocusCapture={() => setNewSessionCwdSuggestionsOpen(true)}
                  onBlurCapture={(event) => {
                    const next = event.relatedTarget;
                    if (next instanceof Node && event.currentTarget.contains(next)) return;
                    setNewSessionCwdSuggestionsOpen(false);
                  }}
                >
                  <input
                    ref={newSessionCwdInputRef}
                    value={newSessionCwd}
                    onInput={(event) => {
                      setNewSessionCwd((event.currentTarget as HTMLInputElement).value);
                      setNewSessionCwdSuggestionsOpen(true);
                    }}
                    onKeyDown={handleNewSessionCwdKeyDown}
                    placeholder="/path/to/workspace"
                    role="combobox"
                    aria-autocomplete="list"
                    aria-expanded={newSessionCwdSuggestionsOpen}
                    aria-controls="new-session-cwd-suggestions"
                    aria-activedescendant={
                      newSessionCwdSuggestionsOpen && newSessionCwdSuggestionIndex >= 0
                        ? `new-session-cwd-option-${newSessionCwdSuggestionIndex}`
                        : undefined
                    }
                    autoComplete="off"
                  />
                  {newSessionCwdSuggestionsOpen &&
                  (newSessionCwdSuggestionsLoading || Boolean(newSessionCwdSuggestionError) || newSessionCwdSuggestionChoices.length || Boolean(newSessionCwd.trim())) ? (
                    <div className="cwdSuggestions" id="new-session-cwd-suggestions" role="listbox">
                      {newSessionCwdSuggestionChoices.map((suggestion, index) => (
                        <button
                          key={`${suggestion.kind}-${suggestion.value}`}
                          id={`new-session-cwd-option-${index}`}
                          className={`cwdSuggestion${index === newSessionCwdSuggestionIndex ? " active" : ""}`}
                          type="button"
                          role="option"
                          aria-selected={index === newSessionCwdSuggestionIndex}
                          onMouseEnter={() => setNewSessionCwdSuggestionIndex(index)}
                          onClick={() => applyNewSessionCwdSuggestion(suggestion)}
                        >
                          <span className="cwdSuggestionRow">
                            <span className="cwdSuggestionLabel">{suggestion.label}</span>
                            {suggestion.kind === "recent" ? <span className="cwdSuggestionBadge">Recent</span> : null}
                          </span>
                          <span className="cwdSuggestionPath">{suggestion.value}</span>
                        </button>
                      ))}
                      {!newSessionCwdSuggestionsLoading && !newSessionCwdSuggestionError && !newSessionCwdSuggestionChoices.length ? (
                        <div className="cwdSuggestionHint">No matching folders.</div>
                      ) : null}
                      {newSessionCwdSuggestionsLoading ? <div className="cwdSuggestionHint">Looking up folders…</div> : null}
                      {newSessionCwdSuggestionError ? <div className="cwdSuggestionHint is-error">{newSessionCwdSuggestionError}</div> : null}
                    </div>
                  ) : null}
                </div>
              </label>
              <div className="field">
                <span>Resume existing</span>
                <div className="resumeChoices">
                  <button
                    className={`resumeChoice${newSessionResumeSelection ? "" : " active"}`}
                    type="button"
                    onClick={() => setNewSessionResumeSelection(null)}
                  >
                    Start fresh
                  </button>
                  {newSessionResumeCandidates.map((candidate) => (
                    <button
                      key={candidate.session_id}
                      className={`resumeChoice${newSessionResumeSelection?.session_id === candidate.session_id ? " active" : ""}`}
                      type="button"
                      onClick={() => setNewSessionResumeSelection(candidate)}
                    >
                      {(candidate.alias || candidate.last_user_message || candidate.session_id).trim()}
                    </button>
                  ))}
                  {!newSessionResumeCandidates.length ? <div className="muted">No matching sessions for this cwd.</div> : null}
                </div>
              </div>
              <div className="twoCol">
                <label className="field">
                  <span>Provider</span>
                  <select value={newSessionProvider} onChange={(event) => setNewSessionProvider((event.currentTarget as HTMLSelectElement).value)}>
                    {newSessionProviders.map((choice) => (
                      <option key={choice} value={choice}>
                        {choice}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="field">
                  <span>Model</span>
                  <input value={newSessionModel} onInput={(event) => setNewSessionModel((event.currentTarget as HTMLInputElement).value)} />
                </label>
              </div>
              <div className="twoCol">
                <label className="field">
                  <span>Reasoning</span>
                  <select value={newSessionReasoning} onChange={(event) => setNewSessionReasoning((event.currentTarget as HTMLSelectElement).value)}>
                    {newSessionReasoningChoices.map((choice) => (
                      <option key={choice} value={choice}>
                        {choice}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="toggleRow">
                  <input
                    type="checkbox"
                    checked={newSessionFast}
                    disabled={!currentNewSessionDefaults?.supports_fast}
                    onChange={(event) => setNewSessionFast((event.currentTarget as HTMLInputElement).checked)}
                  />
                  <span>Fast tier</span>
                </label>
              </div>
              <div className="twoCol">
                <label className="toggleRow">
                  <input
                    type="checkbox"
                    checked={newSessionTmux}
                    disabled={!tmuxAvailable}
                    onChange={(event) => setNewSessionTmux((event.currentTarget as HTMLInputElement).checked)}
                  />
                  <span>Start in tmux</span>
                </label>
                <label className="toggleRow">
                  <input
                    type="checkbox"
                    checked={newSessionWorktree}
                    disabled={Boolean(newSessionResumeSelection)}
                    onChange={(event) => setNewSessionWorktree((event.currentTarget as HTMLInputElement).checked)}
                  />
                  <span>Create worktree</span>
                </label>
              </div>
              {newSessionWorktree && !newSessionResumeSelection ? (
                <label className="field">
                  <span>Worktree branch</span>
                  <input
                    value={newSessionWorktreeBranch}
                    onInput={(event) => setNewSessionWorktreeBranch((event.currentTarget as HTMLInputElement).value)}
                    placeholder={makeWorktreeSlug(baseName(newSessionCwd))}
                  />
                </label>
              ) : null}
              {newSessionError ? <div className="error-inline">{newSessionError}</div> : null}
              <div className="modalActions">
                <button className="secondaryBtn" type="button" onClick={() => setNewSessionOpen(false)}>
                  Cancel
                </button>
                <button className="primary" type="button" disabled={newSessionBusy} onClick={() => void handleCreateSession()}>
                  {newSessionBusy ? "Starting…" : newSessionResumeSelection ? "Resume session" : newSessionWorktree ? "Create worktree session" : "Start session"}
                </button>
              </div>
            </div>
          </div>
        </div>
      ) : null}

      {renameOpen ? (
        <div className="modalBackdrop" onClick={() => setRenameOpen(false)}>
          <div className="modalCard compactModal" onClick={(event) => event.stopPropagation()}>
            <div className="modalHeader">
              <div>Rename session</div>
              <button className="icon-btn" type="button" onClick={() => setRenameOpen(false)}>
                {icon("info")}
              </button>
            </div>
            <form
              className="formGrid"
              onSubmit={(event) => {
                event.preventDefault();
                void handleRenameSession();
              }}
            >
              <label className="field">
                <span>Name</span>
                <input
                  value={renameName}
                  onInput={(event) => setRenameName((event.currentTarget as HTMLInputElement).value)}
                  placeholder={renameTargetSession ? sessionDisplayName(renameTargetSession) : "Session name"}
                  autoFocus
                />
              </label>
              {renameError ? <div className="error-inline">{renameError}</div> : null}
              <div className="modalActions">
                <button className="secondaryBtn" type="button" onClick={() => setRenameOpen(false)}>
                  Cancel
                </button>
                <button className="primary" type="submit" disabled={renameBusy}>
                  {renameBusy ? "Saving..." : "Save"}
                </button>
              </div>
            </form>
          </div>
        </div>
      ) : null}

      {shareCreateOpen ? (
        <div className="modalBackdrop" onClick={() => setShareCreateOpen(false)}>
          <div className="modalCard shareManageModal" onClick={(event) => event.stopPropagation()}>
            <div className="modalHeader">
              <div>Share sessions</div>
              <button className="icon-btn" type="button" onClick={() => setShareCreateOpen(false)}>
                {icon("close")}
              </button>
            </div>
            <form
              className="formGrid"
              onSubmit={(event) => {
                event.preventDefault();
                void handleCreateShareLink();
              }}
            >
              <label className="field">
                <span>Share set name</span>
                <input
                  value={shareCreateLabel}
                  onInput={(event) => setShareCreateLabel((event.currentTarget as HTMLInputElement).value)}
                  placeholder={selectedSession ? sessionDisplayName(selectedSession) : "Shared sessions"}
                />
              </label>
              <label className="field">
                <span>Expires in hours</span>
                <input
                  type="number"
                  min="1"
                  max="720"
                  value={String(shareCreateExpiresHours)}
                  onInput={(event) => setShareCreateExpiresHours(Number((event.currentTarget as HTMLInputElement).value) || 24)}
                />
              </label>
              <div className="field">
                <span>Sessions</span>
                <div className="shareSessionChoices">
                  {shareCreateSessions.map((session) => (
                    <label className="shareSessionChoice" key={session.session_id}>
                      <input type="checkbox" checked={session.checked} onChange={() => toggleShareDraftSession(session.session_id)} />
                      <span>
                        <strong>{session.label}</strong>
                        <small>
                          <span>{session.workspace}</span>
                          <span>{session.session_id}</span>
                        </small>
                        {session.cwd ? <small className="shareSessionPath">{session.cwd}</small> : null}
                      </span>
                    </label>
                  ))}
                </div>
              </div>
              {shareCreateResult ? (
                <div className="shareResult">
                  <label className="field">
                    <span>URL</span>
                    <input readOnly value={absoluteShareUrl(shareCreateResult.share_url)} onFocus={(event) => event.currentTarget.select()} />
                  </label>
                  <label className="field">
                    <span>Temporary password</span>
                    <input readOnly value={shareCreateResult.share_password} onFocus={(event) => event.currentTarget.select()} />
                  </label>
                  <button className="secondaryBtn" type="button" onClick={() => void copyShareInvitation()}>
                    Copy invite
                  </button>
                </div>
              ) : null}
              <section className="shareManager">
                <div className="shareManagerHeader">
                  <div>
                    <strong>Created shares</strong>
                    <span>{managedShares.length ? `${managedShares.length} active` : "No active share sets"}</span>
                  </div>
                  <button className="secondaryBtn" type="button" disabled={managedSharesLoading} onClick={() => void refreshManagedShares()}>
                    {managedSharesLoading ? "Refreshing..." : "Refresh"}
                  </button>
                </div>
                <div className="shareManagerList">
                  {managedShares.map((share) => (
                    <article className="shareManagerItem" key={share.share_id}>
                      <div className="shareManagerMain">
                        <div className="shareManagerTitleRow">
                          <strong>{share.label || "Shared sessions"}</strong>
                          <span className={`shareExpiry${share.expires_at <= Date.now() / 1000 ? " expired" : ""}`}>{formatShareExpiry(share.expires_at)}</span>
                        </div>
                        <div className="shareManagerMeta">
                          <span>{share.sessions.length} session{share.sessions.length === 1 ? "" : "s"}</span>
                          <span>{share.share_id}</span>
                          {share.password_hint ? <span>hint {share.password_hint}</span> : null}
                        </div>
                      </div>
                      <div className="shareManagerActions">
                        <a className="secondaryBtn" href={shareSetUrl(share)} target="_blank" rel="noreferrer">
                          Open
                        </a>
                        <button className="secondaryBtn" type="button" onClick={() => void copyManagedShare(share)}>
                          Copy
                        </button>
                        <button
                          className="icon-btn danger"
                          type="button"
                          title="Close share"
                          disabled={deletingShareId === share.share_id}
                          onClick={() => void deleteManagedShare(share.share_id)}
                        >
                          {icon("trash")}
                        </button>
                      </div>
                    </article>
                  ))}
                  {!managedShares.length ? <div className="shareManagerEmpty">Created share sets will appear here.</div> : null}
                </div>
              </section>
              {shareCreateError ? <div className="error-inline">{shareCreateError}</div> : null}
              <div className="modalActions">
                <button className="secondaryBtn" type="button" onClick={() => setShareCreateOpen(false)}>
                  Close
                </button>
                <button className="primary" type="submit" disabled={shareCreateBusy || !selectedShareSessionCount}>
                  {shareCreateBusy ? "Creating..." : shareCreateResult ? "Create another" : "Create share"}
                </button>
              </div>
            </form>
          </div>
        </div>
      ) : null}

      {settingsOpen ? (
        <div className="modalBackdrop" onClick={() => setSettingsOpen(false)}>
          <div className="modalCard" onClick={(event) => event.stopPropagation()}>
            <div className="modalHeader">
              <div>Settings</div>
              <button className="icon-btn" type="button" onClick={() => setSettingsOpen(false)}>
                {icon("info")}
              </button>
            </div>
            {settingsInitialLoading ? (
              <div className="muted">Loading settings…</div>
            ) : (
              <div className="formGrid">
                {settingsLoadError ? (
                  <div className="error-inline">
                    {settingsLoadError}
                    <button className="secondaryBtn inlineRetry" type="button" onClick={() => void loadVoiceAndNotifications()}>
                      Retry
                    </button>
                  </div>
                ) : null}
                <div className="detailSectionHeader">Appearance</div>
                <div className="themePicker" role="group" aria-label="Theme">
                  <button className={`themeChoice${themeMode === "dark" ? " active" : ""}`} type="button" onClick={() => setThemeMode("dark")}>
                    Dark
                  </button>
                  <button className={`themeChoice${themeMode === "light" ? " active" : ""}`} type="button" onClick={() => setThemeMode("light")}>
                    Light
                  </button>
                </div>
                <label className="toggleRow">
                  <input
                    type="checkbox"
                    checked={compactSessionCards}
                    onChange={(event) => setCompactSessionCards((event.currentTarget as HTMLInputElement).checked)}
                  />
                  <span>Compact session cards</span>
                </label>
                <div className="detailSectionHeader">Voice</div>
                <label className="field">
                  <span>TTS base URL</span>
                  <input value={voiceBaseUrl} onInput={(event) => setVoiceBaseUrl((event.currentTarget as HTMLInputElement).value)} />
                </label>
                <label className="field">
                  <span>TTS API key</span>
                  <input
                    type="password"
                    value={voiceApiKey}
                    onInput={(event) => setVoiceApiKey((event.currentTarget as HTMLInputElement).value)}
                    placeholder="Leave blank to keep current key"
                  />
                </label>
                <label className="toggleRow">
                  <input
                    type="checkbox"
                    checked={voiceNarrationEnabled}
                    onChange={(event) => setVoiceNarrationEnabled((event.currentTarget as HTMLInputElement).checked)}
                  />
                  <span>Narration announcements</span>
                </label>
                <div className="muted">
                  Stream: {displayedVoiceSettings.audio.stream_url || "-"} · listeners {displayedVoiceSettings.audio.active_listener_count} · queued {displayedVoiceSettings.audio.queue_depth}
                </div>
                <div className="detailSectionHeader">Notifications</div>
                <label className="toggleRow">
                  <input
                    type="checkbox"
                    checked={desktopNotificationsEnabled}
                    onChange={async (event) => {
                      const next = (event.currentTarget as HTMLInputElement).checked;
                      if (next && typeof Notification !== "undefined" && Notification.permission !== "granted") {
                        const permission = await Notification.requestPermission();
                        if (permission !== "granted") {
                          pushToast(`Notification permission ${permission}`);
                          return;
                        }
                      }
                      setDesktopNotificationsEnabled(next);
                    }}
                  />
                  <span>Desktop notifications</span>
                </label>
                <div className="muted">
                  Server subscriptions: {notificationSnapshot?.subscriptions.length || 0} · enabled devices {displayedVoiceSettings.notifications.enabled_devices}
                </div>
                <label className="toggleRow">
                  <input
                    type="checkbox"
                    checked={mobilePushEnabled}
                    disabled={notificationDeviceClass() !== "mobile" || !("serviceWorker" in navigator) || !("PushManager" in window)}
                    onChange={async (event) => {
                      const next = (event.currentTarget as HTMLInputElement).checked;
                      try {
                        await setMobilePush(next);
                      } catch (error) {
                        setErrorText(error instanceof Error ? error.message : "Unable to update mobile push");
                      }
                    }}
                  />
                  <span>Mobile push notifications</span>
                </label>
                <div className="muted">
                  Current push endpoint: {mobilePushEndpoint ? "registered" : "not registered"}
                </div>
                <div className="detailSectionHeader">Codex config.toml</div>
                <div className="settingsPath">{codexConfig?.path || "config.toml"}</div>
                <label className="field">
                  <span>config.toml</span>
                  <textarea className="configTextarea" value={codexConfigText} onInput={(event) => setCodexConfigText((event.currentTarget as HTMLTextAreaElement).value)} spellcheck={false} />
                </label>
                <div className="configActions">
                  <button className="secondaryBtn" type="button" onClick={() => void reloadCodexConfig()}>
                    Reload
                  </button>
                  <button className="secondaryBtn" type="button" disabled={codexConfigSaving} onClick={() => void saveCodexConfig()}>
                    {codexConfigSaving ? "Saving…" : "Save config.toml"}
                  </button>
                  <button className="secondaryBtn" type="button" disabled={codexConfigSaving || serviceRestarting} onClick={() => void restartLocalService()}>
                    {serviceRestarting ? "Restarting service…" : "Restart service"}
                  </button>
                </div>
                <div className="muted">Save config.toml, then restart the local service so provider changes are reloaded.</div>
                <div className="modalActions">
                  <button className="secondaryBtn" type="button" onClick={() => setSettingsOpen(false)}>
                    Close
                  </button>
                  <button className="primary" type="button" disabled={voiceSettingsSaving} onClick={() => void saveVoiceAndNotifications()}>
                    {voiceSettingsSaving ? "Saving…" : "Save"}
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      ) : null}

      {queueOpen ? (
        <div className="modalBackdrop" onClick={() => setQueueOpen(false)}>
          <div className="modalCard" onClick={(event) => event.stopPropagation()}>
            <div className="modalHeader">
              <div>Queued messages</div>
              <button className="icon-btn" type="button" onClick={() => setQueueOpen(false)}>
                {icon("info")}
              </button>
            </div>
            {queueLoading ? <div className="muted">Loading queue…</div> : null}
            <div className="queueList">
              {queueItems.map((item, index) => (
                <div className="queueItem" key={item.id}>
                  <textarea
                    className="queueText"
                    value={queueDrafts[item.id] ?? item.text}
                    onInput={(event) =>
                      setQueueDrafts((current) => ({
                        ...current,
                        [item.id]: (event.currentTarget as HTMLTextAreaElement).value,
                      }))
                    }
                  />
                  <div className="queueActions">
                    <button className="icon-btn" type="button" disabled={index === 0 || item.sending} onClick={() => void moveQueueItem(item.id, index - 1)}>
                      {icon("up")}
                    </button>
                    <button
                      className="icon-btn"
                      type="button"
                      disabled={index === queueItems.length - 1 || item.sending}
                      onClick={() => void moveQueueItem(item.id, index + 1)}
                    >
                      {icon("down")}
                    </button>
                    <button className="secondaryBtn" type="button" onClick={() => void saveQueueItem(item.id)}>
                      Save
                    </button>
                    <button className="icon-btn danger" type="button" onClick={() => void deleteQueueItem(item.id)}>
                      {icon("trash")}
                    </button>
                  </div>
                </div>
              ))}
              {!queueLoading && !queueItems.length ? <div className="muted">No queued messages.</div> : null}
            </div>
          </div>
        </div>
      ) : null}

      {harnessOpen ? (
        <div className="modalBackdrop" onClick={() => setHarnessOpen(false)}>
          <div className="modalCard" onClick={(event) => event.stopPropagation()}>
            <div className="modalHeader">
              <div>Harness</div>
              <button className="icon-btn" type="button" onClick={() => setHarnessOpen(false)}>
                {icon("info")}
              </button>
            </div>
            {harnessLoading || !harnessDraft ? (
              <div className="muted">Loading harness config…</div>
            ) : (
              <div className="formGrid">
                <label className="toggleRow">
                  <input
                    type="checkbox"
                    checked={harnessDraft.enabled}
                    onChange={(event) =>
                      setHarnessDraft((current) => (current ? { ...current, enabled: (event.currentTarget as HTMLInputElement).checked } : current))
                    }
                  />
                  <span>Enable harness</span>
                </label>
                <label className="field">
                  <span>Additional request</span>
                  <textarea
                    value={harnessDraft.request}
                    onInput={(event) =>
                      setHarnessDraft((current) => (current ? { ...current, request: (event.currentTarget as HTMLTextAreaElement).value } : current))
                    }
                  />
                </label>
                <label className="field">
                  <span>Cooldown minutes</span>
                  <input
                    type="number"
                    min={1}
                    value={String(harnessDraft.cooldown_minutes)}
                    onInput={(event) =>
                      setHarnessDraft((current) =>
                        current
                          ? {
                              ...current,
                              cooldown_minutes: Math.max(1, Number((event.currentTarget as HTMLInputElement).value || 1)),
                            }
                          : current,
                      )
                    }
                  />
                </label>
                <label className="field">
                  <span>Remaining injections</span>
                  <input
                    type="number"
                    min={0}
                    value={String(harnessDraft.remaining_injections)}
                    onInput={(event) =>
                      setHarnessDraft((current) =>
                        current
                          ? {
                              ...current,
                              remaining_injections: Math.max(0, Number((event.currentTarget as HTMLInputElement).value || 0)),
                            }
                          : current,
                      )
                    }
                  />
                </label>
                <div className="modalActions">
                  <button className="secondaryBtn" type="button" onClick={() => setHarnessOpen(false)}>
                    Cancel
                  </button>
                  <button className="primary" type="button" disabled={harnessSaving} onClick={() => void saveHarness()}>
                    {harnessSaving ? "Saving…" : "Save"}
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      ) : null}

      {errorText ? <div className="error-toast">{errorText}</div> : null}
    </>
  );
}
