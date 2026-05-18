# Codoxear Context

This is the public documentation index for the Codoxear project.

## Project Shape

Codoxear is a local web handoff layer for live Codex/Pi CLI agent sessions. The Rust backend owns the public HTTP surface, session discovery, broker integration, queue/harness workers, voice/push workers, and static shell delivery. The Nova frontend is a Preact/Vite browser UI for mobile and desktop control of those sessions.

## Feature Docs

- `features/server-and-api.md` — Rust web server, auth, API route families, and route verification.
- `features/session-and-broker.md` — Broker/session lifecycle, PTY/socket flow, tmux ownership, queue, and resume behavior.
- `features/ui.md` — Browser UI surface, legacy static assets, notification service worker, and manual UI checks.
- `features/replatforming.md` — Active Nova preview frontend/backend scaffold, transcript layout, API wiring, and verification.
- `features/voice-push.md` — Voice/TTS, notification feed, service worker, VAPID subscription, and mobile push behavior.
- `features/rollout-log-parsing.md` — Rollout/Pi log parsing, chat event normalization, idle, and token extraction.

## Design Docs

- `designs/extension-display-protocol.md` — `codoxear_display` v1 protocol for plugin-authored transcript progress/status display.
- `designs/schedules.md` — Daemon-owned scheduled agent runs, target modes, lifecycle, API, UI, and skill integration plan.
- `designs/voice-push-implementation-plan.md` — Original voice announcements and mobile push implementation plan.

## Private Memory Boundary

Operational work records, local workflow notes, and machine-specific memory are intentionally kept outside this repository. Public docs should describe the project, not an operator's local working path.
