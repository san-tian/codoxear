# Session: 2026-02-26 Queue Badge Refresh

## Focus
Keep the queue badge/list in sync after adding queued messages.

## Requests
- Queue badge remains empty after inserting a queued message.

## Actions Taken
- Captured the active session when queueing to avoid selection races.
- Forced a queue badge/list refresh after queue responses and after opening the queue viewer.
- Synced queue updates when `/send` reports a queued response.

## Outcomes
- Queue badge and queue viewer refresh immediately after queueing.

## Tests
- `python3 -m unittest discover -s tests` (fails in this environment: Python 3.8 cannot import `list[dict]` type hints in `test_idle_heuristics`, `test_last_updated_ts`, `test_session_log`.)
