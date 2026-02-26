# Session: 2026-02-25 File Viewer Tab Title

## Focus
Set the pop-out file viewer tab title to the active filename.

## Requests
- Update the pop-out file window to show the filename in the browser tab title.

## Actions Taken
- Added fullscreen-only tab title updates using the active file path.
- Updated the UI feature doc to note the new tab title behavior.

## Outcomes
- Pop-out file viewer tabs show the current filename in the browser title bar.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Title updates reuse the file basename so the tab label stays concise.
