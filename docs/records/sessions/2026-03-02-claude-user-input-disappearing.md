# Session: 2026-03-02 Claude User Input Disappearing

## Focus
Fix Claude chat behavior where user turns could disappear from the visible chat list while assistant messages remained.

## Requests
- Investigate reports that Claude sessions only showed model output and user inputs disappeared.

## Actions Taken
- Investigated live API/session data and confirmed the issue was mainly UI windowing behavior:
  - Claude can emit many assistant rows in a single turn.
  - UI seed fetch (`init=1`) and DOM trimming could push older user rows out of view sooner than expected.
- Updated `codoxear/static/app.js`:
  - Added Claude-specific larger history seed limits:
    - `INIT_PAGE_LIMIT_CLAUDE_DESKTOP=200`
    - `INIT_PAGE_LIMIT_CLAUDE_MOBILE=80`
    - `OLDER_PAGE_LIMIT_CLAUDE=120`
  - Added Claude-specific larger DOM window (`CHAT_DOM_WINDOW_CLAUDE=520`).
  - Updated `trimRenderedRows({ fromTop: true })` to preserve recent user rows (`CHAT_DOM_MIN_USER_ROWS=24`) where possible before removing old rows.
- Updated docs:
  - `docs/features/ui.md`
  - `docs/records/WORK_RECORDS.md`

## Outcomes
- Claude sessions retain user turns in view longer during long assistant streams.
- Chat trimming now biases toward preserving recent user context rather than dropping user rows first.

## Tests
- `node --check codoxear/static/app.js`
