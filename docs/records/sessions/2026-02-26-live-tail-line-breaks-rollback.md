# Session: 2026-02-26 Live Tail Line Breaks Rollback

## Focus
Fix live tail output after forced line breaks caused excessive wrapping.

## Requests
- Live tail is now all garbled with random line breaks.

## Actions Taken
- Removed the forced line breaks triggered by cursor-movement/line-clear ANSI sequences.
- Kept ANSI/control stripping and carriage-return normalization only.

## Outcomes
- Live tail output should no longer insert random newlines.

## Tests
- `python3 -m unittest discover -s tests` (fails in this environment: Python 3.8 cannot import `list[dict]` type hints in `test_idle_heuristics`, `test_last_updated_ts`, `test_session_log`.)
