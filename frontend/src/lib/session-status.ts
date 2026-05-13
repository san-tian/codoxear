import type { UiTranscriptEvent } from "./types";

function eventTimestamp(event: UiTranscriptEvent) {
  const ts = Number(event.ts || 0);
  return Number.isFinite(ts) && ts > 0 ? ts : 0;
}

function eventClosesAssistantWait(event: UiTranscriptEvent) {
  if (event.kind === "assistant") return true;
  if (event.kind === "tool_result" && event.toolResultIsError) return true;
  if (event.kind === "tool" && event.toolResultIsError) return true;
  if (event.kind === "extension") {
    const status = String(event.status || event.meta || "").trim().toLocaleLowerCase();
    return status === "error" || status === "failed" || status === "cancelled" || status === "aborted";
  }
  return false;
}

export function isAwaitingAssistantReply(transcript: UiTranscriptEvent[], queueLen = 0, closedTurnTs = 0) {
  if (queueLen) return false;
  let latestUserTs = 0;
  let latestCloseTs = Number.isFinite(closedTurnTs) && closedTurnTs > 0 ? closedTurnTs : 0;
  for (const event of transcript) {
    const ts = eventTimestamp(event);
    if (!ts) continue;
    if (event.kind === "user") latestUserTs = Math.max(latestUserTs, ts);
    if (eventClosesAssistantWait(event)) latestCloseTs = Math.max(latestCloseTs, ts);
  }
  return latestUserTs > latestCloseTs;
}
