# Session: 2026-02-24 Topbar Toast Alignment

## Focus
Fix topbar alignment issues when the toast is empty.

## Requests
- Resolve navigation bar misalignment.

## Actions Taken
- Removed reserved toast height when empty to keep the topbar aligned.
- Restored toast spacing only while a message is visible.
- Updated UI docs.

## Outcomes
- Topbar items remain aligned when no toast is shown.

## Tests
- `python3 -m pytest` (failed: pytest not installed)
- `python3 -m unittest tests.test_server_chat_flags`
