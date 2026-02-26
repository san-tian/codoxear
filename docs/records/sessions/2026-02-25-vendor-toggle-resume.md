# Session: 2026-02-25 Vendor Toggle Resume

## Focus
Add a UI button to resume a session with alternate vendor env profiles.

## Requests
- Provide a button that switches between two vendors by resuming the session with new env vars.

## Actions Taken
- Added vendor profile parsing on the server and exposed labels via `/api/me`.
- Added `/api/sessions/<id>/resume` to spawn a web-owned broker with env overrides.
- Added UI vendor switch button + menu and wiring to resume the selected session.
- Documented vendor env configuration and new routes.

## Outcomes
- Users can resume a session with vendor A/B from the topbar when profiles are configured.
- Vendor labels are visible without exposing secrets to the client.

## Tests
- `python3 -m unittest discover -s tests`

## Notes
- Vendor env profiles are configured via JSON in `CODEX_WEB_VENDOR_A_ENV` and `CODEX_WEB_VENDOR_B_ENV`.
