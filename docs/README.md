# Codoxear Documentation

This directory contains public project documentation that should travel with the repository.

## Feature Docs

- `features/server-and-api.md` — Rust web server, auth, API route families, and route verification.
- `features/session-and-broker.md` — Broker/session lifecycle, PTY/socket flow, tmux ownership, queue, and resume behavior.
- `features/ui.md` — Browser UI surface, legacy static assets, notification service worker, and manual UI checks.
- `features/replatforming.md` — Active Nova preview frontend/backend scaffold, transcript layout, API wiring, and verification.
- `features/voice-push.md` — Voice/TTS, notification feed, service worker, VAPID subscription, and mobile push behavior.
- `features/rollout-log-parsing.md` — Rollout/Pi log parsing, chat event normalization, idle, and token extraction.

## Design Docs

- `designs/extension-display-protocol.md` — `codoxear_display` v1 protocol for plugin-authored transcript progress/status display.
- `designs/voice-push-implementation-plan.md` — original voice announcements and mobile push implementation plan.

Operational work records, local workflow notes, and private machine-specific memory are intentionally kept outside this repository.
