# Session: 2026-02-24 Duplicate Session Alias

## Focus
Make duplicated sessions easier to distinguish by auto-adding a duplicate suffix.

## Requests
- Create smarter session names on new (duplicate) sessions, e.g. add "duplicate".

## Actions Taken
- Added a duplicate-name generator that appends `duplicate` and increments if needed.
- Applied the auto-alias after a duplicated session is created.
- Updated UI documentation.

## Outcomes
- Duplicate actions now name the new session with a `duplicate` suffix (and counter if needed).

## Tests
- `python3 -m pytest` (fails: pytest is not installed in this environment).
- `python3 -m unittest tests.test_server_chat_flags`
