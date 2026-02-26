# Session: 2026-02-26 Web Tmux Input

## Focus
Allow interactive input when attaching to tmux sessions spawned by the web UI.

## Requests
- Enable typing inside tmux for web-launched sessions.

## Actions Taken
- Added `CODEX_WEB_TMUX_INTERACTIVE=1` default when spawning tmux-backed web sessions.
- Updated the broker to allow stdin passthrough and raw mode when tmux interactive is enabled.

## Outcomes
- Attaching to tmux for a web session now supports interactive input.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Input passthrough only activates when stdin is a TTY.
