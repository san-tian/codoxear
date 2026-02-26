# Session: 2026-02-24 File Viewer Scroll Restore

## Focus
Preserve file viewer scroll position when reloading a file.

## Requests
- Keep current reading progress when reloading a file after external edits.

## Actions Taken
- Captured scroll ratio for the active file viewer mode (view/edit/preview) before reload.
- Restored scroll position after the file content is reloaded.
- Updated UI documentation.

## Outcomes
- Reloading a file retains the prior scroll position so reading progress is preserved.

## Tests
- `python3 -m pytest` (fails: pytest is not installed in this environment).
- `python3 -m unittest tests.test_server_chat_flags`
