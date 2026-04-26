# Codoxear Agent Entry

> Canonical memory: `/vePFS-Mindverse/user/intern/ccss/docs/Projects/codoxear-nova/AGENTS.md`
> Source CWD: `/vePFS-Mindverse/user/intern/ccss/codoxear`

## Startup

- Read the canonical memory file first.
- Then read `Records/WORK_RECORDS.md` and every Feature whose hook keywords match the task.
- Treat `/vePFS-Mindverse/user/intern/ccss/docs/Projects/codoxear-nova/` as the canonical project memory source.
- Do not create or maintain a repo-local canonical `memory/docs/` tree.
- Prefix shell commands with `rtk`, per `/root/.codex/RTK.md`.

## Quick Commands

- Install/update: `.venv/bin/python -m pip install -e . pytest`
- Dev server: `./scripts/codoxear-server-dev`
- Local daemon: `./scripts/codoxear-local start|stop|restart|status|logs`
- Python tests: `.venv/bin/python -m pytest -q`
- Frontend build: `cd frontend && npm run build`
- Rust tests: `cd backend-rs && cargo test`
- Rust release build: `cd backend-rs && cargo build --release`

## Default Workflow

1. Read — read this `AGENTS.md`, `Records/WORK_RECORDS.md`, and every Feature whose hook keywords in the Feature Index match the current task. If a relevant work-record entry links to a work-topic narrative, read that narrative too. Then check `Workflows/` and both global and project `Skills/`.
2. Code — modify only files relevant to the request. Do not write fallbacks or safety nets.
3. Test — run the check closest to the change. If none exists, explain why and provide manual verification steps.
4. Update docs — sync the relevant `Features/<feature>.md`, update the `AGENTS.md` Feature Index and Documentation Index if docs were added/renamed, and append one entry to `Records/WORK_RECORDS.md`.

## End-of-Turn Block (MANDATORY every turn)

> Step 1 — Files read: ...
> Step 2 — Files changed (code): ...  (or `none`)
> Step 3 — Tests run: `<cmd>` → `<result>`  (or `none — reason`)
> Step 4 — Docs updated: `Features/X.md`, `Records/WORK_RECORDS.md`, ...  (or `none — reason`)

## Definition of Done

- Requested change implemented and verifiable
- Self-review done, risks noted
- Tests run, or explicit reason for skipping
- Relevant `Features/<feature>.md`, `AGENTS.md` Feature Index/Documentation Index (if docs added/renamed), and `Records/WORK_RECORDS.md` updated
