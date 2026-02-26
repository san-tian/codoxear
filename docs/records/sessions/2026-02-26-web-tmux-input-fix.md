# Session: 2026-02-26 Web Tmux Input Fix

## Focus
Restore web session startup after enabling tmux input.

## Requests
- `broker` crashed with `NameError: allow_input` during startup.

## Actions Taken
- Defined `allow_input` in `Broker.run` before use.

## Outcomes
- Web sessions can start again; tmux input support remains enabled.

## Tests
- `python3 -m unittest discover -s tests`
