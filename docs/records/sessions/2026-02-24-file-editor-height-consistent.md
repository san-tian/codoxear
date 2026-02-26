# Session: 2026-02-24 File Editor Height Consistent

## Focus
Stop edit/preview from collapsing by using a consistent file viewer height across modes.

## Requests
- Fix edit/preview layout mismatch on Safari (view is normal, edit collapses).

## Actions Taken
- Added a consistent modal height for the file viewer on desktop.
- Removed edit/preview-specific height overrides that caused smaller editors.
- Documented the sizing behavior.

## Outcomes
- Edit/preview no longer collapse and match view sizing on desktop Safari.

## Tests
- `python3 -m pytest` (failed: pytest not installed)
- `python3 -m unittest tests.test_server_chat_flags`
