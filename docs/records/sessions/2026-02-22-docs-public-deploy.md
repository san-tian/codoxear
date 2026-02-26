# Session: 2026-02-22 Docs Accuracy and Public Deployment

## Focus
Align documentation with actual deployment: server runs on this machine but is accessible via the public internet.

## Requests
- Ensure all documentation statements are accurate.
- Emphasize the public-access deployment model.

## Actions Taken
- Updated deployment doc to describe public access, reverse proxy, and security expectations.
- Corrected broker and sessiond docs for socket naming and metadata paths.
- Clarified that sessiond does not currently queue inputs on send.

## Outcomes
- Documentation now reflects the public-access deployment and actual socket metadata behavior.

## Tests
- None (documentation-only changes).

## Notes
- Future network or auth changes should update `docs/flows/DEPLOYMENT.md` and `docs/features/ROUTES.md`.
