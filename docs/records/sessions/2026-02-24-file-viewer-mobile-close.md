# Session: 2026-02-24 File Viewer Mobile Close

## Focus
Keep the file viewer close button visible on mobile.

## Requests
- On mobile, the file viewer header is too crowded and hides the Close button.

## Actions Taken
- Pinned the Close button to the top-right of the file viewer header on small screens.
- Added header padding so wrapped actions do not overlap the Close button.
- Updated UI documentation.

## Outcomes
- The Close button remains accessible even when the action buttons wrap on mobile.

## Tests
- `python3 -m pytest` (fails: pytest is not installed in this environment).
- `python3 -m unittest tests.test_server_chat_flags`
