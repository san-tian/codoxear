# Session: 2026-02-26 File History Remove All Fallback

## Focus
Ensure file history removal sticks even when workspace removal fails.

## Requests
- File history entry still reappears after removal, even with multiple tabs.

## Actions Taken
- Added `/api/files/remove` `scope=all` to remove a file path from every stored history list.
- Updated the UI to retry removal with `scope=all` when workspace removal still returns the path.
- Added tests for global removal.

## Outcomes
- File history removal now falls back to a global purge if workspace removal does not clear the path.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- The fallback only runs when the cwd-scoped removal still reports the path.
