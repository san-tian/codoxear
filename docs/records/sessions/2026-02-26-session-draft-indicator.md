# Session: 2026-02-26 Session Draft Indicator

## Focus
Show a sidebar indicator when a session has a saved draft.

## Requests
- Reflect draft presence in the navigation session list.

## Actions Taken
- Added a `draft` badge in session cards when local draft text exists.
- Updated draft save/clear logic to keep the badge in sync without waiting for a full refresh.

## Outcomes
- Session rows show a `draft` badge whenever the composer has saved text for that session.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Draft detection relies on localStorage-backed draft cache, trimmed for whitespace.
