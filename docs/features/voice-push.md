# Voice Push

## Purpose

Voice push provides assistant-response summarization, TTS/audio stream support, notification feed state, VAPID-backed web-push subscriptions, and service-worker delivery for mobile devices.

## Key Files

- `backend-rs/src/voice_worker.rs` — daemon-mode Rust voice delivery worker for `voice_inbox/*.json`, OpenAI-compatible summary/TTS calls, VAPID Web Push sends, delivery ledger updates, ffmpeg/HLS audio merging, and `voice_runtime.json` snapshots.
- `backend-rs/src/runtime.rs` and `backend-rs/src/routes.rs` — Rust-owned voice settings, listener heartbeat, notification feed/message, audio playlist/segment, voice scan worker, `config.toml`, restart, and notification-subscription routes that share the same voice-push backing files.
- `codoxear/static/service-worker.js` — displays pushed notifications and reads canonical payload fields.
- `frontend/src/app.tsx` — Nova preview settings UI for desktop notifications and mobile push.

## Call Chain

1. In daemon mode, Rust runs `CODOXEAR_ENABLE_VOICE_SCAN=1`, discovers live socket sidecar sessions, tails newly appended rollout/Pi log deltas from their current end offsets, classifies assistant narration/final-response messages, and either records already-terminal messages directly into `voice_delivery_ledger.json` or atomically writes one JSON file per remaining message into `APP_DIR/voice_inbox/`.
2. Rust also runs `CODOXEAR_ENABLE_VOICE_WORKER=1`, consumes `voice_inbox/*.json`, summarizes final responses and narration through OpenAI-compatible `/chat/completions`, calls `/audio/speech` for AAC TTS, sends mobile VAPID Web Push payloads, merges audio into HLS segments with `ffmpeg`/`ffprobe`, and updates the shared delivery/subscription/runtime files.
3. Browser polling reads `/api/notifications/feed`; audio-listener heartbeats write to the shared listener store through `/api/audio/listener`; mobile push uses service-worker subscription data posted to `/api/notifications/subscription`.
4. Web-push payloads use `notification_text` as the canonical display field.

## Current Behavior

- Summarization prompts target about 15 words for narration and about 30 words for final-response mobile notifications.
- VAPID keys are generated/stored locally when needed; the public key is surfaced through notification settings responses.
- Subscription records include device class/label, user agent, enabled flag, and last delivery/failure metadata.
- Nova preview can register the service worker, subscribe to PushManager, and toggle the current mobile subscription.
- The local daemon no longer launches `python -m codoxear.voice_runtime`; Rust owns live voice scanning, OpenAI-compatible summary/TTS work, Web Push delivery, ffmpeg/HLS audio merging, and runtime snapshot persistence.
- Python `codoxear/voice_push.py`, `codoxear/voice_runtime.py`, their py-vapid/pywebpush dependencies, and Python direct voice route bodies were removed; voice behavior is no longer duplicated in Python.
- Rust voice scanning uses Python-compatible delivery message IDs, skips source messages already present in `voice_delivery_ledger.json`, mutes resumed sessions while sidecar metadata still includes `resume_session_id`, and uses `CODEX_WEB_VOICE_PUSH_SWEEP_SECONDS` for scan cadence.
- Rust voice scanning now directly finalizes disabled narration and final responses that have no TTS API key and no enabled mobile push subscription by writing skipped/raw rows to `voice_delivery_ledger.json`, avoiding the Python companion for messages that do not need external summary, TTS/audio, or Web Push work.
- `voice_inbox/*.json` is now an internal Rust handoff queue between scanner and delivery worker, not a Python companion boundary.
- The Rust delivery worker reads `voice_settings.json`, `push_subscriptions.json`, and `voice_listeners.json` directly, writes delivery and subscription metadata back to the shared JSON stores, and persists a compact `voice_runtime.json` snapshot of listener/audio counters.
- Public `GET|POST /api/settings/voice`, `GET /api/notifications/message`, `GET /api/notifications/feed`, `POST /api/audio/listener`, `GET /api/audio/live.m3u8`, and `GET /api/audio/segments/*` stay on Rust. `POST /api/audio/listener` writes the shared `voice_listeners.json` heartbeat store directly, so browser listeners affect live narration/TTS without a Rust-to-Python proxy hop.

## Verification

- Automated Rust daemon voice scanner: `cd backend-rs && cargo test voice_scan -- --test-threads=1`
- Automated Rust daemon voice delivery worker: `cd backend-rs && cargo test voice_worker -- --test-threads=1`
- Automated Rust route safety: `cd backend-rs && cargo test`
- Manual: authenticated `POST /api/audio/listener` should update `audio.active_listener_count` immediately in `/api/settings/voice` without relying on a Python public-route proxy.
- Manual: on a supported mobile browser over HTTPS/Tailscale, enable mobile push in Settings and confirm notifications arrive for final responses.

## Known Risks

- Browser push requires HTTPS/secure context and user permission; local HTTP testing is limited.
- External TTS/summarization APIs require valid credentials and can fail independently of Codoxear.
- Push delivery failures depend on vendor endpoints and should be surfaced rather than silently hidden.
- Rust now owns the public voice settings/feed/audio routes, notification subscription persistence/control, listener-heartbeat writes, daemon voice log scanning, local ledger finalization, OpenAI-compatible summary/TTS delivery, VAPID Web Push, and ffmpeg/HLS audio merging. Python voice serving and delivery fallback code has been deleted.
