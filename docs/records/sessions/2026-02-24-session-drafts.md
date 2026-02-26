# Session: 2026-02-24 Session Drafts

## Focus
Persist message drafts per session in the browser.

## Requests
- Store composer drafts per session instead of sharing across sessions.

## Actions Taken
- Added per-session draft storage in `localStorage`.
- Save drafts on input and when switching sessions.
- Restore drafts when selecting a session; clear draft on send and session delete.
- Updated UI docs.

## Outcomes
- Each session retains its own draft in the composer.

## Tests
- `python3 -m pytest` (failed: pytest not installed)
- `python3 -m unittest tests.test_server_chat_flags`
