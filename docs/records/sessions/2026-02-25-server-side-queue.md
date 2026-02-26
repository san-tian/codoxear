# Session: 2026-02-25 Server-Side Queue

## Focus
Move queued messages to a server-side queue that continues when the browser closes.

## Requests
- Make the queue server-side and use it instead of Harness mode.

## Actions Taken
- Added broker/sessiond queue commands and drain-on-idle behavior.
- Added `/api/sessions/<id>/queue` endpoints and wired the UI to use them.
- Removed Harness controls from the UI and updated docs for the new queue flow.

## Outcomes
- Queued messages are stored in the broker and continue without the browser.
- The queue viewer now reflects server state and edits sync back to the server.

## Tests
- `python3 -m unittest discover -s tests` (failed: `TypeError: 'type' object is not subscriptable` under Python 3.8)
- `python3 -m unittest tests.test_server_queue`
