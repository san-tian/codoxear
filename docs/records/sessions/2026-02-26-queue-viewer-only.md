# Session: 2026-02-26 Queue Viewer Only

## Focus
Keep queued messages visible only in the queue viewer, not in the chat stream.

## Requests
- Do not show queued messages as chat bubbles; show them only in the queue area.

## Actions Taken
- Removed the queued-message local echo bubble from the chat flow.
- Updated the UI doc to clarify queued messages remain in the queue viewer until drained.

## Outcomes
- Queueing a message updates the queue list/badge without adding a chat bubble.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Queue viewer updates still rely on `/api/sessions/<id>/queue` responses.
