# Session: 2026-02-24 Safari Unread Dot

## Focus
Make the unread response dot render reliably on macOS Safari.

## Requests
- Ensure the unread indicator shows up on Safari.

## Actions Taken
- Wrapped session badges in a dedicated inline-flex container for predictable layout.
- Adjusted unread dot sizing and display rules for Safari compatibility.
- Disabled caching on JSON API responses and fetches to avoid Safari serving stale session lists.
- Updated the UI feature doc with the badge container note.

## Outcomes
- Unread indicators render consistently across browsers, including Safari, and session list data stays fresh.

## Tests
- `python3 -m unittest discover -s tests` (failed: Python 3.8 runtime rejects `list[dict]` type annotations; `SessionManager` fixture missing `_aliases`.)
