# Rollout Log Parsing

Codoxear reads Codex rollout JSONL files to render chat history, compute idle state, and surface token usage.

## Chat event extraction
How users use it:
The server and UI request `/api/sessions/<id>/messages` to receive user/assistant events.

Effect:
The server extracts user and assistant messages and converts them into chat events for the UI.

Files:
- `codoxear/rollout_log.py`
- `codoxear/server.py`

Key flow:
1. Read JSONL tail chunks.
2. Extract `event_msg` user messages and `response_item` assistant text.
3. Track tool usage and turn boundaries for UI indicators (`task_complete` marks turn end).

Call stack:
1. `SessionManager._ensure_chat_index`
2. `_read_chat_tail_snapshot`
3. `_extract_chat_events`

## Idle detection
How users use it:
The UI relies on idle state to determine whether it can send or should queue.

Effect:
Idle state is computed from the most recent terminal events in the rollout log.

Files:
- `codoxear/rollout_log.py`
- `codoxear/server.py`

Key flow:
1. Scan recent log tail for user or assistant events.
2. Track whether a turn is open and if a completion candidate exists.
3. Close turns on `task_complete`, `turn_aborted`, or `thread_rolled_back`.
4. Return idle when the turn is closed and the last terminal event is assistant/aborted, or when an open turn has an assistant completion candidate.

Notes:
- The server combines broker `busy` state with log-derived idle state.
- Large logs cap their scan size with `CODEX_WEB_CHAT_INIT_MAX_SCAN_BYTES` and related settings.

## Token statistics
How users use it:
Context usage appears in the UI header.

Effect:
Token usage and context window are parsed from `token_count` events.

Files:
- `codoxear/rollout_log.py`
- `codoxear/server.py`

Key flow:
1. Extract `token_count` payloads in recent log chunks.
2. Compute percent remaining using the baseline token offset.
3. Persist in session state for UI display.
