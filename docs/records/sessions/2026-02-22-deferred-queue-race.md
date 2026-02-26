# Session: 2026-02-22 Deferred Queue Race

## Focus
Fix cases where a deferred message is sent before the current turn completes.

## Requests
- Investigate reports that queued messages sometimes send immediately while a turn is still running.
- Make deferred sending more reliable.

## Actions Taken
- Marked `task_complete` as `turn_end` in rollout parsing (do not use `token_count`).
- Added a deferred-send gate in the UI that waits for `turn_end` or `turn_aborted` before releasing queued messages.
- Updated UI and rollout log docs to reflect the gating behavior.
- Updated tests to match the new `turn_end` flag behavior.

## Outcomes
- Deferred messages only send after an explicit turn end/abort signal, avoiding early injection during long turns.

## Tests
- `python3 -m unittest tests.test_server_chat_flags`

## Notes
- `python3 -m pytest` is unavailable in this environment (missing pytest), so unittest was used.
