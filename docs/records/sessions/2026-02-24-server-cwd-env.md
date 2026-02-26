# Session: 2026-02-24 Server CWD and .env Location

## Focus
Document where the running server loads `.env` on this host.

## Requests
- Identify how the server is running and document the `.env` location.

## Actions Taken
- Checked the running server process.
- Recorded the working directory used by the server.
- Updated deployment docs to include the observed `.env` path.

## Outcomes
- Documentation now points to the active server working directory for `.env`.

## Tests
- Not run (docs-only change).

## Notes
- Observed process: `python3 -m codoxear.server` with cwd `/root/code/codoxear` on 2026-02-24.
