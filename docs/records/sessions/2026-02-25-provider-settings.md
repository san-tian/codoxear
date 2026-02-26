# Session: 2026-02-25 Provider Settings

## Focus
Add settings UI for provider relay/API configuration and support restarting sessions when switching relays.

## Requests
- Add a settings button to configure Codex/Gemini/Claude relay + API pairs.
- Support restarting provider processes after switching relays.

## Actions Taken
- Added provider config storage and env overrides for Codex/Gemini/Claude.
- Added `/api/providers` and `/api/providers/restart` endpoints.
- Added settings UI with provider sections and restart actions.
- Documented provider settings routes and UI behavior.

## Outcomes
- Users can edit relay/API profiles and restart provider sessions from the UI.

## Tests
- `python3 -m pytest` (fails: pytest is not installed in this environment).
- `python3 -m unittest tests.test_server_chat_flags`
- `python3 -m unittest tests.test_stream_map`
