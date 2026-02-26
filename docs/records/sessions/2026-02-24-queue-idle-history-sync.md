# Session: 2026-02-24 Queue Idle Release + History Sync

## Focus
Fix deferred queue stalls when idle and ensure session history loads reliably on selection.

## Requests
- Queued messages should send once Codex is idle even if no turn_end arrives.
- Latest replies should not disappear when switching sessions; history should refresh without manual refresh.

## Actions Taken
- Added an idle-stability fallback that clears the deferred-send gate after the session stays idle briefly.
- Reset idle timers whenever a deferred gate is set to avoid premature release.
- Prefer init history fetch on session selection; use local cache only if init fails.
- Updated UI docs to reflect turn-end and idle fallback behavior.

## Outcomes
- Deferred queues no longer stall when the backend fails to emit a turn_end signal.
- Session selection reliably shows current history without manual refresh.

## Tests
- `python3 -m unittest tests.test_server_chat_flags`

## Notes
- Manual sanity check: queue a message while a turn is running, wait for idle, and confirm the queued message sends automatically.
