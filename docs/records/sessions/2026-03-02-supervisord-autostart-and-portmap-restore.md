# Session: 2026-03-02 Supervisord Autostart and Portmap Restore

## Focus
Restore Codoxear auto-start after reboot and standardize runtime networking to local `8743` with public compatibility on `13780`.

## Requests
- Investigate why Codoxear did not auto-start after reboot.
- Restore daemon auto-start.
- Keep Codoxear on `8743` and provide public `13780 -> 8743` mapping.
- Clean deployment/development docs to remove stale "service runs on 13780" wording.

## Actions Taken
- Verified runtime manager is `supervisord` (not `systemd`) and confirmed root cause:
  - `/mlplatform/supervisord/supervisord.conf` no longer had a `[program:codoxear]` section.
  - `supervisorctl status codoxear` returned `ERROR (no such process)`.
- Restored supervisord program:
  - Added `[program:codoxear]` with:
    - `directory=/root/code/codoxear`
    - `command=/usr/bin/env bash -lc 'scripts/codoxear-server-dev'`
    - `autostart=true`, `autorestart=true`
    - `environment=PYTHONUNBUFFERED="1",GEMINI_BIN="/usr/local/bin/gemini-web"`
- Added host-level public port mapping under supervisord:
  - Created `/usr/local/bin/tcp-port-forward` (simple TCP forwarder).
  - Added `[program:codoxear-portmap]` mapping `0.0.0.0:13780 -> 127.0.0.1:8743`.
  - Enabled `autostart=true` and `autorestart=true`.
- Applied daemon config:
  - `supervisorctl reread`
  - `supervisorctl update`
- Updated docs:
  - `docs/flows/DEPLOYMENT.md`
  - `docs/flows/DEVELOPMENT.md`
  - `docs/records/WORK_RECORDS.md`

## Outcomes
- Codoxear now auto-starts again under supervisord.
- Service listens on `8743`.
- Public compatibility path is restored via supervisord-managed `13780 -> 8743` forwarding.
- Deployment/development docs now describe `8743` as the service port and document the `13780` public mapping explicitly.

## Tests
- `supervisorctl status codoxear codoxear-portmap`
- `curl -sS -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8743/` => `200`
- `curl -sS -o /dev/null -w '%{http_code}\n' http://127.0.0.1:13780/` => `200`
