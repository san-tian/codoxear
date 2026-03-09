# Session: 2026-03-07 Codex Busy Uses Strict Turn Markers

## Focus
Make Codex busy detection rely on Codex's explicit turn lifecycle markers instead of PTY redraw/status heuristics and assistant-text-only inference.

## Requests
- Investigate whether Codex exposes a more precise busy/idle signal than the current heuristic approach.
- Update Codoxear so Codex sessions use the precise turn lifecycle from Codex source/protocol semantics.

## Actions Taken
- Read the installed Codex package and the upstream `openai/codex` source to verify the authoritative lifecycle events:
  - `EventMsg::TurnStarted` / `EventMsg::TurnComplete` in protocol docs and source
  - v1 compatibility names `task_started` / `task_complete`
  - internal agent status updates driven by `TurnStarted` / `TurnComplete`
- Updated `codoxear/broker.py`:
  - Codex PTY status hints no longer mark the session busy.
  - Added explicit Codex start handling for `event_msg.type=task_started`.
  - Tightened Codex rollout state transitions so assistant-side activity (`agent_message`, `agent_reasoning`, `response_item`) no longer reopens a closed turn.
  - Added Codex rollout replay when binding an existing rollout so terminal-owned/tmux-backed Codex sessions seed exact current turn state before tailing from EOF.
- Updated `codoxear/rollout_log.py`:
  - Codex idle detection now requires explicit turn closure (`task_complete`, `turn_aborted`, `thread_rolled_back`).
  - Codex tail scans expand until they find an explicit turn boundary or hit the scan budget.
  - If Codex assistant output is present but no explicit boundary is found within the scan budget, idle stays `False` conservatively.
- Added/updated regression tests:
  - `tests/test_broker_busy_state.py`
  - `tests/test_idle_heuristics.py`
- Updated docs:
  - `docs/features/broker.md`
  - `docs/features/rollout-log-parsing.md`
  - `docs/flows/TESTING.md`
  - `docs/records/WORK_RECORDS.md`

## Outcomes
- Codex busy state now follows Codex's own turn lifecycle semantics more closely.
- Codex prompt redraws / PTY status hints no longer re-arm busy.
- Codex assistant text alone no longer makes the server consider the session idle; an explicit turn-end marker is required.
- Existing Codex rollouts can be attached with accurate current busy state reconstructed from the rollout itself.

## Tests
- `python3 -m unittest tests.test_broker_busy_state tests.test_idle_heuristics`
- `python3 -m unittest discover -s tests`
