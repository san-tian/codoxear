# Session: 2026-02-24 File Editor Popout Tab

## Focus
Enable full-screen file editing in a dedicated browser tab.

## Requests
- Allow opening the file editor in a separate tab for full-screen editing.

## Actions Taken
- Added a "Pop out" action to open the file editor in a new full-screen tab.
- Added URL param handling for `file`, `session_id`, `mode`, `fullscreen`, and `wrap`.
- Adjusted file viewer behavior and styling for full-screen mode.
- Removed the file editor's global textarea max-height so it can fill full-screen space.
- Updated the UI feature doc.

## Outcomes
- Users can open a dedicated browser tab that launches directly into a full-screen file editor.

## Tests
- `python3 -m pytest` (fails: pytest is not installed in this environment).
- `python3 -m unittest tests.test_server_chat_flags`
