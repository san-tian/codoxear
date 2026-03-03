# Session: 2026-03-02 Chat File Link Open File Page

## Focus
Fix chat-rendered local file hyperlinks so clicking them opens the dedicated file page instead of a broken route.

## Requests
- File links are rendered as hyperlinks, but clicking cannot navigate correctly.
- Make link behavior open a standalone file page similar to using file viewer `Open` + `Pop out`.

## Actions Taken
- Updated markdown link rendering in `codoxear/static/app.js`:
  - Added local file-link parsing for absolute paths and common line/column suffixes.
  - Added full-screen file launch URL generation (`?file=...&mode=view&fullscreen=1`).
  - Rendered local file links with `data-file-path` metadata.
- Added delegated click handling:
  - Clicking a local file link opens the dedicated full-screen file page with current `session_id` when available.
  - Added popup-block fallback to open the inline file viewer.
- Updated docs:
  - `docs/features/ui.md`
  - `docs/flows/TESTING.md`
  - `docs/records/WORK_RECORDS.md`

## Outcomes
- Local absolute-path markdown links in chat/file markdown now open a dedicated file page (pop-out style) instead of navigating to invalid app routes.
- Existing http/https/mailto links keep original behavior.

## Tests
- `node --check codoxear/static/app.js`
- `python3 -m unittest discover -s tests` (110 tests, all pass)
- Manual UI check:
  1. Click a markdown link like `[app.js](/root/code/codoxear/codoxear/static/app.js)`.
  2. Confirm a full-screen file page opens with `?file=...&fullscreen=1`.
