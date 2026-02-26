# Session: 2026-02-24 File Viewer Mobile Header Scroll

## Focus
Reduce crowding in the file viewer header on iOS.

## Requests
- The mobile file viewer header is still too cramped; actions and Close overlap.

## Actions Taken
- Stacked the file viewer header on small screens and made the action row horizontally scrollable.
- Tightened mobile action button padding and font size.
- Updated UI documentation.

## Outcomes
- Header actions no longer crowd out the Close button on iOS.

## Tests
- `python3 -m pytest` (fails: pytest is not installed in this environment).
- `python3 -m unittest tests.test_server_chat_flags`
