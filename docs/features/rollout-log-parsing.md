# Rollout Log Parsing

## Purpose

Rollout log parsing converts Codex/Pi CLI logs into normalized chat events, token usage, idle/busy signals, tool-call visibility, and status data used by server APIs and browser transcripts.

## Key Files

- `backend-rs/src/runtime.rs` — production parser for public `/api/v1/sessions/:id/messages/*`, idle/status, and token usage routes.
- `backend-rs/src/broker.rs` — live broker-side rollout/Pi log token scanning.

## Call Chain

1. Broker/session discovery identifies rollout/log paths for a selected session.
2. Parser code reads new or historical log entries and normalizes event shapes.
3. Server APIs return tail/history/live events, diagnostics, idle labels, and token/queue status.
4. Browser UI filters or displays normal, tool, tool-result, and ask-user events.

## Current Behavior

- Rust rollout parsing is the source of truth for historical transcript display and part of the busy/idle picture.
- Codex `event_msg` lifecycle records feed idle detection: `task_started` marks the session busy, while `task_complete`, `turn_complete`, and `turn_aborted` mark the current turn idle/closed.
- Token usage and final response state feed notification/voice behavior. The parsed token payload is also what Nova uses for the top-bar context chip: `tokens_in_context` comes from the latest log-derived token update, Codex logs provide `model_context_window` directly, and Pi logs currently derive the context-window size from local `models.json` metadata for the active provider/model.
- The Rust broker reuses the Rust rollout/Pi token parser while tailing its discovered log, so broker socket `state` can update `token` live instead of waiting for a separate transcript/status route to rescan the log.
- Tool-call and ask-user normalization supports visibility toggles in both legacy and preview UI work.
- The parser still emits distinct `tool` and `tool_result` history events; Nova preview now merges matching pairs in frontend transcript normalization so browser history shows one combined tool row with call/result details instead of two adjacent rows.
- Nova preview frontend normalization also de-duplicates obvious repeated/overlapping adjacent assistant text before display. This is specifically to tolerate `messages/live` increments that occasionally replay the last visible assistant segment with a fresh event id; the browser now keeps the longer combined text instead of visually repeating the same sentence block twice.
- Normalized `ask_user` events preserve question text, context, options, multi-question payloads, answer/resolution state, and freeform/multiple flags so Nova preview can render unresolved prompts as answerable UI controls.
- Tool-call summaries include common command argument keys such as `cmd` and `command`, so `exec_command` calls expose the shell command text in browser transcripts.
- Tool calls can publish structured Web UI events by including a `codoxear_display` v1 object in arguments, and structured tool results can publish the same event through `structuredContent.codoxear_display`; Codex `update_plan` calls are also normalized into `extension` progress events. The protocol is documented in `Designs/extension-display-protocol.md`.
- Codex goal-mode events are normalized as first-class `extension_kind: "goal"` rows: `create_goal`, `get_goal`, and `update_goal` calls/results surface objective, status, token budget/used/remaining, elapsed seconds, and completion budget reports when present. The parser also recognizes Codex's active-goal developer continuation prompt and converts only that known goal-context message into a visible goal status row, without exposing unrelated system/developer messages.
- The parser’s `percent_remaining` field is not a raw `tokens_in_context / context_window` remainder. It reserves `CONTEXT_WINDOW_BASELINE_TOKENS` first, then reports the remaining percentage of that reduced effective window, so UI copy should treat it as an adjusted estimate rather than an exact full-window percentage.
- Rust `messages/live` normalizes tool calls/results, `ask_user`, `update_plan`, `codoxear_display`, and `diag.tool_names` / `diag.last_tool` plus `meta_delta.thinking|tool|system`.
- Rust `messages/tail|history` keep single-record event selection for tool/result/ask-user/extension event families, so historical Nova transcript pages preserve visible tool/progress rows.

## Verification

- Automated Rust parser safety: `cd backend-rs && cargo test message_ broker::tests::rust_broker_scans_codex_token_updates_incrementally -- --test-threads=1`
- Broader Rust safety: `cd backend-rs && cargo test`
- Manual: open a session with existing logs and confirm history, live updates, tool-call toggle, token/status, and idle state render correctly.

## Known Risks

- CLI JSONL/log schemas can change across Codex/Pi versions.
- Large transcripts require careful tail/history pagination to avoid UI or server latency.
- Parser changes can affect UI, notifications, queue idleness, and metrics at the same time.
- Parser changes can affect UI, notifications, queue idleness, and metrics at the same time; add Rust regressions for any newly discovered CLI log shape.
