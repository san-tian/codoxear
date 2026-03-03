# Testing

Tests are unittest-based and focus on log parsing, idle heuristics, URL prefix handling, and server behavior.

## Run all tests
`python3 -m unittest discover -s tests`

## If pytest is unavailable
Pytest can still run the unittest suite if you install it, but it's optional:
`python3 -m pytest`

## Run a single test file
`python3 -m unittest tests.test_server_chat_flags`
`python3 -m unittest tests.test_server_queue`
`python3 -m unittest tests.test_file_history`
`python3 -m unittest tests.test_broker_proc_rollout`
`python3 -m unittest tests.test_broker_busy_state`
`python3 -m unittest tests.test_broker_spawn_env`
`python3 -m unittest tests.test_idle_heuristics`
`python3 -m unittest tests.test_cli_support`
`python3 -m unittest tests.test_util_gemini_offset`
`python3 -m unittest tests.test_server_spawn_cli`
`python3 -m unittest tests.test_update_check`
`python3 -m unittest tests.test_tail_sanitize`
`python3 -m unittest tests.test_sessions_pending_log_idle`

## Notes
- Tests rely on static fixtures and do not require a running broker.
- When adding new behavior around log parsing or idle detection, extend tests in `tests/`.
- Claude and Gemini support coverage currently lives in `test_broker_proc_rollout`, `test_broker_spawn_env`, `test_cli_support`, `test_idle_heuristics`, `test_server_chat_flags`, `test_server_spawn_cli`, and `test_util_gemini_offset`.
- Tail sanitization coverage lives in `test_tail_sanitize` (including Claude live-tail noise filtering).
- Claude thinking idle false-positive regression coverage lives in `test_idle_heuristics` (tests that Claude sessions stay busy during long thinking phases after initial text output).
- UI-only behavior in `codoxear/static/app.js` is not covered by the Python unittest suite; validate manually in browser.

## Manual UI checks
- File-link launch behavior:
  1. Open a chat message containing a markdown link to an absolute local path (for example `[app.js](/root/code/codoxear/codoxear/static/app.js)`).
  2. Click the link.
  3. Confirm it opens a dedicated full-screen file page (`?file=...&fullscreen=1`) instead of navigating to a broken route.
- Claude leading-`!` send guard:
  1. Open a Claude session and send a prompt that starts with markdown image syntax, for example `![plot](figures/a.svg)`.
  2. Confirm Codoxear sends it as literal text (escaped as `\![plot](...)`) instead of triggering Claude local shell-command mode.
- Claude `send now` ordering:
  1. In a running Claude turn, use `Send now` to inject a new user prompt before the previous reply fully ends.
  2. Confirm the final chat order keeps the injected user message before that turn's assistant chunks, instead of showing assistant chunks first and user text later.
- Sidebar CLI toggle removal:
  1. Open the main UI and check the sidebar header action row.
  2. Confirm the old `Codex/Claude/Gemini` toggle button is not shown.
  3. Click `New session` and `Duplicate session`, then confirm both still open the CLI choice modal before session creation.
- Sidebar collapse/open button:
  1. In desktop width, click the topbar menu button and confirm the left sidebar collapses/expands.
  2. In mobile width, click the same button and confirm it opens/closes the sidebar drawer.
