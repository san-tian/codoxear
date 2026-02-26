# Session: 2026-02-22 Queue Release Uses Task Complete

## Focus
Stop deferred messages from releasing before the current turn actually finishes.

## Requests
- Fix cases where queued message b is sent while message a is still processing.

## Actions Taken
- Reverted `token_count` as a turn-end signal and rely on `task_complete` instead.
- Updated tests to reflect the correct turn-end behavior.
- Updated rollout log parsing docs.

## Outcomes
- Deferred queue release now waits for explicit task completion rather than mid-turn token updates.

## Tests
- `python3 -m unittest tests.test_server_chat_flags`

## Notes
- If a backend build does not emit `task_complete`, consider adding a secondary idle-based fallback.
