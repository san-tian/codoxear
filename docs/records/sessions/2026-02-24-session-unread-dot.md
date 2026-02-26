# Session: 2026-02-24 Session Unread Dot

## Focus
Add an unread indicator for assistant responses in the session list.

## Requests
- Show a red dot when a message completes, independent of busy state.

## Actions Taken
- Added `last_assistant_ts` to session metadata so the UI can detect unread assistant responses.
- Stored per-session read markers in `localStorage` and updated them on selection and when new assistant messages arrive.
- Rendered a red dot on session cards with unread assistant output.
- Updated server and UI docs.

## Outcomes
- Sessions with unread assistant responses show a red dot until opened.

## Tests
- `python3 -m pytest` (failed: pytest not installed)
- `python3 -m unittest tests.test_server_chat_flags tests.test_sessions_pending_log_idle` (fails: `SessionManager` fixture missing `_aliases`)
- `python3 -m unittest tests.test_server_chat_flags`
