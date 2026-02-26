# Session: 2026-02-25 Remove Provider + Vendor Support

## Focus
Remove Gemini/Claude adapters and vendor-switching support, reverting to Codex-only behavior.

## Requests
- Remove Claude/Gemini adaptations.
- Remove Codex vendor-switching/resume support.

## Actions Taken
- Dropped stream-json provider code (`streamd`/`stream_map`) and related tests.
- Removed provider/session configuration and vendor resume routes from the server.
- Removed provider settings and vendor UI controls from the browser app.
- Cleaned docs and testing references.

## Outcomes
- Codoxear is back to Codex-only sessions with the original broker flow.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- .env vendor placeholders were removed; existing base env entries are untouched.
