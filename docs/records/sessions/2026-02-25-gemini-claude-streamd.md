# Session: 2026-02-25 Gemini + Claude Streamd

## Focus
Add Gemini CLI and Claude Code support with stream-json providers.

## Requests
- Extend Codoxear sessions to support Gemini CLI and Claude Code with parity to Codex.

## Actions Taken
- Added a stream-json broker (`streamd`) that maps Gemini/Claude output into Codex-style JSONL logs.
- Added stream-json mapping helpers and tests for Gemini/Claude event translation.
- Wired provider selection into session creation and session list metadata.
- Documented streamd configuration and new testing command.

## Outcomes
- New sessions can target `gemini` or `claude` providers and behave like Codex sessions in the UI.

## Tests
- `python3 -m pytest` (fails: pytest is not installed in this environment).
- `python3 -m unittest tests.test_server_chat_flags`
- `python3 -m unittest tests.test_stream_map`
