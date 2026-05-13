# Codoxear Agent Entry

## Startup

- Read this file first.
- Read `docs/CONTEXT.md` for the public documentation index.
- Then read every repo Feature doc whose hook keywords match the task.
- Treat repo `docs/features/` and `docs/designs/` as the public project documentation source.
- If a private operator memory exists outside the repository, treat it as private working memory for records and workflow notes only.
- Do not create or maintain a repo-local canonical `memory/docs/` tree.

## Quick Commands

- Local daemon: `./scripts/codoxear-local start|stop|restart|status|logs`
- Frontend build: `cd frontend && npm run build`
- Rust tests: `cd backend-rs && cargo test`
- Rust release build: `cd backend-rs && cargo build --release --bins`

## Default Workflow

1. Read — read this `AGENTS.md`, `docs/CONTEXT.md`, and every repo Feature doc under `docs/features/` whose hook keywords in the Feature Index match the current task. If private operator memory is available, read relevant work records or workflow notes from there without copying private content into the repo.
2. Code — modify only files relevant to the request. Do not write fallbacks or safety nets.
3. Test — run the check closest to the change. If none exists, explain why and provide manual verification steps.
4. Update docs — sync the relevant `docs/features/<feature>.md` or `docs/designs/<design>.md`, update the Feature/Documentation Index if docs were added/renamed, and update private work records outside the repo when available.

## End-of-Turn Block (MANDATORY every turn)

> Step 1 — Files read: ...
> Step 2 — Files changed (code): ...  (or `none`)
> Step 3 — Tests run: `<cmd>` → `<result>`  (or `none — reason`)
> Step 4 — Docs updated: `docs/features/X.md`, `docs/designs/X.md`, ...  (or `none — reason`)

## Definition of Done

- Requested change implemented and verifiable
- Self-review done, risks noted
- Tests run, or explicit reason for skipping
- Relevant `docs/features/<feature>.md` or `docs/designs/<design>.md` updated, plus `AGENTS.md` Feature Index/Documentation Index if docs were added/renamed

## Feature Index

| Feature | When to read (hook keywords) |
|-------|------------------------------|
| `docs/features/server-and-api.md` | api, routes, auth, session endpoint, queue, harness, diagnostics, file read, inject_file, nova route |
| `docs/features/session-and-broker.md` | broker, sessiond, pty, socket, send, enqueue, interrupt, tmux, resume, ownership, Codex, Pi |
| `docs/features/ui.md` | static/app.js, static/app.css, legacy UI, /nova, browser, button, composer, mobile, tmux attach |
| `docs/features/replatforming.md` | replatforming, frontend, pretext, preact, vite, rust, axum, sse, transcript, backend-rs, nova-preview |
| `docs/features/voice-push.md` | voice_push, TTS, notification, web push, VAPID, service-worker, narration, mobile push, audio stream |
| `docs/features/rollout-log-parsing.md` | rollout_log, JSONL, messages, transcript, idle, token usage, tool call, ask_user, pi_log |

## Documentation Index

- `docs/CONTEXT.md` — public documentation index and project context.
- `docs/README.md` — short pointer to the documentation index.
- `docs/features/server-and-api.md` — Rust web server, auth, API route families, and route verification.
- `docs/features/session-and-broker.md` — Broker/session lifecycle, PTY/socket flow, tmux ownership, queue, and resume behavior.
- `docs/features/ui.md` — Browser UI surface, legacy static assets, notification service worker, and manual UI checks.
- `docs/features/replatforming.md` — Active Nova preview frontend/backend scaffold, transcript layout, API wiring, and verification.
- `docs/features/voice-push.md` — Voice/TTS, notification feed, service worker, VAPID subscription, and mobile push behavior.
- `docs/features/rollout-log-parsing.md` — Rollout/Pi log parsing, chat event normalization, idle, and token extraction.
- `docs/designs/extension-display-protocol.md` — `codoxear_display` v1 protocol for plugin-authored transcript progress/status display.
- `docs/designs/voice-push-implementation-plan.md` — Original voice announcements and mobile push implementation plan.
