# Session: 2026-02-26 File Viewer Copy Buttons

## Focus
Add copy actions for the file viewer header.

## Requests
- Add two buttons to copy the current file's full path and filename.

## Actions Taken
- Added "Copy path" and "Copy name" buttons to the file viewer header.
- Used the existing clipboard helper and tracked the last loaded path for accurate copies.
- Updated UI documentation for the new actions.

## Outcomes
- File viewer provides quick copy actions for the active file path and name.

## Tests
- `python3 -m unittest discover -s tests` (fails in this environment: Python 3.8 cannot import `list[dict]` type hints in `test_idle_heuristics`, `test_last_updated_ts`, `test_session_log`.)
