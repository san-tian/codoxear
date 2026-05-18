import type { NewSessionDefaults, Schedule, ScheduleRun, SchedulesResponse, SessionSummary, ShareSet } from "./lib/types";

type ScheduleScope = "session" | "all";
type TargetMode = "existing_session" | "new_session_each_run" | "create_once_reuse";
type RuleKind = "once" | "daily" | "weekly" | "monthly" | "interval";

export type ScheduleDraft = {
  name: string;
  message: string;
  alias: string;
  targetMode: TargetMode;
  sessionId: string;
  ruleKind: RuleKind;
  nextLocal: string;
  intervalMinutes: string;
  timeoutMode: "default" | "custom" | "none";
  timeoutMinutes: string;
  backend: "codex" | "pi";
  cwd: string;
  createInTmux: boolean;
};

export function defaultScheduleDraft(sessionId = "", cwd = ""): ScheduleDraft {
  const next = new Date(Date.now() + 60 * 60 * 1000);
  next.setSeconds(0, 0);
  return {
    name: "Scheduled run",
    message: "",
    alias: "",
    targetMode: "existing_session",
    sessionId,
    ruleKind: "once",
    nextLocal: toLocalInputValue(next),
    intervalMinutes: "60",
    timeoutMode: "default",
    timeoutMinutes: "240",
    backend: "codex",
    cwd,
    createInTmux: true,
  };
}

export function buildSchedulePayload(draft: ScheduleDraft, defaults: NewSessionDefaults | null, shareOnly = false): Record<string, unknown> {
  const message = draft.message.trim();
  const name = draft.name.trim() || "Scheduled run";
  const nextRunAt = draft.ruleKind === "interval" ? Date.now() / 1000 + Number(draft.intervalMinutes || 60) * 60 : localInputToEpoch(draft.nextLocal);
  const rule =
    draft.ruleKind === "interval"
      ? { kind: "interval", interval_seconds: Math.max(60, Number(draft.intervalMinutes || 60) * 60), next_run_at: nextRunAt }
      : { kind: draft.ruleKind, next_run_at: nextRunAt };
  const timeout =
    draft.timeoutMode === "none"
      ? null
      : draft.timeoutMode === "custom"
        ? Math.max(1, Number(draft.timeoutMinutes || 240))
        : 240;
  const target =
    shareOnly || draft.targetMode === "existing_session"
      ? { mode: "existing_session", session_id: draft.sessionId }
      : {
          mode: draft.targetMode,
          session_template: newSessionTemplate(draft, defaults),
        };
  return {
    name,
    enabled: true,
    target,
    rule,
    next_run_at: nextRunAt,
    message_template: message,
    alias_template: draft.alias.trim() || null,
    busy_policy: "enqueue",
    timeout_minutes: timeout,
  };
}

export function ScheduleModal(props: {
  open: boolean;
  owner: boolean;
  share?: ShareSet | null;
  selectedSessionId: string;
  sessions: SessionSummary[];
  defaults: NewSessionDefaults | null;
  scope: ScheduleScope;
  loading: boolean;
  saving: boolean;
  errorText: string;
  schedules: Schedule[];
  runs: ScheduleRun[];
  draft: ScheduleDraft;
  onClose: () => void;
  onScopeChange: (scope: ScheduleScope) => void;
  onDraftChange: (draft: ScheduleDraft) => void;
  onCreate: () => void | Promise<void>;
  onRunNow: (scheduleId: string) => void | Promise<void>;
  onToggle: (scheduleId: string, enabled: boolean) => void | Promise<void>;
  onDelete: (scheduleId: string) => void | Promise<void>;
  onMarkDone: (scheduleId: string, runId: string) => void | Promise<void>;
}) {
  if (!props.open) return null;
  const visibleSchedules =
    props.scope === "session" && props.selectedSessionId
      ? props.schedules.filter((schedule) => scheduleSessionId(schedule) === props.selectedSessionId)
      : props.schedules;
  const shareOnly = !props.owner;
  const sessionOptions = props.owner ? props.sessions : shareSessionsToOptions(props.share);
  return (
    <div className="modalBackdrop" onClick={props.onClose}>
      <div className="modalCard scheduleModal" onClick={(event) => event.stopPropagation()}>
        <div className="modalHeader">
          <div>Schedules</div>
          <button className="icon-btn" type="button" onClick={props.onClose} aria-label="Close">
            x
          </button>
        </div>
        <div className="scheduleToolbar">
          <div className="backendTabs scheduleTabs">
            <button className={`backendTab${props.scope === "session" ? " active" : ""}`} type="button" onClick={() => props.onScopeChange("session")}>
              Current session
            </button>
            <button className={`backendTab${props.scope === "all" ? " active" : ""}`} type="button" onClick={() => props.onScopeChange("all")}>
              All schedules
            </button>
          </div>
          {props.loading ? <span className="muted">Loading...</span> : null}
        </div>
        {props.errorText ? <div className="error-inline">{props.errorText}</div> : null}

        <div className="scheduleLayout">
          <section className="scheduleList" aria-label="Schedules">
            {visibleSchedules.length ? (
              visibleSchedules.map((schedule) => {
                const recentRuns = props.runs
                  .filter((run) => run.schedule_id === schedule.schedule_id)
                  .sort((a, b) => Number(b.created_at || 0) - Number(a.created_at || 0))
                  .slice(0, 3);
                const activeRun = recentRuns.find((run) => run.status === "queued" || run.status === "running");
                return (
                  <article className="scheduleItem" key={schedule.schedule_id}>
                    <div className="scheduleItemHeader">
                      <div>
                        <div className="scheduleName">{schedule.name}</div>
                        <div className="scheduleMeta">
                          {targetLabel(schedule, sessionOptions)} / {schedule.rule.kind} / {nextRunLabel(schedule)}
                        </div>
                      </div>
                      <span className={`status-chip${schedule.enabled ? "" : " waiting"}`}>{schedule.status}</span>
                    </div>
                    <div className="scheduleMessage">{schedule.message_template}</div>
                    {recentRuns.length ? (
                      <div className="scheduleRuns">
                        {recentRuns.map((run) => (
                          <div className="scheduleRun" key={run.run_id}>
                            <span>{run.status}</span>
                            <span>{formatDateTime(run.created_at)}</span>
                            {run.assistant_preview ? <span>{run.assistant_preview}</span> : run.error ? <span>{run.error}</span> : null}
                            {run.status === "queued" || run.status === "running" ? (
                              <button type="button" onClick={() => void props.onMarkDone(schedule.schedule_id, run.run_id)}>
                                Mark as done
                              </button>
                            ) : null}
                          </div>
                        ))}
                      </div>
                    ) : null}
                    <div className="scheduleActions">
                      <button type="button" disabled={Boolean(activeRun)} onClick={() => void props.onRunNow(schedule.schedule_id)}>
                        Run now
                      </button>
                      <button type="button" onClick={() => void props.onToggle(schedule.schedule_id, !schedule.enabled)}>
                        {schedule.enabled ? "Disable" : "Enable"}
                      </button>
                      <button className="danger" type="button" onClick={() => void props.onDelete(schedule.schedule_id)}>
                        Delete
                      </button>
                    </div>
                  </article>
                );
              })
            ) : (
              <div className="emptyState">No schedules.</div>
            )}
          </section>

          <form
            className="scheduleEditor"
            onSubmit={(event) => {
              event.preventDefault();
              void props.onCreate();
            }}
          >
            <div className="detailSectionHeader">New schedule</div>
            <label className="field">
              Name
              <input value={props.draft.name} onInput={(event) => props.onDraftChange({ ...props.draft, name: event.currentTarget.value })} />
            </label>
            <label className="field">
              Message
              <textarea rows={5} value={props.draft.message} onInput={(event) => props.onDraftChange({ ...props.draft, message: event.currentTarget.value })} />
            </label>
            <label className="field">
              Alias template
              <input value={props.draft.alias} placeholder={props.draft.targetMode === "new_session_each_run" ? "Run {run_number} {datetime}" : ""} onInput={(event) => props.onDraftChange({ ...props.draft, alias: event.currentTarget.value })} />
            </label>
            {props.owner ? (
              <div className="backendTabs scheduleTabs">
                <button className={`backendTab${props.draft.targetMode === "existing_session" ? " active" : ""}`} type="button" onClick={() => props.onDraftChange({ ...props.draft, targetMode: "existing_session" })}>
                  Existing
                </button>
                <button className={`backendTab${props.draft.targetMode === "new_session_each_run" ? " active" : ""}`} type="button" onClick={() => props.onDraftChange({ ...props.draft, targetMode: "new_session_each_run" })}>
                  New each run
                </button>
                <button className={`backendTab${props.draft.targetMode === "create_once_reuse" ? " active" : ""}`} type="button" onClick={() => props.onDraftChange({ ...props.draft, targetMode: "create_once_reuse" })}>
                  Create once
                </button>
              </div>
            ) : null}
            {(shareOnly || props.draft.targetMode === "existing_session") ? (
              <label className="field">
                Session
                <select value={props.draft.sessionId} onChange={(event) => props.onDraftChange({ ...props.draft, sessionId: event.currentTarget.value })}>
                  {sessionOptions.map((session) => (
                    <option key={session.session_id} value={session.session_id}>
                      {sessionName(session)}
                    </option>
                  ))}
                </select>
              </label>
            ) : (
              <div className="twoCol">
                <label className="field">
                  Backend
                  <select value={props.draft.backend} onChange={(event) => props.onDraftChange({ ...props.draft, backend: event.currentTarget.value as "codex" | "pi" })}>
                    <option value="codex">Codex</option>
                    <option value="pi">Pi</option>
                  </select>
                </label>
                <label className="field">
                  CWD
                  <input value={props.draft.cwd} onInput={(event) => props.onDraftChange({ ...props.draft, cwd: event.currentTarget.value })} />
                </label>
              </div>
            )}
            <div className="twoCol">
              <label className="field">
                Rule
                <select value={props.draft.ruleKind} onChange={(event) => props.onDraftChange({ ...props.draft, ruleKind: event.currentTarget.value as RuleKind })}>
                  <option value="once">Once</option>
                  <option value="daily">Daily</option>
                  <option value="weekly">Weekly</option>
                  <option value="monthly">Monthly</option>
                  <option value="interval">Interval</option>
                </select>
              </label>
              {props.draft.ruleKind === "interval" ? (
                <label className="field">
                  Every minutes
                  <input type="number" min="1" value={props.draft.intervalMinutes} onInput={(event) => props.onDraftChange({ ...props.draft, intervalMinutes: event.currentTarget.value })} />
                </label>
              ) : (
                <label className="field">
                  Next run
                  <input type="datetime-local" value={props.draft.nextLocal} onInput={(event) => props.onDraftChange({ ...props.draft, nextLocal: event.currentTarget.value })} />
                </label>
              )}
            </div>
            <div className="twoCol">
              <label className="field">
                Timeout
                <select value={props.draft.timeoutMode} onChange={(event) => props.onDraftChange({ ...props.draft, timeoutMode: event.currentTarget.value as ScheduleDraft["timeoutMode"] })}>
                  <option value="default">240 minutes</option>
                  <option value="custom">Custom</option>
                  <option value="none">No timeout</option>
                </select>
              </label>
              {props.draft.timeoutMode === "custom" ? (
                <label className="field">
                  Minutes
                  <input type="number" min="1" value={props.draft.timeoutMinutes} onInput={(event) => props.onDraftChange({ ...props.draft, timeoutMinutes: event.currentTarget.value })} />
                </label>
              ) : null}
            </div>
            <div className="modalActions">
              <button className="primary" type="submit" disabled={props.saving || !props.draft.message.trim() || !props.draft.sessionId && (shareOnly || props.draft.targetMode === "existing_session")}>
                {props.saving ? "Creating..." : "Create schedule"}
              </button>
            </div>
          </form>
        </div>
      </div>
    </div>
  );
}

function newSessionTemplate(draft: ScheduleDraft, defaults: NewSessionDefaults | null): Record<string, unknown> {
  const backendDefaults = defaults?.backends?.[draft.backend];
  return {
    cwd: draft.cwd,
    workspace_cwd: draft.cwd,
    agent_backend: draft.backend,
    model_provider: backendDefaults?.model_provider || null,
    preferred_auth_method: backendDefaults?.preferred_auth_method || null,
    model: backendDefaults?.model || null,
    reasoning_effort: backendDefaults?.reasoning_effort || "high",
    service_tier: backendDefaults?.service_tier || null,
    create_in_tmux: draft.createInTmux,
  };
}

function scheduleSessionId(schedule: Schedule): string {
  if (schedule.target.mode === "existing_session") return schedule.target.session_id;
  if (schedule.target.mode === "create_once_reuse") return schedule.target.created_session_id || "";
  return "";
}

function shareSessionsToOptions(share?: ShareSet | null): SessionSummary[] {
  return (share?.sessions || []).map((session) => ({
    session_id: session.session_id,
    thread_id: null,
    pid: 0,
    broker_pid: 0,
    agent_backend: session.agent_backend || "codex",
    owned: false,
    transport: null,
    cwd: session.cwd || "",
    workspace_cwd: session.workspace_cwd || null,
    start_ts: 0,
    updated_ts: session.updated_ts || session.added_ts || 0,
    log_path: null,
    queue_len: session.queue_len || 0,
    busy: Boolean(session.busy),
    terminal_prompt: null,
    harness_enabled: false,
    harness_cooldown_minutes: 0,
    harness_remaining_injections: 0,
    alias: session.nickname || "",
    files: [],
    git_branch: null,
    model_provider: null,
    preferred_auth_method: null,
    provider_choice: null,
    model: null,
    reasoning_effort: null,
    service_tier: null,
    tmux_session: null,
    tmux_window: null,
    priority_offset: 0,
    snooze_until: null,
    dependency_session_id: null,
    time_priority: 0,
    base_priority: 0,
    final_priority: 0,
    blocked: false,
    snoozed: false,
  }));
}

function sessionName(session: SessionSummary): string {
  return session.alias || session.session_id;
}

function targetLabel(schedule: Schedule, sessions: SessionSummary[]): string {
  const id = scheduleSessionId(schedule);
  const session = sessions.find((item) => item.session_id === id);
  if (schedule.target.mode === "new_session_each_run") return "new session each run";
  if (schedule.target.mode === "create_once_reuse") return session ? `create once: ${sessionName(session)}` : "create once";
  return session ? sessionName(session) : id || "session";
}

function nextRunLabel(schedule: Schedule): string {
  if (!schedule.next_run_at) return "no next run";
  return formatDateTime(schedule.next_run_at);
}

function formatDateTime(ts?: number | null): string {
  if (!ts) return "";
  return new Date(ts * 1000).toLocaleString();
}

function toLocalInputValue(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function localInputToEpoch(value: string): number {
  const date = value ? new Date(value) : new Date(Date.now() + 60 * 60 * 1000);
  return Math.floor(date.getTime() / 1000);
}

export function applySchedulesResponse(response: SchedulesResponse, setSchedules: (value: Schedule[]) => void, setRuns: (value: ScheduleRun[]) => void) {
  setSchedules(response.schedules || []);
  setRuns(response.runs || []);
}
