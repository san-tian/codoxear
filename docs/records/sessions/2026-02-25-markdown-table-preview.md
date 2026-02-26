# Session: 2026-02-25 Markdown Table Preview

## Focus
Fix markdown table rendering in preview.

## Requests
- Correct table handling in markdown preview.

## Actions Taken
- Added basic pipe-table parsing to the markdown renderer with column alignment support.
- Wrapped rendered tables in a scrollable container and added table styling.
- Updated UI docs to note table support.

## Outcomes
- Markdown previews now render tables consistently instead of showing raw pipe text.

## Tests
- `python3 -m unittest discover -s tests` (fails: Python 3.8 cannot import `list[dict]` type hints; `_aliases` missing in `SessionManager`.)
