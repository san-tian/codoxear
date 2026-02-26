# Session: 2026-02-24 Fixed Startup

## Focus
Provide a fixed, stable server startup flow that is convenient for development.

## Requests
- Make the server startup directory fixed and easy to use for dev.
- Document how the running server loads `.env`.

## Actions Taken
- Added `scripts/codoxear-server-dev` to always start from repo root.
- Added a sample systemd user unit `scripts/codoxear-server.service` with a fixed `WorkingDirectory`.
- Updated development and deployment docs with fixed-start instructions.

## Outcomes
- Startup can be pinned to a known working directory for consistent `.env` loading.

## Tests
- Not run (scripts/docs-only change).
