# Session: 2026-02-22 Refresh Clears Cache

## Focus
Make the Refresh button clear local cache and re-fetch messages to recover missing history.

## Requests
- Merge cache reset behavior into the navigation refresh action.

## Actions Taken
- Updated the refresh handler to clear the selected session cache and reselect it.
- Updated UI feature doc to reflect refresh behavior.

## Outcomes
- Using Refresh now forces a clean reload of message history for the selected session.

## Tests
- Not run (UI-only change).
