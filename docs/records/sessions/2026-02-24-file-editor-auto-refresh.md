# Session: 2026-02-24 File Editor Auto Refresh

## Focus
Refresh the full-screen file viewer when the underlying file changes.

## Requests
- Auto-refresh the popout tab when the file changes on disk.

## Actions Taken
- Added a full-screen-only polling loop to re-read the file and refresh the content.
- Paused auto-refresh while there are unsaved edits to avoid overwriting local changes.
- Documented auto-refresh behavior in the UI feature doc.

## Outcomes
- Full-screen file viewer updates automatically when the file changes on disk, unless there are unsaved edits.

## Tests
- `python3 -m pytest` (fails: pytest is not installed in this environment).
- `python3 -m unittest tests.test_server_chat_flags`
