# Session: 2026-03-02 Claude Leading Bang Send Escape

## Focus
Fix Claude prompt sending so markdown image syntax that starts with `![...]` is not misinterpreted as a local shell command.

## Requests
- Investigate reports that Codoxear was sending some `!`-prefixed prompts to Claude as commands.

## Actions Taken
- Traced send/queue paths in UI and server:
  - `codoxear/static/app.js`: `sendText`, `queueServerMessage`, `saveQueueToServer`
  - `codoxear/server.py`: `SessionManager.send`, `queue_push`, `queue_set`
- Added Claude-specific outgoing prompt normalization in UI (`normalizeOutgoingTextForCli`):
  - If the prompt starts with markdown image syntax `![`, rewrite to `\![` before send/queue.
  - Applied consistently to immediate send, send-after-current queue push, and queue editor save.
- Added matching server-side guard (`_normalize_outgoing_text_for_cli`) so API/direct calls are protected too.
- Added regression tests in `tests/test_server_queue.py`:
  - Claude `/send` escapes leading `![`.
  - Normal shell-style `!cmd` remains unchanged.
  - Claude queue `push`/`set` also escape leading `![`.
- Updated docs:
  - `docs/features/ui.md`
  - `docs/features/server-and-api.md`
  - `docs/flows/TESTING.md`
  - `docs/records/WORK_RECORDS.md`

## Outcomes
- Claude prompts beginning with markdown image syntax are now sent as literal text instead of triggering local shell command mode.
- Protection is applied in both frontend and backend send/queue paths.

## Tests
- `python3 -m unittest tests.test_server_queue`
- `python3 -m unittest discover -s tests`
- `node --check codoxear/static/app.js`
