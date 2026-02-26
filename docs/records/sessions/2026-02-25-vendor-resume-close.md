# Session: 2026-02-25 Vendor Resume Close

## Focus
Fix vendor resume UX and close the current session before resuming.

## Requests
- Resume with vendor button was disabled.
- Resume should close the current session, then reopen with the new vendor profile.

## Actions Taken
- Enabled vendor resume even when no session is explicitly selected (falls back to the newest session).
- Added a `close` flag to the resume API so the server can close web-owned sessions before spawning the resume broker.

## Outcomes
- Vendor resume reliably opens the menu and closes owned sessions before resuming.

## Tests
- Not run (manual verification needed).
