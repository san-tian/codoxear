# Session: 2026-02-26 Session Draft Summary Indicator

## Focus
Show draft state in the session list where the last user summary normally appears.

## Requests
- Draft indicator should be yellow.
- Draft indicator should replace the last user summary line in the sidebar.

## Actions Taken
- Moved draft indicator into the session summary line, replacing the last user summary when a draft exists.
- Updated draft save/clear and summary persistence to keep the line in sync.
- Styled the draft summary line with a yellow color.

## Outcomes
- Sessions with drafts show a yellow `draft` line in place of the last user summary.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Summary text returns once the draft is cleared.
