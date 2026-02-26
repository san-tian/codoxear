# Session: 2026-02-26 Live Tail UTF-8 Decoder

## Focus
Reduce remaining garbled characters in live tail output.

## Requests
- Live tail still shows occasional garbled characters after ANSI stripping.

## Actions Taken
- Switched PTY output decoding in broker and sessiond to incremental UTF-8 decoding.
- This prevents multibyte characters from turning into replacement symbols when split across reads.

## Outcomes
- Live tail should show fewer replacement characters and cleaner output.

## Tests
- `python3 -m unittest discover -s tests` (fails in this environment: Python 3.8 cannot import `list[dict]` type hints in `test_idle_heuristics`, `test_last_updated_ts`, `test_session_log`.)
