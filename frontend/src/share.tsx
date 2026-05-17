import { TranscriptEventRow, WorkingIndicator } from "./lib/transcript";
import type { FileReadResponse, ShareFilesResponse, ShareSet, TerminalPrompt, UiTranscriptEvent } from "./lib/types";

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

function shareIcon(name: "send" | "stop" | "replace" | "keep") {
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
  loading: boolean;
  loadingOlder: boolean;
  hasOlder: boolean;
  busy: boolean;
  queueLen: number;
  terminalPrompt: TerminalPrompt | null;
  terminalPromptSending: boolean;
  errorText: string;
  sendText: string;
  canSend: boolean;
  onSessionChange: (sessionId: string) => void;
  onSendTextChange: (value: string) => void;
  onSend: () => void | Promise<void>;
  onInterrupt: () => void | Promise<void>;
  onTerminalPromptResponse: (value: string) => void | Promise<void>;
  onLoadOlder: () => void | Promise<void>;
  onSelectFile: (path: string) => void;
  onOpenMentionedFile: (path: string) => void | Promise<void>;
  onCopyText: (event: UiTranscriptEvent) => void | Promise<void>;
  isEventCollapsed: (event: UiTranscriptEvent) => boolean;
  onToggleEvent: (event: UiTranscriptEvent) => void;
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
                <div className="workspacePath">{item.workspace_cwd || item.cwd || item.session_id}</div>
                <div className="workspaceMeta">
                  {String(item.agent_backend || "codex").toUpperCase()} / {item.busy ? "working" : item.queue_len ? `queue ${item.queue_len}` : "idle"} / {relativeAge(item.updated_ts || item.added_ts)}
                </div>
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
                  {session?.cwd ? <span className="status-chip" title={session.cwd}>{baseName(session.workspace_cwd || session.cwd)}</span> : null}
                  <span className={shareStatusClass}>{shareStatus}</span>
                </div>
              </div>
            </div>
          </div>
          <div className="actions topActions">
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
            <button className="icon-btn primary" type="submit" title={props.loading ? "Sending..." : "Send"} disabled={!props.canSend || props.loading}>
              {shareIcon("send")}
            </button>
          </form>
        </div>
        {props.errorText ? <div className="toast muted">{props.errorText}</div> : null}
      </div>
    </div>
  );
}
