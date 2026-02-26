# Session: 2026-02-26 File History X Close

## Focus
Make the file history remove button immediately clear the row in the sidebar.

## Requests
- Fix the "x" in the navigation sidebar so file history entries close/remove on tap.

## Actions Taken
- Removed the file history row in the UI immediately on tap before refreshing sessions.

## Outcomes
- Tapping the "x" now visibly removes the file history entry right away.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Server refresh still runs after removal to keep history in sync.
