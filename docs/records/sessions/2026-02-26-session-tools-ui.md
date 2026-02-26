# Session: 2026-02-26 Session Tools UI

## Focus
Provide a one-click way to copy session commands and view live session output.

## Requests
- Add a UI button to copy a command for checking the current session.
- Optionally show a live view of the current session.

## Actions Taken
- Added a topbar session tools button that opens a modal.
- The modal shows status/resume commands with copy buttons and a live tail view.
- Documented the new UI behavior in the UI feature doc.

## Outcomes
- Users can copy SSH-friendly commands or watch live tail output without leaving the UI.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Live tail uses `/api/sessions/<id>/tail` polling while the modal is open.
