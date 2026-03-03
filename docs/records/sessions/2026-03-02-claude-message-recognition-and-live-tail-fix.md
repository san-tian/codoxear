# Session: 2026-03-02 Claude Message Recognition and Live Tail Fix

## Focus
Fix Claude chat rendering noise (placeholder bubbles) and improve Claude Session Tools live tail readability.

## Requests
- Claude message recognition was showing incorrect tiny/placeholder messages.
- Claude live tail output in Session Tools was noisy and hard to read.

## Actions Taken
- Investigated live Claude project logs under `~/.claude/projects/**` and confirmed repeated assistant placeholder rows (`"."` and blank text fragments) were being parsed as real chat messages.
- Updated `codoxear/cli_support.py`:
  - `claude_assistant_text` now ignores whitespace-only text parts.
  - Added placeholder detection for Claude progress dot rows (`"."`, `".."`, `"..."`, `"…"` with in-progress metadata) and excludes them from chat events.
- Updated `codoxear/server.py`:
  - Added `_sanitize_claude_tail_text` for Claude-specific tail cleanup after base ANSI/control stripping.
  - Wired `SessionManager.get_tail` to apply the extra filter only for `cli=claude`.
  - Filtering removes common spinner/status fragments (`Flowing…`, short alnum shards, shortcut hints, box-drawing noise) while preserving meaningful lines.
- Added tests:
  - `tests/test_cli_support.py` for Claude dot/blank filtering.
  - `tests/test_server_chat_flags.py` to ensure Claude dot placeholders do not create chat events.
  - `tests/test_tail_sanitize.py` for Claude tail noise filtering behavior.
- Updated docs:
  - `docs/features/rollout-log-parsing.md`
  - `docs/features/session-tools.md`
  - `docs/flows/TESTING.md`
  - `docs/records/WORK_RECORDS.md`

## Outcomes
- Claude chat no longer renders placeholder dot/blank assistant rows as message bubbles.
- Claude Session Tools live tail is significantly less noisy and keeps readable result/output lines.

## Tests
- `python3 -m unittest tests.test_cli_support tests.test_server_chat_flags tests.test_tail_sanitize`
- `python3 -m unittest tests.test_broker_busy_state tests.test_idle_heuristics`
- `python3 -m unittest discover -s tests`
