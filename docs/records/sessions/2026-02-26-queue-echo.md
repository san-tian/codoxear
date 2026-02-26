# Session: 2026-02-26 Queue Echo

## Focus
Make queued messages visible immediately in the chat UI.

## Requests
- Fix the UI so queued messages are shown after enqueueing.

## Actions Taken
- Added a local pending bubble when a message is queued (same matching flow as send).
- Updated the UI doc to mention the queued local echo.

## Outcomes
- Queued messages appear in the chat immediately and reconcile when the log emits them.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Queue errors now mark the pending bubble with the same visual error styling as send failures.
