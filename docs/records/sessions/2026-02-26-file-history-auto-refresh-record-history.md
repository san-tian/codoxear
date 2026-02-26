# Session: 2026-02-26 File History Auto Refresh Record History

## Focus
Prevent auto-refresh reads from re-adding removed file history entries.

## Requests
- File history "x" still does not stay removed.

## Actions Taken
- Restored manual file opens to record history by default.
- Set full-screen auto-refresh reads to pass `record_history=false`.

## Outcomes
- Background refresh no longer re-inserts removed file history entries.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Auto-refresh runs only in full-screen mode.
