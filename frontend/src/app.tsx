import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { api } from "./lib/api";
import { PretextParagraph } from "./lib/pretext";
import type {
  ChangedFilesResponse,
  DiagnosticsResponse,
  FileEntry,
  FileReadResponse,
  HarnessConfig,
  NewSessionDefaults,
  NotificationSubscriptionsResponse,
  QueueResponse,
  QueueItem,
  RawChatEvent,
  ResumeCandidate,
  SessionSummary,
  UiTranscriptEvent,
  VoiceSettingsResponse,
} from "./lib/types";

const INIT_PAGE_LIMIT = 120;
const OLDER_PAGE_LIMIT = 60;
const POLL_IDLE_MS = 1400;
const POLL_BUSY_MS = 700;
const SESSION_REFRESH_MS = 12000;
const NOTIFICATION_POLL_MS = 5000;
const ATTACH_UPLOAD_MAX_BYTES = 16 * 1024 * 1024;
const SELECTED_SESSION_KEY = "codoxear.nova.selected";
const SHOW_TOOL_CALLS_KEY = "codoxear.showToolCalls";
const DESKTOP_NOTIFICATIONS_KEY = "codoxear.desktopNotificationsEnabled";
const CHAT_BOTTOM_FOLLOW_THRESHOLD_PX = 96;

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

function sessionIsStarting(session: SessionSummary | null) {
  return Boolean(session && session.owned && !session.log_path);
}

function sessionStatusText(session: SessionSummary) {
  if (session.queue_len) return `queue ${session.queue_len}`;
  if (sessionIsStarting(session)) return "starting";
  if (session.busy) return "working";
  return "idle";
}

function workspaceKeyForSession(session: SessionSummary) {
  return String(session.cwd || "").trim() || "__unknown_workspace__";
}

function workspaceTitle(cwd: string) {
  return cwd === "__unknown_workspace__" ? "Unknown workspace" : baseName(cwd) || cwd;
}

function defaultsForBackend(defaults: NewSessionDefaults | null, backend: "codex" | "pi") {
  if (!defaults) return null;
  return defaults.backends[backend];
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

function formatTime(ts: number | null) {
  if (!(typeof ts === "number" && Number.isFinite(ts) && ts > 0)) return "";
  return new Date(ts * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
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

function eventTextPreview(event: UiTranscriptEvent) {
  const body = String(event.body || "").trim();
  if (!body) return isCollapsibleEvent(event.kind) ? "" : event.title || "";
  const preview = body.replace(/\s+/g, " ").slice(0, 160);
  if (!isCollapsibleEvent(event.kind) || !event.title) return preview;
  return stripRepeatedTitlePrefix(preview, event.title);
}

function stripRepeatedTitlePrefix(preview: string, title: string) {
  const normalizedTitle = title.trim();
  if (!preview || !normalizedTitle) return preview;
  const lowerPreview = preview.toLocaleLowerCase();
  const lowerTitle = normalizedTitle.toLocaleLowerCase();
  if (lowerPreview === lowerTitle) return "";
  const separators = [":", "-", "—", "–", "·", "|"];
  for (const separator of separators) {
    const prefix = `${lowerTitle}${separator}`;
    if (lowerPreview.startsWith(prefix)) return preview.slice(normalizedTitle.length + separator.length).trimStart();
  }
  if (lowerPreview.startsWith(`${lowerTitle} `)) return preview.slice(normalizedTitle.length).trimStart();
  return preview;
}

function describeAskUser(
  raw: {
    question?: string;
    context?: string;
    options?: string[];
    questions?: Array<Record<string, unknown>>;
    answer?: string | string[];
  },
) {
  const parts: string[] = [];
  if (typeof raw.question === "string" && raw.question.trim()) parts.push(raw.question.trim());
  if (typeof raw.context === "string" && raw.context.trim()) parts.push(raw.context.trim());
  if (Array.isArray(raw.options) && raw.options.length) {
    parts.push(`Options: ${raw.options.filter(Boolean).join(" · ")}`);
  }
  if (Array.isArray(raw.questions) && raw.questions.length) {
    parts.push(`Questions: ${raw.questions.length}`);
  }
  if (Array.isArray(raw.answer) && raw.answer.length) {
    parts.push(`Answer: ${raw.answer.join(", ")}`);
  } else if (typeof raw.answer === "string" && raw.answer.trim()) {
    parts.push(`Answer: ${raw.answer.trim()}`);
  }
  return parts.join("\n\n");
}

function normalizeEvent(raw: RawChatEvent, index: number, knownToolTitles: Map<string, string>): UiTranscriptEvent | null {
  if ("role" in raw) {
    if (typeof raw.text !== "string" || !raw.text.trim()) return null;
    const ts = typeof raw.ts === "number" && Number.isFinite(raw.ts) ? raw.ts : null;
    const id = raw.message_id || raw.localId || `${raw.role}-${ts ?? "na"}-${index}`;
    return {
      id,
      kind: raw.role,
      ts,
      title: raw.role === "user" ? "User" : "Assistant",
      body: raw.text,
      meta: String(raw.message_class || ""),
    };
  }
  const ts = typeof raw.ts === "number" && Number.isFinite(raw.ts) ? raw.ts : null;
  if (raw.type === "ask_user") {
    return {
      id: raw.tool_call_id || `ask-${ts ?? "na"}-${index}`,
      kind: "ask_user",
      ts,
      title: raw.header || "Ask user",
      body: describeAskUser(raw),
      meta: [
        raw.allow_multiple ? "multiple" : "",
        raw.allow_freeform ? "freeform" : "",
        raw.resolved ? "resolved" : "",
        raw.cancelled ? "cancelled" : "",
      ]
        .filter(Boolean)
        .join(" · "),
    };
  }
  const rawName = typeof raw.name === "string" && raw.name.trim() ? raw.name.trim() : "";
  const knownName = raw.tool_call_id ? knownToolTitles.get(raw.tool_call_id) || "" : "";
  const toolTitle = raw.type === "tool_result" ? knownName || (rawName === "tool" ? "" : rawName) : rawName;
  return {
    id: raw.tool_call_id || `${raw.type}-${raw.name || "tool"}-${ts ?? "na"}-${index}`,
    kind: raw.type,
    ts,
    title: toolTitle || (raw.type === "tool_result" ? "Tool result" : "Tool call"),
    body: String(raw.text || ""),
    meta: [raw.tool_call_id || "", raw.is_error ? "error" : ""].filter(Boolean).join(" · "),
  };
}

function normalizeEvents(events: RawChatEvent[]) {
  const out: UiTranscriptEvent[] = [];
  const knownToolTitles = new Map<string, string>();
  events.forEach((event) => {
    if ("role" in event || event.type !== "tool" || !event.tool_call_id) return;
    if (typeof event.name === "string" && event.name.trim()) knownToolTitles.set(event.tool_call_id, event.name.trim());
  });
  events.forEach((event, index) => {
    const normalized = normalizeEvent(event, index, knownToolTitles);
    if (normalized) out.push(normalized);
  });
  return out;
}

function coalesceAdjacentAssistantEvents(events: UiTranscriptEvent[]) {
  const out: UiTranscriptEvent[] = [];
  for (const event of events) {
    const previous = out[out.length - 1];
    if (previous && previous.kind === "assistant" && event.kind === "assistant") {
      const metaParts = [previous.meta, event.meta].filter((value, index, arr) => value && arr.indexOf(value) === index);
      out[out.length - 1] = {
        ...previous,
        id: `${previous.id}+${event.id}`,
        ts: previous.ts ?? event.ts,
        body: [previous.body, event.body].filter((value) => value.trim()).join("\n\n"),
        meta: metaParts.join(" · "),
      };
      continue;
    }
    out.push(event);
  }
  return out;
}

function shouldShowMessageSide(events: UiTranscriptEvent[], index: number) {
  const event = events[index];
  if (!event || event.kind !== "assistant") return true;
  for (let cursor = index - 1; cursor >= 0; cursor -= 1) {
    const previous = events[cursor];
    if (previous.kind === "user") return true;
    if (previous.kind === "assistant") return false;
  }
  return true;
}

function isAssistantNarration(event: UiTranscriptEvent) {
  return event.kind === "assistant" && event.meta.trim() === "narration";
}

function shouldShowMessageFooter(events: UiTranscriptEvent[], index: number, collapsible: boolean) {
  const event = events[index];
  if (!event || collapsible || (!event.meta && !event.ts)) return false;
  if (!isAssistantNarration(event)) return true;
  for (let cursor = index + 1; cursor < events.length; cursor += 1) {
    const next = events[cursor];
    if (next.kind === "user") return true;
    if (isAssistantNarration(next)) return false;
  }
  return true;
}

function mergeTranscriptEvents(current: UiTranscriptEvent[], incoming: UiTranscriptEvent[]) {
  if (!incoming.length) return current;
  const incomingIds = new Set(incoming.map((event) => event.id));
  const incomingUserBodies = new Set(
    incoming.filter((event) => event.kind === "user").map((event) => event.body.trim()).filter(Boolean),
  );
  return current
    .filter((event) => !incomingIds.has(event.id))
    .filter((event) => !(event.id.startsWith("local-user-") && incomingUserBodies.has(event.body.trim())))
    .concat(incoming);
}

function previewFromEvents(events: UiTranscriptEvent[]) {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    if (event.kind !== "user") continue;
    return eventTextPreview(event);
  }
  return "";
}

function isNearScrollBottom(element: HTMLElement) {
  return element.scrollHeight - element.scrollTop - element.clientHeight <= CHAT_BOTTOM_FOLLOW_THRESHOLD_PX;
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
    case "help":
      return (
        <svg {...common}>
          <circle cx="8" cy="8" r="5.5" />
          <path d="M6.8 6.4A1.6 1.6 0 1 1 8.9 8c-.6.3-.9.7-.9 1.3" />
          <path d="M8 11.8h.01" />
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
    case "edit":
      return (
        <svg {...common}>
          <path d="M9.8 3.2 12.8 6.2" />
          <path d="M11.6 2.4a1.2 1.2 0 0 1 1.7 1.7L6 11.4 3.2 12.8 4.6 10Z" />
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

function eventClassName(kind: UiTranscriptEvent["kind"]) {
  switch (kind) {
    case "user":
      return "event-user";
    case "assistant":
      return "event-assistant";
    case "tool":
      return "event-tool";
    case "tool_result":
      return "event-result";
    case "ask_user":
      return "event-question";
  }
}

function isCollapsibleEvent(kind: UiTranscriptEvent["kind"]) {
  return kind === "tool" || kind === "tool_result" || kind === "ask_user";
}

function renderTokenSummary(token: Record<string, unknown> | null | undefined) {
  if (!token || typeof token !== "object") return "";
  const contextWindow = Number(token.context_window);
  const tokensInContext = Number(token.tokens_in_context);
  const percentRemaining = Number(token.percent_remaining);
  if (Number.isFinite(contextWindow) && Number.isFinite(tokensInContext) && contextWindow > 0) {
    if (Number.isFinite(percentRemaining)) return `${tokensInContext}/${contextWindow} (${Math.round(percentRemaining)}% left)`;
    return `${tokensInContext}/${contextWindow}`;
  }
  return "";
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

export function App() {
  const [authState, setAuthState] = useState<"loading" | "login" | "ready">("loading");
  const [loadingText, setLoadingText] = useState("Loading workspace…");
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [collapsedWorkspaces, setCollapsedWorkspaces] = useState<Record<string, boolean>>({});
  const [selectedSessionId, setSelectedSessionId] = useState("");
  const [transcript, setTranscript] = useState<UiTranscriptEvent[]>([]);
  const [collapsedEvents, setCollapsedEvents] = useState<Record<string, boolean>>({});
  const [busy, setBusy] = useState(false);
  const [sending, setSending] = useState(false);
  const [closingSession, setClosingSession] = useState(false);
  const [queueLen, setQueueLen] = useState(0);
  const [tokenSummary, setTokenSummary] = useState("");
  const [liveCursor, setLiveCursor] = useState<string | null>(null);
  const [historyCursor, setHistoryCursor] = useState<string | null>(null);
  const [hasOlder, setHasOlder] = useState(false);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [errorText, setErrorText] = useState("");
  const [composerText, setComposerText] = useState("");
  const [toastText, setToastText] = useState("");
  const [loginPassword, setLoginPassword] = useState("");
  const [showTools, setShowTools] = useState(() => readLocalStorage(SHOW_TOOL_CALLS_KEY) !== "0");
  const [showFilesPanel, setShowFilesPanel] = useState(false);
  const [showDetailsPanel, setShowDetailsPanel] = useState(false);
  const [newSessionOpen, setNewSessionOpen] = useState(false);
  const [newSessionBusy, setNewSessionBusy] = useState(false);
  const [newSessionDefaults, setNewSessionDefaults] = useState<NewSessionDefaults | null>(null);
  const [recentCwds, setRecentCwds] = useState<string[]>([]);
  const [tmuxAvailable, setTmuxAvailable] = useState(false);
  const [newSessionBackend, setNewSessionBackend] = useState<"codex" | "pi">("codex");
  const [newSessionCwd, setNewSessionCwd] = useState("");
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
  const [renameName, setRenameName] = useState("");
  const [renameBusy, setRenameBusy] = useState(false);
  const [renameError, setRenameError] = useState("");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [voiceSettings, setVoiceSettings] = useState<VoiceSettingsResponse | null>(null);
  const [voiceSettingsLoading, setVoiceSettingsLoading] = useState(false);
  const [voiceSettingsSaving, setVoiceSettingsSaving] = useState(false);
  const [voiceBaseUrl, setVoiceBaseUrl] = useState("");
  const [voiceApiKey, setVoiceApiKey] = useState("");
  const [voiceNarrationEnabled, setVoiceNarrationEnabled] = useState(false);
  const [notificationSnapshot, setNotificationSnapshot] = useState<NotificationSubscriptionsResponse | null>(null);
  const [desktopNotificationsEnabled, setDesktopNotificationsEnabled] = useState(() => readLocalStorage(DESKTOP_NOTIFICATIONS_KEY) === "1");
  const [mobilePushEnabled, setMobilePushEnabled] = useState(false);
  const [mobilePushEndpoint, setMobilePushEndpoint] = useState("");
  const [helpOpen, setHelpOpen] = useState(false);
  const [attachedFiles, setAttachedFiles] = useState(0);
  const [attachBusy, setAttachBusy] = useState(false);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsResponse | null>(null);
  const [detailsLoading, setDetailsLoading] = useState(false);
  const [fileEntries, setFileEntries] = useState<FileEntry[]>([]);
  const [fileSearchQuery, setFileSearchQuery] = useState("");
  const [fileSearchEntries, setFileSearchEntries] = useState<FileEntry[]>([]);
  const [fileSearchLoading, setFileSearchLoading] = useState(false);
  const [filesLoading, setFilesLoading] = useState(false);
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
  const fastPollUntilRef = useRef(0);
  const openRequestRef = useRef(0);
  const notificationFeedSinceRef = useRef(0);
  const shownNotificationIdsRef = useRef<Set<string>>(new Set());
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const composerInputRef = useRef<HTMLTextAreaElement | null>(null);
  const chatScrollRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef(true);
  const lastAutoScrollKeyRef = useRef("");

  const selectedSession = useMemo(
    () => sessions.find((session) => session.session_id === selectedSessionId) || null,
    [sessions, selectedSessionId],
  );
  const workspaceGroups = useMemo(() => {
    const groups = new Map<
      string,
      { key: string; cwd: string; sessions: SessionSummary[]; queueLen: number; busyCount: number; updatedTs: number }
    >();
    for (const session of sessions) {
      const key = workspaceKeyForSession(session);
      const group = groups.get(key) || { key, cwd: key, sessions: [], queueLen: 0, busyCount: 0, updatedTs: 0 };
      group.sessions.push(session);
      group.queueLen += Number(session.queue_len || 0);
      if (session.busy || sessionIsStarting(session)) group.busyCount += 1;
      group.updatedTs = Math.max(group.updatedTs, Number(session.updated_ts || session.start_ts || 0));
      groups.set(key, group);
    }
    return Array.from(groups.values()).sort((left, right) => {
      const leftSelected = left.sessions.some((session) => session.session_id === selectedSessionId) ? 1 : 0;
      const rightSelected = right.sessions.some((session) => session.session_id === selectedSessionId) ? 1 : 0;
      if (leftSelected !== rightSelected) return rightSelected - leftSelected;
      if (left.updatedTs !== right.updatedTs) return right.updatedTs - left.updatedTs;
      return workspaceTitle(left.cwd).localeCompare(workspaceTitle(right.cwd));
    });
  }, [selectedSessionId, sessions]);
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
  const visibleTranscript = useMemo(
    () => coalesceAdjacentAssistantEvents(showTools ? transcript : transcript.filter((event) => event.kind === "user" || event.kind === "assistant")),
    [showTools, transcript],
  );
  const visibleFileEntries = useMemo(
    () => (fileSearchQuery.trim() ? fileSearchEntries : fileEntries),
    [fileEntries, fileSearchEntries, fileSearchQuery],
  );
  const tmuxCommand = useMemo(() => tmuxAttachCommandForSession(selectedSession), [selectedSession]);
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
  const topSessionStatus = useMemo(() => {
    if (!selectedSession) return "No session";
    if (closingSession) return "closing";
    if (sending) return "sending";
    if (queueLen) return `queue ${queueLen}`;
    if (awaitingAssistantReply || busy || selectedSession.busy) return "working";
    if (sessionIsStarting(selectedSession)) return "starting";
    return "idle";
  }, [awaitingAssistantReply, busy, closingSession, queueLen, selectedSession, sending]);
  const topSessionStatusClass = topSessionStatus === "working" || topSessionStatus === "starting" ? "status-chip working" : "status-chip";

  function toggleTranscriptEvent(eventId: string) {
    setCollapsedEvents((current) => ({ ...current, [eventId]: current[eventId] === false }));
  }

  function toggleWorkspaceGroup(workspaceKey: string) {
    setCollapsedWorkspaces((current) => ({ ...current, [workspaceKey]: !current[workspaceKey] }));
  }

  function pushToast(text: string) {
    setToastText(text);
    window.setTimeout(() => {
      setToastText((current) => (current === text ? "" : current));
    }, 2200);
  }

  function selectSession(sessionId: string) {
    const changed = selectedSessionRef.current !== sessionId;
    selectedSessionRef.current = sessionId;
    if (changed) {
      stickToBottomRef.current = true;
      lastAutoScrollKeyRef.current = "";
    }
    setSelectedSessionId(sessionId);
    if (sessionId) {
      writeLocalStorage(SELECTED_SESSION_KEY, sessionId);
      writeSessionHash(sessionId);
    } else {
      writeLocalStorage(SELECTED_SESSION_KEY, null);
      writeSessionHash("");
    }
  }

  function focusComposerInput() {
    window.setTimeout(() => composerInputRef.current?.focus(), 0);
  }

  function updateChatScrollFollowState() {
    const element = chatScrollRef.current;
    if (!element) return;
    stickToBottomRef.current = isNearScrollBottom(element);
  }

  function applyRuntime(data: {
    live_cursor?: string | null;
    history_cursor?: string | null;
    has_older?: boolean;
    busy: boolean;
    queue_len: number;
    token?: Record<string, unknown> | null;
  }) {
    liveCursorRef.current = data.live_cursor ?? null;
    historyCursorRef.current = data.history_cursor ?? null;
    setLiveCursor(liveCursorRef.current);
    setHistoryCursor(historyCursorRef.current);
    setHasOlder(Boolean(data.has_older));
    setBusy(Boolean(data.busy));
    setQueueLen(Number.isFinite(Number(data.queue_len)) ? Number(data.queue_len) : 0);
    setTokenSummary(renderTokenSummary(data.token));
  }

  async function refreshSessions({ preserveSelection = true }: { preserveSelection?: boolean } = {}) {
    const payload = await api.fetchSessions();
    const ordered = sortSessions(payload.sessions || []);
    setSessions(ordered);
    setRecentCwds(Array.isArray(payload.recent_cwds) ? payload.recent_cwds : []);
    setNewSessionDefaults(payload.new_session_defaults || null);
    setTmuxAvailable(Boolean(payload.tmux_available));
    setErrorText("");
    if (payload.new_session_defaults) {
      const defaultBackend = payload.new_session_defaults.default_backend || "codex";
      if (!newSessionDefaults) {
        const backend = defaultBackend === "pi" ? "pi" : "codex";
        const backendDefaults = defaultsForBackend(payload.new_session_defaults, backend);
        setNewSessionBackend(backend);
        setNewSessionProvider(String(backendDefaults?.provider_choice || ""));
        setNewSessionModel(String(backendDefaults?.model || ""));
        setNewSessionReasoning(String(backendDefaults?.reasoning_effort || "high"));
        setNewSessionFast(String(backendDefaults?.service_tier || "").toLowerCase() === "fast");
      }
    }
    if (!ordered.length) {
      selectSession("");
      setTranscript([]);
      return;
    }
    const stored = readLocalStorage(SELECTED_SESSION_KEY) || sessionIdFromHash();
    const current = preserveSelection ? selectedSessionRef.current || stored : stored;
    const nextSelected = ordered.some((item) => item.session_id === current) ? current : ordered[0].session_id;
    selectSession(nextSelected);
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
    setLoadingText("Loading session…");
    try {
      const data = await api.fetchTail(sessionId, INIT_PAGE_LIMIT);
      if (requestId !== openRequestRef.current || selectedSessionRef.current !== sessionId) return;
      const nextEvents = normalizeEvents(data.events || []);
      setTranscript(nextEvents);
      setSessionLastLines((current) => ({ ...current, [sessionId]: previewFromEvents(nextEvents) || current[sessionId] || "" }));
      applyRuntime(data);
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
          return merged;
        });
      }
      applyRuntime(data);
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
    setLoadingOlder(true);
    try {
      const data = await api.fetchHistory(selectedSessionId, historyCursorRef.current, OLDER_PAGE_LIMIT);
      if (selectedSessionRef.current !== selectedSessionId) return;
      const older = normalizeEvents(data.events || []);
      setTranscript((current) => older.concat(current));
      historyCursorRef.current = data.history_cursor ?? null;
      setHistoryCursor(historyCursorRef.current);
      setHasOlder(Boolean(data.has_older));
      setBusy(Boolean(data.busy));
      setQueueLen(Number.isFinite(Number(data.queue_len)) ? Number(data.queue_len) : 0);
    } catch (error) {
      const status = error && typeof error === "object" && "status" in error ? Number((error as { status?: number }).status) : 0;
      if (status === 409) {
        await openSession(selectedSessionId);
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

  async function loadFiles(sessionId = selectedSessionRef.current) {
    if (!sessionId) return;
    const session = sessions.find((item) => item.session_id === sessionId) || null;
    setFilesLoading(true);
    let changedFiles: ChangedFilesResponse | null = null;
    try {
      changedFiles = await api.fetchChangedFiles(sessionId);
    } catch {
      changedFiles = null;
    }
    const entries = buildFileEntries(session, changedFiles);
    setFileEntries(entries);
    setFilesLoading(false);
    if (entries.length && !entries.some((entry) => entry.request_path === activeFilePath)) {
      void openFile(entries[0].request_path);
    }
    if (!entries.length) {
      setActiveFile(null);
      setActiveFilePath("");
    }
  }

  async function openFile(path: string) {
    if (!selectedSessionRef.current || !path) return;
    setActiveFilePath(path);
    try {
      const data = await api.readSessionFile(selectedSessionRef.current, path);
      if (selectedSessionRef.current !== selectedSessionId && selectedSessionId) return;
      setActiveFile(data);
      setErrorText("");
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to open file");
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
    const shouldQueue = Boolean(busy || selectedSession?.busy);
    let localEvent: UiTranscriptEvent | null = null;
    setComposerText("");
    setSending(true);
    setErrorText("");
    if (!shouldQueue) {
      localEvent = {
        id: `local-user-${Date.now()}`,
        kind: "user",
        ts: Date.now() / 1000,
        title: "User",
        body: text,
        meta: "sending",
      };
      setTranscript((current) => current.concat(localEvent as UiTranscriptEvent));
      setSessionLastLines((current) => ({ ...current, [sessionId]: eventTextPreview(localEvent as UiTranscriptEvent) }));
    }
    try {
      if (shouldQueue) {
        const response = await api.enqueueMessage(sessionId, text);
        setQueueLen(Number(response.queue_len || queueLen + 1));
        pushToast(`Queued${response.queue_len ? ` (${response.queue_len})` : ""}`);
        await refreshSessions();
      } else {
        await api.sendMessage(sessionId, text);
        if (localEvent) {
          setTranscript((current) => current.map((event) => (event.id === localEvent.id ? { ...event, meta: "sent" } : event)));
        }
        pushToast("Sent");
      }
      fastPollUntilRef.current = Date.now() + 5000;
      schedulePoll(0);
    } catch (error) {
      setComposerText(text);
      if (localEvent) {
        setTranscript((current) => current.map((event) => (event.id === localEvent.id ? { ...event, meta: "send failed" } : event)));
      }
      setErrorText(error instanceof Error ? error.message : "Unable to send message");
    } finally {
      setSending(false);
      focusComposerInput();
    }
  }

  async function handleCloseSession() {
    const sessionId = selectedSessionRef.current;
    if (!sessionId || closingSession) return;
    if (!window.confirm(`Close ${sessionDisplayName(selectedSession)}?`)) return;
    setClosingSession(true);
    try {
      await api.deleteSession(sessionId);
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
    if (!selectedSessionRef.current) return;
    const text = composerText.trim();
    if (!text) {
      setQueueOpen(true);
      void loadQueue(selectedSessionRef.current);
      return;
    }
    try {
      await api.enqueueMessage(selectedSessionRef.current, text);
      setComposerText("");
      pushToast("Queued");
      await refreshSessions();
      setQueueOpen(true);
      await loadQueue(selectedSessionRef.current);
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to queue message");
    }
  }

  async function handleInterrupt() {
    if (!selectedSessionRef.current) return;
    try {
      await api.interrupt(selectedSessionRef.current);
      fastPollUntilRef.current = Date.now() + 4000;
      pushToast("Interrupt sent");
      schedulePoll(0);
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to interrupt session");
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
    selectSession("");
    setTranscript([]);
  }

  function openRenameDialog() {
    if (!selectedSession) return;
    setRenameName(String(selectedSession.alias || sessionDisplayName(selectedSession) || ""));
    setRenameError("");
    setRenameOpen(true);
  }

  async function handleRenameSession() {
    const sessionId = selectedSessionRef.current;
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

  function openNewSessionDialog() {
    const defaults = defaultsForBackend(newSessionDefaults, newSessionBackend) || defaultsForBackend(newSessionDefaults, "codex");
    setNewSessionCwd(selectedSession?.cwd || recentCwds[0] || "");
    setNewSessionProvider(String(defaults?.provider_choice || ""));
    setNewSessionModel(String(defaults?.model || ""));
    setNewSessionReasoning(String(defaults?.reasoning_effort || "high"));
    setNewSessionFast(String(defaults?.service_tier || "").toLowerCase() === "fast");
    setNewSessionTmux(tmuxAvailable);
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
        create_in_tmux: tmuxAvailable && newSessionTmux,
        resume_session_id: newSessionResumeSelection?.session_id || null,
        worktree_branch: worktreeBranch,
      });
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
    try {
      const [voice, notifications] = await Promise.all([api.fetchVoiceSettings(), api.fetchNotificationSubscriptions()]);
      setVoiceSettings(voice);
      setVoiceBaseUrl(String(voice.tts_base_url || ""));
      setVoiceApiKey(String(voice.tts_api_key || ""));
      setVoiceNarrationEnabled(Boolean(voice.tts_enabled_for_narration));
      setNotificationSnapshot(notifications);
      await syncMobilePushState(notifications, voice.notifications.vapid_public_key);
      setErrorText("");
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : "Unable to load settings");
    } finally {
      setVoiceSettingsLoading(false);
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
    writeLocalStorage(DESKTOP_NOTIFICATIONS_KEY, desktopNotificationsEnabled ? "1" : null);
  }, [desktopNotificationsEnabled]);

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
    ].join(":");
    if (!scrollKey || scrollKey === lastAutoScrollKeyRef.current) return;
    lastAutoScrollKeyRef.current = scrollKey;
    if (!visibleTranscript.length || !stickToBottomRef.current) return;
    window.requestAnimationFrame(() => {
      const element = chatScrollRef.current;
      if (!element) return;
      element.scrollTop = element.scrollHeight;
      stickToBottomRef.current = true;
    });
  }, [selectedSessionId, visibleTranscript]);

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
    if (!showDetailsPanel || !selectedSessionId) return;
    void loadDiagnostics(selectedSessionId);
  }, [showDetailsPanel, selectedSessionId]);

  useEffect(() => {
    if (!showFilesPanel || !selectedSessionId) return;
    void loadFiles(selectedSessionId);
  }, [showFilesPanel, selectedSessionId, sessions]);

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

  return (
    <>
      <div className={`app${showFilesPanel || showDetailsPanel ? " withRail" : ""}`}>
        <aside className="sidebar">
          <header>
            <div className="title">
              <span className="sidebarLogoDot" />
              Codoxear Nova
            </div>
            <div className="actions">
              <button className="icon-btn" type="button" title="New session" onClick={() => openNewSessionDialog()}>
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
              <button className="icon-btn" type="button" title="Voice settings" onClick={() => setSettingsOpen(true)}>
                {icon("volume")}
              </button>
            </div>
          </header>
          <div className="sessions">
            {workspaceGroups.map((group) => {
              const hasSelected = group.sessions.some((session) => session.session_id === selectedSessionId);
              const collapsed = Boolean(collapsedWorkspaces[group.key] && !hasSelected);
              return (
                <section key={group.key} className={`workspaceGroup${hasSelected ? " active" : ""}`}>
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
                      {group.sessions.map((session) => (
                        <button
                          key={session.session_id}
                          className={`workspace${selectedSessionId === session.session_id ? " active" : ""}`}
                          onClick={() => selectSession(session.session_id)}
                        >
                          <div className="workspaceHeader">
                            <div className="workspaceTitleRow">
                              <div className="workspaceTitle">{sessionDisplayName(session)}</div>
                              <div className={`status-dot ${session.busy || sessionIsStarting(session) ? "running" : "idle"}`} />
                            </div>
                            <div className="workspacePath">{session.git_branch ? session.git_branch : String(session.agent_backend || "codex").toUpperCase()}</div>
                            <div className="workspaceMeta">
                              {String(session.agent_backend || "codex").toUpperCase()} · {sessionStatusText(session)} ·{" "}
                              {relativeAge(session.updated_ts)}
                            </div>
                          </div>
                          <div className="lastLine">{sessionLastLines[session.session_id] || session.session_id}</div>
                        </button>
                      ))}
                    </div>
                  ) : null}
                </section>
              );
            })}
          </div>
          <footer>
            <button type="button" onClick={() => setHelpOpen(true)}>
              {icon("help")}
              Help
            </button>
            <button type="button" onClick={() => setSettingsOpen(true)}>
              {icon("settings")}
              Settings
            </button>
            <button type="button" onClick={() => void handleLogout()}>
              {icon("logout")}
              Log out
            </button>
          </footer>
        </aside>

        <div className="main">
          <div className="topbar">
            <div className="pill">
              <button className="icon-btn" type="button" title="Sidebar structure now follows the legacy station" disabled>
                {icon("menu")}
              </button>
              <div className="titleWrap">
                <div className="titleRow">
                  <div id="threadTitle">{sessionDisplayName(selectedSession)}</div>
                  <div className="topMeta">
                    <span className="status-chip">{selectedSession ? baseName(selectedSession.cwd) : "No workspace"}</span>
                    <span className={topSessionStatusClass}>{topSessionStatus}</span>
                    {tokenSummary ? <span className="status-chip">{tokenSummary}</span> : null}
                  </div>
                </div>
              </div>
            </div>
            <div className="actions topActions">
              <button
                className="icon-btn"
                type="button"
                title={selectedSession ? "Rename session" : "No session selected"}
                disabled={!selectedSession}
                onClick={() => openRenameDialog()}
              >
                {icon("edit")}
              </button>
              <button
                className={`icon-btn${showTools ? " active" : ""}`}
                type="button"
                title={showTools ? "Hide tool calls" : "Show tool calls"}
                onClick={() => setShowTools((current) => !current)}
              >
                {icon("wrench")}
              </button>
              <button
                className="icon-btn danger"
                type="button"
                title={selectedSession ? "Close session" : "No session selected"}
                disabled={!selectedSession || closingSession}
                onClick={() => void handleCloseSession()}
              >
                {icon("trash")}
              </button>
              <button
                className={`icon-btn${tmuxCommand ? " active" : ""}`}
                type="button"
                title={tmuxCommand || "No tmux attach command for this session"}
                disabled={!tmuxCommand}
                onClick={() => void copyTmuxAttachCommand()}
              >
                {icon("terminal")}
              </button>
              <button
                className={`icon-btn${showFilesPanel ? " active" : ""}`}
                type="button"
                title="Files"
                onClick={() => setShowFilesPanel((current) => !current)}
              >
                {icon("file")}
              </button>
              <button
                className={`icon-btn${showDetailsPanel ? " active" : ""}`}
                type="button"
                title="Details"
                onClick={() => setShowDetailsPanel((current) => !current)}
              >
                {icon("info")}
              </button>
              <button className="icon-btn" type="button" title="Interrupt" onClick={() => void handleInterrupt()}>
                {icon("stop")}
              </button>
              <button
                className={`icon-btn${selectedSession?.harness_enabled ? " active" : ""}`}
                type="button"
                title="Harness mode"
                onClick={() => {
                  setHarnessOpen(true);
                  void loadHarness();
                }}
              >
                {icon("harness")}
              </button>
            </div>
          </div>

          <div className="toast muted">{toastText}</div>

          <div className="workspaceBody">
            <div className="chatWrap">
              <div className="chat" ref={chatScrollRef} onScroll={updateChatScrollFollowState}>
                <div className="chatInner">
                  {hasOlder ? (
                    <button className="olderBtn" type="button" disabled={loadingOlder} onClick={() => void loadOlder()}>
                      {loadingOlder ? "Loading older messages…" : "Load older messages"}
                    </button>
                  ) : null}
                  {visibleTranscript.map((event, index) => {
                    const collapsible = isCollapsibleEvent(event.kind);
                    const collapsed = Boolean(collapsible && collapsedEvents[event.id] !== false);
                    const showMessageSide = shouldShowMessageSide(visibleTranscript, index);
                    const showMessageFooter = shouldShowMessageFooter(visibleTranscript, index, collapsible);
                    return (
                      <article
                        key={event.id}
                        className={`msg ${eventClassName(event.kind)}${collapsible ? " is-collapsible" : ""}${collapsed ? " is-collapsed" : ""}`}
                        role={collapsible ? "button" : undefined}
                        tabIndex={collapsible ? 0 : undefined}
                        aria-expanded={collapsible ? !collapsed : undefined}
                        onClick={collapsible ? () => toggleTranscriptEvent(event.id) : undefined}
                        onKeyDown={
                          collapsible
                            ? (keyEvent) => {
                                if (keyEvent.key === "Enter" || keyEvent.key === " ") {
                                  keyEvent.preventDefault();
                                  toggleTranscriptEvent(event.id);
                                }
                              }
                            : undefined
                        }
                      >
                        <div className={`message-side${showMessageSide && !collapsible ? "" : " is-hidden"}`}>
                          {showMessageSide && !collapsible ? (
                            <>
                              <span className="message-kind">{event.title || event.kind.replace("_", " ")}</span>
                            </>
                          ) : null}
                        </div>
                        <div className="message-main">
                          {collapsed ? (
                            <div className="message-inline">
                              <span className="message-kind">{event.title || event.kind.replace("_", " ")}</span>
                              <span className="message-fold" aria-label="Show details" title="Show details" />
                              <span className="message-summary">{eventTextPreview(event) || "No details"}</span>
                            </div>
                          ) : (
                            <>
                              {collapsible ? (
                                <div className="message-inline is-open">
                                  <span className="message-kind">{event.title || event.kind.replace("_", " ")}</span>
                                  <span className="message-fold is-open" aria-label="Hide details" title="Hide details" />
                                </div>
                              ) : null}
                              {event.body ? <PretextParagraph text={event.body} className="message-body" /> : null}
                              {collapsible && event.meta ? <div className="message-meta">{event.meta}</div> : null}
                              {showMessageFooter ? (
                                <div className="message-footer">
                                  {event.meta ? <span className="message-meta">{event.meta}</span> : null}
                                  {event.ts ? <span className="message-time">{formatTime(event.ts)}</span> : null}
                                </div>
                              ) : null}
                            </>
                          )}
                        </div>
                      </article>
                    );
                  })}
                  {!visibleTranscript.length ? <div className="emptyState">No transcript yet for this session.</div> : null}
                </div>
              </div>
            </div>

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
                      <button className="secondaryBtn" type="button" disabled={!fileSearchQuery.trim()} onClick={() => void openFile(fileSearchQuery.trim())}>
                        Open
                      </button>
                    </form>
                    {filesLoading ? <div className="muted">Loading files…</div> : null}
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
                        <div className="muted">{fileSearchQuery.trim() ? "No matching files. Use Open to try this exact path." : "No tracked or changed files yet."}</div>
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
            <form
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
                  onInput={(event) => setComposerText((event.currentTarget as HTMLTextAreaElement).value)}
                  aria-label="Enter your instructions here"
                />
                {!composerText ? <div className="ph">Enter your instructions here</div> : null}
              </div>
              <button className="icon-btn" type="button" title="Queued messages" onClick={() => void handleQueueAction()}>
                {icon("queue")}
              </button>
              <button className="icon-btn primary" type="submit" title={sending ? "Sending…" : "Send"} disabled={sending || !selectedSessionId}>
                {icon("send")}
              </button>
            </form>
          </div>
        </div>
      </div>

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
                      const nextDefaults = defaultsForBackend(newSessionDefaults, backend);
                      setNewSessionBackend(backend);
                      setNewSessionProvider(String(nextDefaults?.provider_choice || ""));
                      setNewSessionModel(String(nextDefaults?.model || ""));
                      setNewSessionReasoning(String(nextDefaults?.reasoning_effort || "high"));
                      setNewSessionFast(String(nextDefaults?.service_tier || "").toLowerCase() === "fast");
                    }}
                  >
                    {backend.toUpperCase()}
                  </button>
                ))}
              </div>
              <label className="field">
                <span>CWD</span>
                <input
                  list="recent-cwds"
                  value={newSessionCwd}
                  onInput={(event) => setNewSessionCwd((event.currentTarget as HTMLInputElement).value)}
                  placeholder="/path/to/workspace"
                />
                <datalist id="recent-cwds">
                  {recentCwds.map((cwd) => (
                    <option key={cwd} value={cwd} />
                  ))}
                </datalist>
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
                      {(candidate.alias || candidate.first_user_message || candidate.session_id).trim()}
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
            <div className="formGrid">
              <label className="field">
                <span>Name</span>
                <input
                  value={renameName}
                  onInput={(event) => setRenameName((event.currentTarget as HTMLInputElement).value)}
                  placeholder={selectedSession ? sessionDisplayName(selectedSession) : "Session name"}
                  autoFocus
                />
              </label>
              {renameError ? <div className="error-inline">{renameError}</div> : null}
              <div className="modalActions">
                <button className="secondaryBtn" type="button" onClick={() => setRenameOpen(false)}>
                  Cancel
                </button>
                <button className="primary" type="button" disabled={renameBusy} onClick={() => void handleRenameSession()}>
                  {renameBusy ? "Saving..." : "Save"}
                </button>
              </div>
            </div>
          </div>
        </div>
      ) : null}

      {helpOpen ? (
        <div className="modalBackdrop" onClick={() => setHelpOpen(false)}>
          <div className="modalCard" onClick={(event) => event.stopPropagation()}>
            <div className="modalHeader">
              <div>Help</div>
              <button className="icon-btn" type="button" onClick={() => setHelpOpen(false)}>
                {icon("info")}
              </button>
            </div>
            <div className="helpGrid">
              <section>
                <div className="detailSectionHeader">Sending</div>
                <p>Use Send for an immediate prompt. Use the queue button to enqueue text or edit queued messages while a session is busy.</p>
              </section>
              <section>
                <div className="detailSectionHeader">Attachments</div>
                <p>The paperclip uploads a file to the selected session and injects an attachment line into the tmux/broker input.</p>
              </section>
              <section>
                <div className="detailSectionHeader">Tool Calls</div>
                <p>The wrench button toggles tool, tool result, and ask-user events without hiding normal user or assistant messages.</p>
              </section>
              <section>
                <div className="detailSectionHeader">Harness</div>
                <p>Harness mode automatically injects a configured request after idle periods until the remaining injection budget reaches zero.</p>
              </section>
              <section>
                <div className="detailSectionHeader">Notifications</div>
                <p>Desktop notifications poll the server feed in this browser. Mobile push uses the service worker and server-side web-push subscription.</p>
              </section>
            </div>
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
            {voiceSettingsLoading || !voiceSettings ? (
              <div className="muted">Loading settings…</div>
            ) : (
              <div className="formGrid">
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
                  Stream: {voiceSettings.audio.stream_url} · listeners {voiceSettings.audio.active_listener_count} · queued {voiceSettings.audio.queue_depth}
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
                  Server subscriptions: {notificationSnapshot?.subscriptions.length || 0} · enabled devices {voiceSettings.notifications.enabled_devices}
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
