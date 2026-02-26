# Session: 2026-02-26 File History Remove Across Sessions

## Focus
Stop workspace file history entries from reappearing when multiple sessions share the file.

## Requests
- File history "x" still reappears after removal.

## Actions Taken
- Collected all session IDs that list each workspace file.
- Removed/cleared file history by issuing `/api/files/remove` or `/api/files/clear` for every session in the workspace.

## Outcomes
- Removing a file from workspace history now clears it across all sessions in the workspace, so it no longer reappears after refresh.
- Clearing the workspace file history now wipes every session in the workspace.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Multi-session removal is best-effort; errors surface via toast if any call fails.
