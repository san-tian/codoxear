# Session: 2026-02-26 File History Remove by Cwd

## Focus
Ensure the workspace file history "x" removes entries reliably.

## Requests
- File history entry still reappears after removal.

## Actions Taken
- Added server-side `cwd` handling for `/api/files/remove` and `/api/files/clear` to target workspace keys directly.
- Updated the UI to call file-history remove/clear with the workspace `cwd`.
- Added tests for cwd-based removal/clearing.

## Outcomes
- Workspace file history entries are removed using the cwd key, so they no longer reappear after refresh.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- UI falls back to session-based removal when the workspace cwd is unknown.
