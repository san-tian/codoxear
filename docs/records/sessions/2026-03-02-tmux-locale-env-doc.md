# Session: 2026-03-02 Tmux Locale Env Doc

## Focus
Document required UTF-8 locale environment variables for tmux-backed Codoxear session launches.

## Requests
- Update documentation so tmux startup requirements include:
  - `LANG=en_US.UTF-8`
  - `LC_ALL=en_US.UTF-8`

## Actions Taken
- Updated tmux-related deployment guidance in `docs/flows/DEPLOYMENT.md`:
  - Added explicit UTF-8 locale env requirements under tmux-backed web sessions.
  - Added a `supervisord` note to place both vars in the `codoxear` program `environment=` line.
- Updated architecture/behavior docs:
  - `docs/features/server-and-api.md`
  - `docs/features/broker.md`
- Updated developer workflow docs:
  - `docs/flows/DEVELOPMENT.md`
- Updated `docs/records/WORK_RECORDS.md` to include this session record.

## Outcomes
- Tmux launch prerequisites now consistently require UTF-8 locale env in runtime/service configuration.
- Deployment and development docs align on the same locale requirement.

## Tests
- Not run (documentation-only changes).
