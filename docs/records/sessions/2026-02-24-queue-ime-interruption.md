# Session: 2026-02-24 Queue IME Interruption

## Focus
Stop queue editor re-renders from interrupting IME composition while editing queued messages.

## Requests
- Fix IME composition being interrupted when editing queued messages.

## Actions Taken
- Skipped queue list re-renders while a queue textarea is focused.
- Documented the IME-safe queue editor behavior in the UI feature doc.

## Outcomes
- IME composition is no longer disrupted by background polling that refreshes the queue list.

## Tests
- `python3 -m pytest` (failed: pytest not installed)
- `python3 -m unittest tests.test_server_chat_flags`
