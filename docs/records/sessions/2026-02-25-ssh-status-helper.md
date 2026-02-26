# Session: 2026-02-25 SSH Status Helper

## Focus
Add a quick CLI helper to check web-owned session status from SSH.

## Requests
- Provide a simpler way to see if a web-created session is still running.

## Actions Taken
- Added `scripts/codoxear-status` to read socket sidecars and query broker state.
- Documented the helper usage in README and development flow docs.

## Outcomes
- Users can list sessions and see running/idle state without opening the UI.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Status uses broker socket `state`; stale sockets show as `down`.
