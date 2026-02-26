# Session: 2026-02-22 Missing User Echo

## Focus
Fix cases where a sent user message is not shown in the UI even though Codex responds.

## Requests
- Debug missing user messages after send.

## Actions Taken
- Stopped marking pending local-echo events as duplicates to avoid suppressing real user events if the pending bubble is cleared.

## Outcomes
- User messages should render even if the local pending bubble is lost and the server event arrives later.

## Tests
- `python3 -m unittest tests.test_message_index`

## Notes
- This addresses a likely duplicate-filtering edge case; if the issue persists, add client-side logging to capture `/api/sessions/<id>/messages` payloads.
