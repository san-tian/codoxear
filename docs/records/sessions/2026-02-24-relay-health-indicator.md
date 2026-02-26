# Session: 2026-02-24 Relay Health Indicator

## Focus
Expose Codex relay health in the UI.

## Requests
- Add a breathing light to show whether the relay is healthy.

## Actions Taken
- Added a topbar relay status indicator driven by API success/failure and offline detection.
- Wired breathing green for healthy and yellow/red for degraded/offline.

## Outcomes
- Users can see relay health at a glance without opening logs.

## Tests
- Not run (manual verification needed).
