# Session: 2026-02-25 Provider Health Checks

## Focus
Provide a health check control for all configured provider relay profiles.

## Requests
- Add a button or breathing indicator to query health for all relay profiles.

## Actions Taken
- Added `/api/providers/health` to probe each provider profile.
- Added a settings UI health button with per-profile status dots.
- Documented the new route and UI behavior.

## Outcomes
- Users can trigger a health probe and see status for every configured relay profile.

## Tests
- `python3 -m pytest` (fails: pytest is not installed in this environment).
- `python3 -m unittest tests.test_provider_health`
