# Session: 2026-02-24 iOS File Viewer Tap

## Focus
Fix iOS Safari tap handling for opening the file viewer.

## Requests
- File viewer does not open on iOS.

## Actions Taken
- Added a tap helper that binds `touchend` + `click` with a short guard window.
- Applied the helper to the file button and workspace file buttons so taps always open the viewer.

## Outcomes
- iOS taps should reliably open the file viewer modal.

## Tests
- Not run (manual on iOS needed).
