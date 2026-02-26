# Session: 2026-02-26 Live Tail Sanitize

## Focus
Reduce garbled output in the session tools live tail.

## Requests
- Live tail shows lots of garbled characters mixed with some Chinese characters.

## Actions Taken
- Added ANSI/control-sequence stripping for `/api/sessions/<id>/tail` responses.
- Removed C0/C1 control characters and normalized carriage returns.

## Outcomes
- Live tail output is readable text instead of raw terminal escape noise.

## Tests
- `python3 -m unittest discover -s tests` (fails in this environment: Python 3.8 cannot import `list[dict]` type hints in `test_idle_heuristics`, `test_last_updated_ts`, `test_session_log`.)
