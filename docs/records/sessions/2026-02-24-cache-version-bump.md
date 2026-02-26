# Session: 2026-02-24 Cache Version Bump

## Focus
Clear stale session caches that caused missing history until manual refresh.

## Requests
- Fix missing history when switching sessions without pressing Refresh.

## Actions Taken
- Bumped chat cache key to `codexweb.cache.v4.*` to invalidate all old caches.

## Outcomes
- Session switches now rebuild history from server instead of stale cache.

## Tests
- Not run (cache key change).
