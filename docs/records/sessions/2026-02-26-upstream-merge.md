# Session: 2026-02-26 Upstream Merge

## Focus
Merge the latest `upstream/main` changes into `dev`.

## Requests
- Pull upstream updates and merge them locally.

## Actions Taken
- Fetched `upstream/main` and merged into `dev`.
- Resolved the `codoxear/static/app.js` conflict by keeping relay status logic with the upstream base URL resolver.

## Outcomes
- `dev` now includes upstream rollout log discovery changes and new broker rollout tests.
- Test suite still fails under Python 3.8 due to `list[dict]` annotations in tests.

## Tests
- `python3 -m unittest discover -s tests`
