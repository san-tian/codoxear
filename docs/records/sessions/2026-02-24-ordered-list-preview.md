# Session: 2026-02-24 Ordered List Preview

## Focus
Fix ordered list numbering in markdown preview.

## Requests
- Ordered list numbering in preview should continue when list items start at `2.` or later.

## Actions Taken
- Added ordered-list start handling in the markdown renderer so the first list number is preserved.
- Updated the UI feature doc.

## Outcomes
- Markdown preview now respects ordered list starting numbers.

## Tests
- `python3 -m pytest` (fails: pytest is not installed in this environment).
- `python3 -m unittest tests.test_server_chat_flags`
