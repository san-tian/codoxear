# Session: 2026-02-26 Relay Indicator Sidebar

## Focus
Move the relay health indicator into the sidebar header.

## Requests
- Place the breathing relay indicator in the left navigation header, to the right of the icon/logo.

## Actions Taken
- Moved the relay status element from the topbar pill into the sidebar header title row.
- Updated UI documentation to reference the sidebar placement.

## Outcomes
- Relay indicator now sits next to the Codoxear logo in the sidebar header.

## Tests
- `python3 -m unittest discover -s tests` (fails in this environment: Python 3.8 cannot import `list[dict]` type hints in `test_idle_heuristics`, `test_last_updated_ts`, `test_session_log`.)
