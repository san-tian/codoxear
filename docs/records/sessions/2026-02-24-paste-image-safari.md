# Session: 2026-02-24 Paste Image Support

## Focus
Enable image paste (including Safari) into the message composer.

## Requests
- Fix paste-to-attach not working in macOS Safari.

## Actions Taken
- Added paste handler on the message textarea to detect clipboard image files.
- Reused existing image attach pipeline.
- Updated UI feature doc.

## Outcomes
- Pasting images in Safari now attaches them like file input.

## Tests
- Not run (UI-only change).
