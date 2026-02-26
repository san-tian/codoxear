# Session: 2026-02-24 Mermaid Preview

## Focus
Render Mermaid diagrams in markdown preview.

## Requests
- Add Mermaid diagram rendering to markdown preview.

## Actions Taken
- Added Mermaid via CDN and initialized rendering in the UI.
- Rendered Mermaid code fences (language `mermaid`/`mmd`) as diagrams in markdown output.
- Added basic Mermaid block styling for overflow and sizing.
- Updated the UI feature doc.

## Outcomes
- Markdown preview now renders Mermaid diagrams when Mermaid is available.

## Tests
- `python3 -m pytest` (fails: pytest is not installed in this environment).
- `python3 -m unittest tests.test_server_chat_flags`
