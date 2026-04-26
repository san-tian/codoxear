import { useState } from "preact/hooks";
import { MarkdownBlock } from "./markdown";
import type { AskUserOption, AskUserOptionInput, RawChatEvent, TranscriptExtensionItem, UiAskUserQuestion, UiTranscriptEvent } from "./types";

function formatTime(ts: number | null) {
  if (!(typeof ts === "number" && Number.isFinite(ts) && ts > 0)) return "";
  return new Date(ts * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export function isCollapsibleEvent(kind: UiTranscriptEvent["kind"]) {
  return kind === "tool" || kind === "tool_result" || kind === "ask_user";
}

export function eventTextPreview(event: UiTranscriptEvent) {
  const body = String(event.body || "").trim();
  if (!body) return isCollapsibleEvent(event.kind) ? "" : event.title || "";
  const preview = body.replace(/\s+/g, " ").slice(0, 160);
  if (!isCollapsibleEvent(event.kind) || !event.title) return preview;
  return stripRepeatedTitlePrefix(preview, event.title);
}

export function stripRepeatedTitlePrefix(preview: string, title: string) {
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

function normalizeAskOption(option: AskUserOptionInput, index: number): AskUserOption {
  if (typeof option === "string") {
    return {
      label: option,
      value: option,
      description: "",
    };
  }
  const label = option.label || option.title || option.value || `Option ${index + 1}`;
  return {
    label,
    value: option.value || option.title || label,
    description: option.description || "",
  };
}

function normalizeAskOptions(options: unknown): AskUserOption[] {
  if (!Array.isArray(options)) return [];
  return options
    .filter((option): option is AskUserOptionInput => typeof option === "string" || Boolean(option && typeof option === "object"))
    .map((option, index) => normalizeAskOption(option, index))
    .filter((option) => option.label.trim() && option.value.trim());
}

function normalizeAskQuestions(questions: unknown): UiAskUserQuestion[] {
  if (!Array.isArray(questions)) return [];
  const out: UiAskUserQuestion[] = [];
  questions.forEach((item) => {
    if (!item || typeof item !== "object") return;
    const row = item as Record<string, unknown>;
    const question = typeof row.question === "string" ? row.question.trim() : "";
    if (!question) return;
    out.push({
      header: typeof row.header === "string" ? row.header.trim() : "",
      question,
      options: normalizeAskOptions(row.options),
      allowMultiple: Boolean(row.allow_multiple || row.allowMultiple || row.multiSelect),
    });
  });
  return out;
}

function describeAskUser(raw: {
  question?: string;
  context?: string;
  options?: unknown;
  questions?: unknown;
  answer?: string | string[];
}) {
  const parts: string[] = [];
  if (typeof raw.question === "string" && raw.question.trim()) parts.push(raw.question.trim());
  if (typeof raw.context === "string" && raw.context.trim()) parts.push(raw.context.trim());
  const options = normalizeAskOptions(raw.options);
  if (options.length) parts.push(`Options: ${options.map((option) => option.label).join(" · ")}`);
  if (Array.isArray(raw.questions) && raw.questions.length) parts.push(`Questions: ${raw.questions.length}`);
  if (Array.isArray(raw.answer) && raw.answer.length) parts.push(`Answer: ${raw.answer.join(", ")}`);
  else if (typeof raw.answer === "string" && raw.answer.trim()) parts.push(`Answer: ${raw.answer.trim()}`);
  return parts.join("\n\n");
}

function numberOrUndefined(value: unknown) {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
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
    const askOptions = normalizeAskOptions(raw.options);
    const askQuestions = normalizeAskQuestions(raw.questions);
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
      askQuestion: raw.question || "",
      askContext: raw.context || "",
      askOptions,
      askQuestions,
      askAnswer: raw.answer,
      askAllowFreeform: raw.allow_freeform !== false,
      askAllowMultiple: Boolean(raw.allow_multiple),
      askResolved: Boolean(raw.resolved),
      askCancelled: Boolean(raw.cancelled),
    };
  }

  if (raw.type === "extension") {
    const title = typeof raw.title === "string" && raw.title.trim() ? raw.title.trim() : "Extension";
    const source = typeof raw.source === "string" && raw.source.trim() ? raw.source.trim() : "";
    const status = typeof raw.status === "string" && raw.status.trim() ? raw.status.trim() : "";
    const summary = typeof raw.summary === "string" && raw.summary.trim() ? raw.summary.trim() : "";
    const extensionKind =
      typeof raw.extension_kind === "string" && raw.extension_kind.trim() ? raw.extension_kind.trim() : "status";
    const items = Array.isArray(raw.items) ? raw.items : [];
    return {
      id: raw.tool_call_id || `extension-${source || title}-${ts ?? "na"}-${index}`,
      kind: "extension",
      ts,
      title,
      body: String(raw.text || ""),
      meta: [source, status].filter(Boolean).join(" · "),
      extensionKind,
      source,
      status,
      summary,
      progressCurrent: numberOrUndefined(raw.progress_current),
      progressTotal: numberOrUndefined(raw.progress_total),
      progressLabel: typeof raw.progress_label === "string" ? raw.progress_label : "",
      items,
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

export function normalizeEvents(events: RawChatEvent[]) {
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

function joinAssistantBodies(left: string, right: string) {
  if (!left) return right;
  if (!right) return left;
  return `${left}${right}`;
}

export function coalesceAdjacentAssistantEvents(events: UiTranscriptEvent[]) {
  const out: UiTranscriptEvent[] = [];
  for (const event of events) {
    const previous = out[out.length - 1];
    if (previous && previous.kind === "assistant" && event.kind === "assistant") {
      const metaParts = [previous.meta, event.meta].filter((value, index, arr) => value && arr.indexOf(value) === index);
      out[out.length - 1] = {
        ...previous,
        id: `${previous.id}+${event.id}`,
        ts: previous.ts ?? event.ts,
        body: joinAssistantBodies(previous.body, event.body),
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

export function mergeTranscriptEvents(current: UiTranscriptEvent[], incoming: UiTranscriptEvent[]) {
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

export function previewFromEvents(events: UiTranscriptEvent[]) {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    if (event.kind !== "user") continue;
    return eventTextPreview(event);
  }
  return "";
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
    case "extension":
      return "event-extension";
  }
}

function statusLabel(status: string) {
  const value = status.replace(/_/g, " ").trim();
  return value || "status";
}

function itemStatusClass(status: string | undefined) {
  const normalized = String(status || "").trim().replace(/_/g, "-");
  return normalized ? ` is-${normalized}` : "";
}

function ExtensionProgress(props: { event: UiTranscriptEvent }) {
  const { event } = props;
  const total = event.progressTotal;
  const current = event.progressCurrent;
  const hasProgress = typeof total === "number" && total > 0 && typeof current === "number";
  const percent = hasProgress ? Math.max(0, Math.min(100, (current / total) * 100)) : 0;
  const items = event.items || [];
  return (
    <div className="extension-panel">
      <div className="extension-head">
        <div className="extension-title-row">
          <span className="extension-title">{event.summary || event.title}</span>
          {event.status ? <span className={`extension-status${itemStatusClass(event.status)}`}>{statusLabel(event.status)}</span> : null}
        </div>
        {event.source ? <span className="extension-source">{event.source}</span> : null}
      </div>
      {hasProgress ? (
        <div className="extension-progress" aria-label={`${current} of ${total} ${event.progressLabel || "complete"}`}>
          <div className="extension-progress-bar" style={{ width: `${percent}%` }} />
          <span className="extension-progress-text">
            {current}/{total}
            {event.progressLabel ? ` ${event.progressLabel}` : ""}
          </span>
        </div>
      ) : null}
      {items.length ? (
        <ol className="extension-items">
          {items.map((item: TranscriptExtensionItem, itemIndex: number) => (
            <li className={`extension-item${itemStatusClass(item.status)}`} key={`${item.label || "item"}-${itemIndex}`}>
              <span className="extension-item-mark" />
              <span className="extension-item-text">
                <span>{item.label || "Untitled item"}</span>
                {item.detail ? <small>{item.detail}</small> : null}
              </span>
            </li>
          ))}
        </ol>
      ) : null}
      {event.body ? <MarkdownBlock text={event.body} className="message-body markdown-body extension-body" /> : null}
    </div>
  );
}

function answerTextForAskUser(event: UiTranscriptEvent, values: string[], freeform: string, bridgeAnswers: Record<string, string | string[]>) {
  const trimmedFreeform = freeform.trim();
  const questions = event.askQuestions || [];
  if (questions.length) {
    const lines = questions
      .map((question) => {
        const answer = bridgeAnswers[question.question];
        const value = Array.isArray(answer) ? answer.join(", ") : String(answer || "").trim();
        return value ? `${question.question}: ${value}` : "";
      })
      .filter(Boolean);
    if (trimmedFreeform) lines.push(trimmedFreeform);
    return lines.join("\n");
  }
  const selected = values.filter((value) => value.trim());
  if (!event.askAllowMultiple) return trimmedFreeform || selected[0] || "";
  if (trimmedFreeform) selected.push(trimmedFreeform);
  return selected.join(", ").trim();
}

function AskUserPrompt(props: {
  event: UiTranscriptEvent;
  onRespond?: (event: UiTranscriptEvent, text: string) => Promise<void>;
}) {
  const { event, onRespond } = props;
  const [selectedValues, setSelectedValues] = useState<string[]>([]);
  const [freeform, setFreeform] = useState("");
  const [bridgeAnswers, setBridgeAnswers] = useState<Record<string, string | string[]>>({});
  const [submitting, setSubmitting] = useState(false);
  const [sent, setSent] = useState(false);
  const [status, setStatus] = useState("");
  const resolved = Boolean(event.askResolved || event.askAnswer || event.askCancelled);
  const canRespond = Boolean(onRespond && !resolved && !submitting && !sent);
  const options = event.askOptions || [];
  const questions = event.askQuestions || [];

  async function submit(values: string[] = selectedValues, freeformValue = freeform) {
    if (!onRespond || !canRespond) return;
    const text = answerTextForAskUser(event, values, freeformValue, bridgeAnswers);
    if (!text) {
      setStatus("Enter an answer first.");
      return;
    }
    setSubmitting(true);
    setStatus("");
    try {
      await onRespond(event, text);
      setSent(true);
      setStatus("Answer sent.");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Unable to send answer.");
    } finally {
      setSubmitting(false);
    }
  }

  function toggleOption(value: string) {
    if (!event.askAllowMultiple) {
      void submit([value], "");
      return;
    }
    setSelectedValues((current) => (current.includes(value) ? current.filter((item) => item !== value) : current.concat(value)));
  }

  const answerText = Array.isArray(event.askAnswer) ? event.askAnswer.join(", ") : event.askAnswer || "";

  return (
    <div
      className="ask-user-panel"
      onClick={(clickEvent) => clickEvent.stopPropagation()}
      onKeyDown={(keyEvent) => keyEvent.stopPropagation()}
    >
      {event.askContext ? <MarkdownBlock text={event.askContext} className="message-body markdown-body ask-user-context" /> : null}
      {event.askQuestion ? <div className="ask-user-question">{event.askQuestion}</div> : null}

      {questions.length ? (
        <div className="ask-user-question-list">
          {questions.map((question) => {
            const current = bridgeAnswers[question.question];
            const selected = Array.isArray(current) ? current : typeof current === "string" ? [current] : [];
            return (
              <section className="ask-user-subquestion" key={question.question}>
                {question.header ? <div className="ask-user-header">{question.header}</div> : null}
                <div className="ask-user-subtitle">{question.question}</div>
                <div className="ask-user-options">
                  {question.options.map((option) => {
                    const isSelected = selected.includes(option.value);
                    return (
                      <button
                        className={`ask-user-option${isSelected ? " is-selected" : ""}`}
                        disabled={!canRespond}
                        key={`${question.question}-${option.value}`}
                        type="button"
                        onClick={() => {
                          setBridgeAnswers((currentAnswers) => {
                            const existing = currentAnswers[question.question];
                            const previous = Array.isArray(existing) ? existing : typeof existing === "string" ? [existing] : [];
                            const next = question.allowMultiple
                              ? previous.includes(option.value)
                                ? previous.filter((value) => value !== option.value)
                                : previous.concat(option.value)
                              : option.value;
                            return { ...currentAnswers, [question.question]: next };
                          });
                        }}
                      >
                        <span>{option.label}</span>
                        {option.description ? <small>{option.description}</small> : null}
                      </button>
                    );
                  })}
                </div>
              </section>
            );
          })}
        </div>
      ) : null}

      {!questions.length && options.length ? (
        <div className="ask-user-options">
          {options.map((option) => (
            <button
              className={`ask-user-option${selectedValues.includes(option.value) ? " is-selected" : ""}`}
              disabled={!canRespond}
              key={option.value}
              type="button"
              onClick={() => toggleOption(option.value)}
            >
              <span>{option.label}</span>
              {option.description ? <small>{option.description}</small> : null}
            </button>
          ))}
        </div>
      ) : null}

      {canRespond && (event.askAllowFreeform || event.askAllowMultiple || questions.length > 0) ? (
        <div className="ask-user-composer">
          {event.askAllowFreeform ? (
            <textarea
              className="ask-user-input"
              rows={2}
              value={freeform}
              placeholder="Type an answer"
              onInput={(inputEvent) => setFreeform(inputEvent.currentTarget.value)}
            />
          ) : null}
          <button className="ask-user-submit" disabled={submitting} type="button" onClick={() => void submit()}>
            {submitting ? "Sending..." : questions.length || event.askAllowMultiple ? "Submit answer" : "Send answer"}
          </button>
        </div>
      ) : null}

      {event.askCancelled ? <div className="ask-user-status">Cancelled</div> : null}
      {answerText ? <div className="ask-user-status">Answer: {answerText}</div> : null}
      {status ? <div className="ask-user-status">{status}</div> : null}
    </div>
  );
}

export function TranscriptEventRow(props: {
  event: UiTranscriptEvent;
  events: UiTranscriptEvent[];
  index: number;
  collapsed: boolean;
  onToggle: (event: UiTranscriptEvent) => void;
  onAskUserRespond?: (event: UiTranscriptEvent, text: string) => Promise<void>;
}) {
  const { event, events, index, collapsed, onToggle, onAskUserRespond } = props;
  const collapsible = isCollapsibleEvent(event.kind);
  const showMessageSide = shouldShowMessageSide(events, index);
  const showMessageFooter = shouldShowMessageFooter(events, index, collapsible);
  const label = event.title || event.kind.replace("_", " ");

  return (
    <article
      className={`msg ${eventClassName(event.kind)}${collapsible ? " is-collapsible" : ""}${collapsed ? " is-collapsed" : ""}`}
      role={collapsible ? "button" : undefined}
      tabIndex={collapsible ? 0 : undefined}
      aria-expanded={collapsible ? !collapsed : undefined}
      onClick={collapsible ? () => onToggle(event) : undefined}
      onKeyDown={
        collapsible
          ? (keyEvent) => {
              if (keyEvent.key === "Enter" || keyEvent.key === " ") {
                keyEvent.preventDefault();
                onToggle(event);
              }
            }
          : undefined
      }
    >
      <div className={`message-side${showMessageSide && !collapsible ? "" : " is-hidden"}`}>
        {showMessageSide && !collapsible ? <span className="message-kind">{label}</span> : null}
      </div>
      <div className="message-main">
        {collapsed ? (
          <div className="message-inline">
            <span className="message-kind">{label}</span>
            <span className="message-fold" aria-label="Show details" title="Show details" />
            <span className="message-summary">{eventTextPreview(event) || "No details"}</span>
          </div>
        ) : (
          <>
            {collapsible ? (
              <div className="message-inline is-open">
                <span className="message-kind">{label}</span>
                <span className="message-fold is-open" aria-label="Hide details" title="Hide details" />
              </div>
            ) : null}
            {event.kind === "extension" ? <ExtensionProgress event={event} /> : null}
            {event.kind === "ask_user" ? <AskUserPrompt event={event} onRespond={onAskUserRespond} /> : null}
            {event.kind !== "extension" && event.kind !== "ask_user" && event.body ? <MarkdownBlock text={event.body} className="message-body markdown-body" /> : null}
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
}
