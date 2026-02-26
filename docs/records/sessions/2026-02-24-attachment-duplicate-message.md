# Session: 2026-02-24 Attachment Duplicate Message

## Focus
Prevent duplicate user messages when image attachments are present.

## Requests
- Stop user messages from appearing twice after attaching an image.

## Actions Taken
- Filtered upload-path placeholder lines injected during image attachment.
- Normalized user events before pending reconciliation and rendering.
- Updated UI docs to note upload-path filtering.

## Outcomes
- Image attach no longer produces duplicate user message bubbles.

## Tests
- Not run (UI-only change).
