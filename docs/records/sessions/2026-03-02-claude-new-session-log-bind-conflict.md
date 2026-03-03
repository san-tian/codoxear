# Session: 2026-03-02 Claude New Session Log Bind Conflict

## Focus
Fix Claude new-session creation attaching to an existing Claude session log when another Claude session is already active in the same workspace.

## Requests
- Investigate why creating a new Claude session can jump into an existing Claude session.
- Ensure new Claude sessions do not auto-bind to logs already used by other live sessions.

## Actions Taken
- Traced Claude log discovery in `codoxear/broker.py`:
  - `_discover_log_watcher`
  - `_find_recent_claude_project_log`
- Identified root cause: fallback log discovery selected the newest `cwd`-matched Claude log without excluding logs already claimed by other live broker sockets.
- Added a spawn-level guard for Claude web sessions:
  - Default Claude spawns now include `--session-id <uuid>` to force fresh-session semantics for "new session".
  - Explicit session-control args (`--resume`, `--continue`, `--session-id`, `--from-pr`) are respected and do not get overridden.
- Implemented broker-side guardrails:
  - Added claimed-log collection from socket sidecar metadata (`*.json`) for live sessions.
  - Added exclusion support to Claude/Gemini fallback selectors.
  - Wired `_discover_log_watcher` to pass claimed log exclusions during fallback resolution.
- Added regression coverage:
  - `test_broker_fallback_skips_excluded_claude_logs` in `tests/test_broker_proc_rollout.py`.
  - Claude spawn tests for auto `--session-id` and explicit resume args in `tests/test_server_spawn_cli.py`.
- Updated broker feature documentation with the new fallback exclusion behavior.
  - Updated server/API feature documentation for Claude spawn session-id behavior.

## Outcomes
- New Claude sessions in a workspace with existing live Claude sessions no longer attach to already-claimed logs.
- New Claude web sessions now explicitly start with a fresh session ID by default.
- Fallback discovery remains available for cases where `/proc` open-fd discovery cannot find the active Claude log.

## Tests
- `python3 -m unittest tests.test_broker_proc_rollout`
- `python3 -m unittest tests.test_server_spawn_cli`
- `python3 -m unittest tests.test_broker_busy_state`
- `python3 -m unittest discover -s tests`
