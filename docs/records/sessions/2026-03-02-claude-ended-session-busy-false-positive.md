# Session: 2026-03-02 Claude Ended Session Busy False Positive

## Focus
Fix cases where a finished Claude session remained marked as busy in the sidebar/API.

## Requests
- Investigate reports that ended Claude sessions were still shown as busy.

## Actions Taken
- Traced busy synthesis path in `codoxear/server.py`:
  - `SessionManager.list_sessions`
  - `GET /api/sessions/<id>/messages`
- Identified the false-positive pattern:
  - Broker state could already be idle (`state_busy=false`),
  - But log-derived idle remained false without new log writes,
  - Resulting in persistent `busy=true` from log-only fallback.
- Implemented stale-log guardrails:
  - Added `_busy_from_state_and_log_idle(...)` helper.
  - Added `CODEX_WEB_LOG_BUSY_FROM_LOG_STALE_SECONDS` (default `45.0`).
  - Busy now uses broker state as primary; log-derived busy applies only within the recency window.
  - Wired helper into both:
    - session list busy (`/api/sessions`)
    - message busy (`/api/sessions/<id>/messages`)
- Added regression tests in `tests/test_sessions_pending_log_idle.py`:
  - stale log + broker idle + log-busy => reported idle
  - recent log + broker idle + log-busy => keep temporary busy fallback
- Updated docs:
  - `docs/features/server-and-api.md`
  - `docs/flows/TESTING.md`
  - `docs/records/WORK_RECORDS.md`

## Outcomes
- Finished Claude sessions no longer stay busy forever due stale log-only signals.
- Busy fallback from logs is preserved for short recent windows to avoid immediate false-idle transitions.

## Tests
- `python3 -m unittest tests.test_sessions_pending_log_idle`
- `python3 -m unittest discover -s tests`
