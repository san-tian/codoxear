import { TranscriptEventRow, WorkingIndicator } from "./lib/transcript";
import type { FileReadResponse, QueueItem, ShareFilesResponse, ShareSet, TerminalPrompt, UiTranscriptEvent } from "./lib/types";

function baseName(path: string) {
  const parts = String(path || "").split("/").filter(Boolean);
  return parts.length ? parts[parts.length - 1] : path;
}

function relativeAge(ts: number) {
  if (!(ts > 0)) return "";
  const delta = Math.max(0, Math.floor(Date.now() / 1000 - ts));
  if (delta < 60) return "just now";
  if (delta < 3600) return `${Math.max(1, Math.floor(delta / 60))}m ago`;
  if (delta < 86400) return `${Math.max(1, Math.floor(delta / 3600))}h ago`;
  return `${Math.max(1, Math.floor(delta / 86400))}d ago`;
}

function shareIcon(name: "send" | "stop" | "replace" | "keep" | "schedule" | "tools" | "queue" | "up" | "down" | "trash") {
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
  if (name === "stop") {
    return (
      <svg {...common}>
        <rect x="4" y="4" width="8" height="8" rx="1.4" />
      </svg>
    );
  }
  if (name === "replace") {
    return (
      <svg {...common}>
        <path d="m8 2.7 1.45 3 3.25.47-2.35 2.3.55 3.25L8 10.18 5.1 11.72l.55-3.25-2.35-2.3 3.25-.47L8 2.7Z" />
      </svg>
    );
  }
  if (name === "keep") {
    return (
      <svg {...common}>
        <path d="M4.5 4.5 11.5 11.5" />
        <path d="M11.5 4.5 4.5 11.5" />
      </svg>
    );
  }
  if (name === "schedule") {
    return (
      <svg {...common}>
        <rect x="3" y="3.5" width="10" height="9.5" rx="1.4" />
        <path d="M5.5 2.5v2" />
        <path d="M10.5 2.5v2" />
        <path d="M3 6.5h10" />
        <path d="M8 8.4v2.3l1.6.8" />
      </svg>
    );
  }
  if (name === "tools") {
    return (
      <svg {...common}>
        <path d="M10.7 2.8a3.1 3.1 0 0 0 2.5 3.7L6.1 13.6a1.5 1.5 0 0 1-2.1-2.1l7.1-7.1A3.1 3.1 0 0 0 10.7 2.8Z" />
        <path d="M4.4 11.6 5.9 13.1" />
      </svg>
    );
  }
  if (name === "queue") {
    return (
      <svg {...common}>
        <path d="M4 4.5h8" />
        <path d="M4 8h8" />
        <path d="M4 11.5h5" />
      </svg>
    );
  }
  if (name === "up") {
    return (
      <svg {...common}>
        <path d="m4.5 9.5 3.5-3.5 3.5 3.5" />
      </svg>
    );
  }
  if (name === "down") {
    return (
      <svg {...common}>
        <path d="m4.5 6.5 3.5 3.5 3.5-3.5" />
      </svg>
    );
  }
  if (name === "trash") {
    return (
      <svg {...common}>
        <path d="M3.8 4.8h8.4" />
        <path d="M6.3 4.8v-1h3.4v1" />
        <path d="m5.2 4.8.5 7h4.6l.5-7" />
      </svg>
    );
  }
  return (
    <svg {...common}>
      <path d="M3 8h8.5" />
      <path d="M8.8 4.5 12.3 8l-3.5 3.5" />
    </svg>
  );
}

export function ShareLoginScreen(props: {
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

export function ShareWorkspace(props: {
  share: ShareSet;
  sessionId: string;
  transcript: UiTranscriptEvent[];
  files: ShareFilesResponse | null;
  selectedFilePath: string;
  selectedFile: FileReadResponse | null;
  showTools: boolean;
  loading: boolean;
  loadingOlder: boolean;
  hasOlder: boolean;
  busy: boolean;
  queueLen: number;
  queueItems: QueueItem[];
  queueDrafts: Record<string, string>;
  queueLoading: boolean;
  queueOpen: boolean;
  terminalPrompt: TerminalPrompt | null;
  terminalPromptSending: boolean;
  errorText: string;
  sendText: string;
  canSend: boolean;
  onSessionChange: (sessionId: string) => void;
  onSendTextChange: (value: string) => void;
  onSend: () => void | Promise<void>;
  onEnqueue: () => void | Promise<void>;
  onInterrupt: () => void | Promise<void>;
  onTerminalPromptResponse: (value: string) => void | Promise<void>;
  onLoadOlder: () => void | Promise<void>;
  onSelectFile: (path: string) => void;
  onOpenMentionedFile: (path: string) => void | Promise<void>;
  onCopyText: (event: UiTranscriptEvent) => void | Promise<void>;
  isEventCollapsed: (event: UiTranscriptEvent) => boolean;
  onToggleEvent: (event: UiTranscriptEvent) => void;
  onToggleTools: () => void;
  onOpenQueue: () => void;
  onCloseQueue: () => void;
  onQueueDraftChange: (itemId: string, value: string) => void;
  onSaveQueueItem: (itemId: string) => void | Promise<void>;
  onDeleteQueueItem: (itemId: string) => void | Promise<void>;
  onMoveQueueItem: (itemId: string, toIndex: number) => void | Promise<void>;
  onOpenSchedules: () => void;
}) {
  const session = props.share.sessions.find((item) => item.session_id === props.sessionId) || props.share.sessions[0] || null;
  const shareFiles = props.share.allow_files ? props.files?.files || [] : [];
  const showFileRail = props.share.allow_files && (shareFiles.length > 0 || Boolean(props.selectedFile));
  const shareWorking = Boolean(props.busy || session?.busy);
  const shareQueueLen = Number(props.queueLen || session?.queue_len || 0);
  const shareWaiting = Boolean(!shareWorking && shareQueueLen > 0);
  const shareStatus = shareWorking ? "working" : shareQueueLen ? `queue ${shareQueueLen}` : "idle";
  const shareStatusClass = shareWorking ? "status-chip working" : shareWaiting ? "status-chip waiting" : "status-chip";
  const shareWorkingIndicatorLabel = shareWorking ? "Working" : shareWaiting ? "Waiting" : "";
  const queuePreviewItems = props.queueItems.slice(0, 3);
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
          {props.share.sessions.map((item) => {
            const isCurrentSession = item.session_id === props.sessionId;
            const itemBusy = isCurrentSession ? Boolean(props.busy || item.busy) : Boolean(item.busy);
            const itemQueueLen = isCurrentSession
              ? Math.max(Number(props.queueLen || 0), Number(item.queue_len || 0))
              : Number(item.queue_len || 0);
            const itemStatus = itemBusy ? "working" : itemQueueLen ? `queue ${itemQueueLen}` : "idle";
            const itemDotClass = itemBusy ? "running" : itemQueueLen ? "waiting" : "idle";
            return (
              <button
                key={item.session_id}
                className={`workspaceSelect shareSession${isCurrentSession ? " active" : ""}`}
                type="button"
                onClick={() => props.onSessionChange(item.session_id)}
              >
                <div className="workspaceHeader">
                  <div className="workspaceTitleRow">
                    <div className="workspaceTitle">{item.nickname || item.session_id}</div>
                    <span className={`status-dot ${itemDotClass}`} title={itemStatus} aria-label={itemStatus} />
                  </div>
                  <div className="workspacePath">{item.workspace_cwd || item.cwd || item.session_id}</div>
                  <div className="workspaceMeta">
                    {String(item.agent_backend || "codex").toUpperCase()} / {itemStatus} / {relativeAge(item.updated_ts || item.added_ts)}
                  </div>
                </div>
              </button>
            );
          })}
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
                  {session?.cwd ? <span className="status-chip" title={session.cwd}>{baseName(session.workspace_cwd || session.cwd)}</span> : null}
                  <span className={shareStatusClass}>{shareStatus}</span>
                </div>
              </div>
            </div>
          </div>
          <div className="actions topActions">
            <button className="icon-btn" type="button" title="Queued messages" disabled={!props.sessionId} onClick={props.onOpenQueue}>
              {shareIcon("queue")}
            </button>
            <button
              className={`icon-btn${props.showTools ? " active" : ""}`}
              type="button"
              title={props.showTools ? "Hide tools and narration" : "Show tools and narration"}
              aria-pressed={props.showTools}
              onClick={props.onToggleTools}
            >
              {shareIcon("tools")}
            </button>
            <button className="icon-btn" type="button" title="Schedules" disabled={!props.sessionId || props.loading} onClick={props.onOpenSchedules}>
              {shareIcon("schedule")}
            </button>
            <button className="icon-btn" type="button" title="Interrupt" disabled={!props.share.allow_interrupt || !props.sessionId || props.loading} onClick={() => void props.onInterrupt()}>
              {shareIcon("stop")}
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
                      {props.loadingOlder ? "Loading older messages..." : "Load older messages"}
                    </button>
                  ) : null}
                  {props.transcript.map((event, index) => (
                    <TranscriptEventRow
                      key={event.id}
                      event={event}
                      events={props.transcript}
                      index={index}
                      collapsed={props.isEventCollapsed(event)}
                      onToggle={props.onToggleEvent}
                      onCopyText={props.onCopyText}
                      onOpenPath={props.onOpenMentionedFile}
                      attachmentHref={(path) =>
                        `/share/${encodeURIComponent(props.share.share_id)}/sessions/${encodeURIComponent(props.sessionId)}/file/download?path=${encodeURIComponent(path)}`
                      }
                    />
                  ))}
                  {props.terminalPrompt ? (
                    <div className="terminalPromptPanel" role="group" aria-label="Terminal prompt">
                      <div className="terminalPromptText">{props.terminalPrompt.message || "The terminal is waiting for confirmation."}</div>
                      <div className="terminalPromptActions">
                        {props.terminalPrompt.choices.map((choice) => (
                          <button
                            className={`terminalPromptBtn ${choice.value === "replace" ? "primary" : ""}`}
                            type="button"
                            key={choice.value}
                            title={choice.description || choice.label}
                            disabled={props.terminalPromptSending}
                            onClick={() => void props.onTerminalPromptResponse(choice.value)}
                          >
                            {shareIcon(choice.value === "replace" ? "replace" : "keep")}
                            <span>{choice.label}</span>
                          </button>
                        ))}
                      </div>
                    </div>
                  ) : null}
                  {shareWorkingIndicatorLabel ? <WorkingIndicator label={shareWorkingIndicatorLabel} tone={shareWaiting ? "waiting" : "working"} /> : null}
                  {!props.transcript.length && !shareWorkingIndicatorLabel ? <div className="emptyState">No transcript yet for this shared session.</div> : null}
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
          {shareQueueLen > 0 ? (
            <button className="queuePreview" type="button" onClick={props.onOpenQueue}>
              <span className="queuePreviewHeader">
                <span>Queued messages</span>
                <span>{props.queueLoading ? "loading" : `${shareQueueLen}`}</span>
              </span>
              <span className="queuePreviewList">
                {queuePreviewItems.map((item, index) => (
                  <span className="queuePreviewItem" key={item.id}>
                    <span>{index + 1}</span>
                    <span>{item.text}</span>
                  </span>
                ))}
                {!queuePreviewItems.length ? <span className="queuePreviewEmpty">Loading queued messages...</span> : null}
                {shareQueueLen > queuePreviewItems.length ? <span className="queuePreviewMore">+{shareQueueLen - queuePreviewItems.length} more</span> : null}
              </span>
            </button>
          ) : null}
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
            <button className="icon-btn" type="button" title="Queue message" disabled={!props.canSend || props.loading} onClick={() => void props.onEnqueue()}>
              {shareIcon("queue")}
            </button>
            <button className="icon-btn primary" type="submit" title={props.loading ? "Sending..." : "Send"} disabled={!props.canSend || props.loading}>
              {shareIcon("send")}
            </button>
          </form>
        </div>
        {props.errorText ? <div className="toast muted">{props.errorText}</div> : null}
      </div>
      {props.queueOpen ? (
        <div className="modalBackdrop" onClick={props.onCloseQueue}>
          <div className="modalCard" onClick={(event) => event.stopPropagation()}>
            <div className="modalHeader">
              <div>Queued messages</div>
              <button className="icon-btn" type="button" onClick={props.onCloseQueue}>
                {shareIcon("keep")}
              </button>
            </div>
            {props.queueLoading ? <div className="muted">Loading queue...</div> : null}
            <div className="queueList">
              {props.queueItems.map((item, index) => (
                <div className="queueItem" key={item.id}>
                  <textarea
                    className="queueText"
                    value={props.queueDrafts[item.id] ?? item.text}
                    onInput={(event) => props.onQueueDraftChange(item.id, (event.currentTarget as HTMLTextAreaElement).value)}
                  />
                  <div className="queueActions">
                    <button className="icon-btn" type="button" disabled={index === 0 || item.sending} onClick={() => void props.onMoveQueueItem(item.id, index - 1)}>
                      {shareIcon("up")}
                    </button>
                    <button className="icon-btn" type="button" disabled={index === props.queueItems.length - 1 || item.sending} onClick={() => void props.onMoveQueueItem(item.id, index + 1)}>
                      {shareIcon("down")}
                    </button>
                    <button className="secondaryBtn" type="button" onClick={() => void props.onSaveQueueItem(item.id)}>
                      Save
                    </button>
                    <button className="icon-btn danger" type="button" onClick={() => void props.onDeleteQueueItem(item.id)}>
                      {shareIcon("trash")}
                    </button>
                  </div>
                </div>
              ))}
              {!props.queueLoading && !props.queueItems.length ? <div className="muted">No queued messages.</div> : null}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
