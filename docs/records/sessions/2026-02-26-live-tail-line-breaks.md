# Session: 2026-02-26 Live Tail Line Breaks

## Focus
Keep live tail output readable when the terminal uses cursor movement instead of newlines.

## Requests
- Input sometimes appears on the same line as output in the live tail view.

## Actions Taken
- Interpreted ANSI cursor-movement/line-clear sequences as line breaks during tail sanitization.
- This prevents prompts or echoed input from appearing mid-line when cursor moves were stripped.

## Outcomes
- Live tail shows a new line before prompts/inputs that rely on cursor positioning.

## Tests
- `python3 -m unittest discover -s tests` (fails in this environment: Python 3.8 cannot import `list[dict]` type hints in `test_idle_heuristics`, `test_last_updated_ts`, `test_session_log`.)
