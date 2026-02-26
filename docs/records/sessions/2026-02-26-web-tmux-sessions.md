# Session: 2026-02-26 Web Tmux Sessions

## Focus
Launch web-owned sessions inside tmux for easier SSH monitoring.

## Requests
- Start all web-launched sessions under tmux so they can be attached via SSH.

## Actions Taken
- Wrapped web session spawn in tmux when `CODEX_WEB_TMUX=1`, with a unique tmux session name per broker.
- Added `tmux_name` to broker metadata and `/api/sessions` output.
- Exposed a tmux attach command in the session tools UI.
- Added a unit test to confirm `tmux_name` is surfaced in session listings.

## Outcomes
- Web sessions are launched in tmux when enabled, and the UI can show the attach command.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- If tmux is not installed, the server falls back to direct broker spawn with a warning.
