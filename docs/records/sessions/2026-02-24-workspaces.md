# Session: 2026-02-24 Workspaces

## Focus
Group sessions by shared `cwd` and surface workspace-level file lists.

## Requests
- Add a workspace concept that shows sessions sharing the same `cwd` along with files opened in those sessions.

## Actions Taken
- Added workspace grouping in the sidebar session list based on `cwd`.
- Rendered workspace-level file lists aggregated across sessions in the same `cwd`.
- Updated UI docs to describe workspace grouping.

## Outcomes
- Sessions are grouped by workspace with a shared file list per `cwd`.

## Tests
- `python3 -m pytest` (failed: pytest not installed)
- `python3 -m unittest tests.test_server_chat_flags`
