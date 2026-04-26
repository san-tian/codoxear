# codoxear

> CWD: `/vePFS-Mindverse/user/intern/ccss/codoxear`

## Canonical Memory

- Project memory entry: `/vePFS-Mindverse/user/intern/ccss/docs/Projects/codoxear-yiwenlu66/AGENTS.md`
- Work records: `/vePFS-Mindverse/user/intern/ccss/docs/Projects/codoxear-yiwenlu66/Records/WORK_RECORDS.md`
- Shared memory root: `/vePFS-Mindverse/user/intern/ccss/docs/AGENTS.md`

## Purpose

Local clone of `https://github.com/yiwenlu66/codoxear.git`, used to serve the Codoxear Web UI on this machine through the existing `127.0.0.1:13780 -> 127.0.0.1:8743` port forwarder.

## Standard startup (this host)

- Runtime config: `.env` (gitignored), currently binds backend to `0.0.0.0:8743` with password auth.
- External/local entry: `http://127.0.0.1:13780/` (forwarded to backend `8743`).
- Daemon helper: `./scripts/codoxear-local start|stop|restart|status|logs`.
- Python env: `.venv/`; install/update with `.venv/bin/python -m pip install -e . pytest`.
- Frontend build: `cd frontend && npm run build`.
- Rust release build: `cd backend-rs && cargo build --release`.

## Structure at a glance

- `README.md` — usage, configuration, and route split.
- `codoxear/` — Python package, server, broker, rollout parsing, voice push, static assets.
- `frontend/` — Nova preview frontend (`Preact + TypeScript + Vite + Pretext`).
- `backend-rs/` — Nova preview Rust backend (`Axum + SSE`).
- `scripts/` — local daemon/dev/resume helpers.
- `tests/` — Python tests.

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

## Feature Index

| Feature | When to read (hook keywords) |
|-------|------------------------------|
| `Features/server-and-api.md` | server.py, api, routes, auth, session endpoint, queue, harness, diagnostics, file read, inject_file, nova route |
| `Features/session-and-broker.md` | broker, sessiond, pty, socket, send, enqueue, interrupt, tmux, resume, ownership, Codex, Pi |
| `Features/ui.md` | static/app.js, static/app.css, legacy UI, /nova, browser, button, composer, mobile, tmux attach |
| `Features/replatforming.md` | replatforming, frontend, pretext, preact, vite, rust, axum, sse, transcript, backend-rs, nova-preview |
| `Features/voice-push.md` | voice_push, TTS, notification, web push, VAPID, service-worker, narration, mobile push, audio stream |
| `Features/rollout-log-parsing.md` | rollout_log, JSONL, messages, transcript, idle, token usage, tool call, ask_user, pi_log |

## Documentation Index

- `/vePFS-Mindverse/user/intern/ccss/docs/Projects/codoxear-yiwenlu66/AGENTS.md` — canonical project memory entry and full Documentation Index.
- `/vePFS-Mindverse/user/intern/ccss/docs/Projects/codoxear-yiwenlu66/Records/WORK_RECORDS.md` — newest-first work record index.
