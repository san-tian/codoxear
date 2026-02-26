# Session: 2026-02-25 Standardize Startup Method

## Focus
Confirm how the server is started on this host and standardize docs to a single startup method.

## Requests
- Check how codoxear-server is started on this host.
- Fix the documentation to a single, standard startup path.

## Actions Taken
- Verified the running process is `python3 -m codoxear.server` with cwd `/root/code/codoxear`.
- Updated development and deployment docs to standardize on `scripts/codoxear-server-dev`.
- Removed systemd startup instructions from the standard flow.

## Outcomes
- Documentation now points to one startup method to keep `.env` loading consistent.

## Tests
- Not run (docs-only change).

## Notes
- Current server process parent is PID 1, indicating it was started as a daemonized process.
