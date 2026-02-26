# Session: 2026-02-26 File History Auto Refresh

## Focus
Prevent background file refresh from re-adding removed file history entries.

## Requests
- Stop the file history entry from popping back immediately after removing it.

## Actions Taken
- Added a `record_history` flag to `/api/files/read` to skip history updates for background refresh.
- Set fullscreen file auto-refresh reads to pass `record_history=false`.
- Documented the new flag in the server/API feature doc.

## Outcomes
- Removing a file history entry no longer re-adds it during auto-refresh polling.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Manual file opens still record history as before.
