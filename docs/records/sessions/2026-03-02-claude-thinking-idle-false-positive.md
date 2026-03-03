# Session: 2026-03-02 Claude Thinking Idle False Positive

## Focus
Fix Claude sessions incorrectly showing `idle` when the model outputs text but continues thinking for an extended period without new log writes.

## Problem
When Claude outputs initial text (e.g., "Let me analyze this...") and then enters a long thinking phase, the idle detection logic would incorrectly mark the session as `idle` because:
1. The turn was still open (`turn_open=True`)
2. Text output set `turn_has_completion_candidate=True`
3. The logic returned `True` (idle) for open turns with completion candidates

This was a false positive because Claude's turn only truly ends when a `turn_duration` or `api_error` system message is received. Until then, the model may still be thinking or using tools.

## Solution
Modified `_compute_idle_from_log` in `codoxear/rollout_log.py` to distinguish between Claude and Codex/Gemini formats:

- Added `is_claude_format` flag to detect Claude-specific message types (`type="user"`, `type="assistant"`, `type="system"`)
- For Claude format: require explicit turn closure (via `turn_duration` or `api_error`) before marking as idle
- For Codex/Gemini: continue using the completion candidate heuristic (existing behavior)

This mirrors the fix previously applied to Gemini (see `2026-03-02-gemini-thinking-idle-false-positive.md`).

## Actions Taken
- Updated `codoxear/rollout_log.py`:
  - Added `is_claude_format` tracking in `_compute_idle_from_log`
  - Modified idle return logic to require explicit turn closure for Claude
- Added regression tests in `tests/test_idle_heuristics.py`:
  - `test_claude_thinking_only_stays_busy`: Pure thinking without text stays busy
  - `test_claude_text_then_thinking_stays_busy`: Separate text and thinking messages stay busy
  - `test_claude_text_and_thinking_in_same_message_stays_busy`: Combined text+thinking stays busy
  - `test_claude_text_without_turn_end_is_idle_false_positive`: Text without turn closure stays busy (main regression test)
- All existing tests continue to pass (126 tests total)

## Outcomes
- Claude sessions now correctly stay `busy` during long thinking phases after initial text output
- Sessions only transition to `idle` when the turn explicitly closes with `turn_duration` or `api_error`
- Codex and Gemini behavior unchanged

## Tests
- `python3 -m unittest tests.test_idle_heuristics`
- `python3 -m unittest discover -s tests`
