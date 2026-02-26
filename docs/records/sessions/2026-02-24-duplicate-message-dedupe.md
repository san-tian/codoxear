# Session: 2026-02-24 Duplicate Message Dedupe

## Focus
Reduce intermittent duplicate user/assistant messages in the chat UI.

## Requests
- Fix cases where input and replies appear twice.

## Actions Taken
- Added signature-based near-duplicate suppression alongside exact event-key dedupe.
- Applied duplicate filtering during initial render to avoid re-adding identical events.

## Outcomes
- UI now skips same-role, same-text events that repeat within a short window.

## Tests
- Not run (manual verification needed).
