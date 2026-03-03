# Session: 2026-03-02 Portmap IPv6 Listener Fix

## Focus
Fix external access failures on public port `13780` caused by IPv6 clients.

## Requests
- Investigate why `13780` could not be opened from user-side network.

## Actions Taken
- Verified local daemon state:
  - `codoxear` and `codoxear-portmap` were `RUNNING`.
  - Local HTTP checks for `127.0.0.1:8743` and `127.0.0.1:13780` both returned `200`.
- Reproduced protocol-family gap:
  - `curl -4 http://127.0.0.1:13780/` returned `200`.
  - `curl -6 http://[::1]:13780/` returned connection refused.
  - `curl -6 http://[::1]:8743/` returned `200`.
- Confirmed root cause:
  - Existing public mapping process listened only on IPv4 (`0.0.0.0:13780`).
- Applied fix:
  - Added supervisord program `codoxear-portmap-v6` with mapping `[::]:13780 -> [::1]:8743`.
  - Reloaded supervisord config via `supervisorctl reread` and `supervisorctl update`.
- Updated docs:
  - `docs/flows/DEPLOYMENT.md`
  - `docs/flows/DEVELOPMENT.md`
  - `docs/records/WORK_RECORDS.md`

## Outcomes
- Public `13780` now works for both IPv4 and IPv6 clients while service remains on local `8743`.

## Tests
- `supervisorctl status codoxear codoxear-portmap codoxear-portmap-v6`
- `curl -4 -sS -o /dev/null -w '%{http_code}\n' http://127.0.0.1:13780/` => `200`
- `curl -6 -g -sS -o /dev/null -w '%{http_code}\n' http://[::1]:13780/` => `200`
