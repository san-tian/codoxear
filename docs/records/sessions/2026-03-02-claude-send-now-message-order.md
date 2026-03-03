# Session: 2026-03-02 Claude Send Now Message Order

## Focus
Fix Claude chat message ordering when a new user prompt is injected with `Send now` before the current reply ends.

## Requests
- Claude chat order was incorrect in mid-reply `Send now` scenarios.
- Example wrong order: `q1, a1_q1, a2_q1, a3_q1, a1_q2, a2_q2, q2, a3_q2`.

## Actions Taken
- Investigated frontend render path in `codoxear/static/app.js`:
  - Live append path (`appendEvent`)
  - Initial/history render paths (`startInitialRender`, `prependOlderEvents`)
  - Pending-user reconciliation (`consumePendingUserIfMatches`)
  - Session cache event storage (`replaceCacheEvents`, `appendCacheEvents`)
- Implemented stable timestamp-based ordering:
  - Added event comparator/sorter (`compareChatEventsByTs`, `sortChatEventsByTs`).
  - Added chronological DOM insertion helper (`insertRowChronologically`) with user-before-assistant tie-break on equal timestamps.
  - `appendEvent` now inserts rows chronologically instead of always appending to bottom.
  - Pending user rows are re-positioned after ack when real `ts` arrives.
  - When a pending local row is not present in DOM, ack matching now falls back to rendering the real user event instead of swallowing it.
  - Initial and older-page rendering now sort event batches by timestamp before building rows.
  - Cache event lists are normalized to the same timestamp order.
- Updated docs:
  - `docs/features/ui.md`
  - `docs/flows/TESTING.md`
  - `docs/records/WORK_RECORDS.md`

## Outcomes
- Mid-reply `Send now` user turns no longer appear behind subsequent assistant chunks when timestamps indicate the user event happened first.
- Pending-to-acked user row transitions keep consistent visual ordering.

## Tests
- `node --check codoxear/static/app.js`
- `python3 -m unittest discover -s tests`
