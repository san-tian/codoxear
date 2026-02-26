# Session: 2026-02-24 File Editor iOS Height

## Focus
Keep the file editor visible when the iOS keyboard resizes the visual viewport.

## Requests
- Fix edit/preview layout mismatch on iOS (view mode is fine).

## Actions Taken
- Mobile file viewer now uses `height: var(--appH)` and removes `bottom: 0` so it tracks the visual viewport.
- Documented the iOS visual viewport behavior in the UI feature doc.

## Outcomes
- File viewer edit/preview stay aligned with view when the iOS keyboard opens.

## Tests
- `python3 -m pytest` (failed: pytest not installed)
- `python3 -m unittest tests.test_server_chat_flags`
