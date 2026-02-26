# Session: 2026-02-24 Queue Interrupt Guard

## Focus
Prevent deferred queue messages from interrupting active replies.

## Requests
- Queue messages should not send while the assistant is still replying.

## Actions Taken
- Added a minimum gate-hold window before idle-based release.
- Require a quiet period since the last incoming event before releasing the deferred gate.
- Centralized gate set/clear helpers for consistent timing.
- Updated UI docs to reflect the more conservative release policy.

## Outcomes
- Deferred queues no longer release immediately on brief idle gaps.

## Tests
- Not run (UI-only change).
