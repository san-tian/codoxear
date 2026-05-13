# Replatforming

## Purpose

Move Codoxear toward the `/nova-preview/` shell backed by `frontend/` and `backend-rs/`, while keeping the public browser contract stable during route-by-route migration.

## Key Files

- `frontend/src/app.tsx`
- `frontend/src/share.tsx`
- `frontend/src/lib/transcript.tsx`
- `frontend/src/lib/api.ts`
- `backend-rs/src/routes.rs`
- `backend-rs/src/runtime.rs`

## Current Behavior

- The daemon runs Rust as the production/public HTTP front door; Rust also owns daemon queue draining, harness injection, voice scanning, OpenAI-compatible summary/TTS delivery, VAPID Web Push, ffmpeg/HLS audio merging, and default browser-owned Codex/Pi launches through `codoxear-broker-rs`.
- The legacy Python server and proxy layer have been removed; stable public `/api/*` routes are mounted directly in Rust.
- Public `GET|POST /api/settings/codex_config`, `POST /api/settings/restart_service`, and `GET|POST /api/notifications/subscription` plus `POST /api/notifications/subscription/toggle` now also proxy to Rust.
- Rust currently serves `GET /api/v1/sessions`, `/sessions/:id/diagnostics`, `/sessions/:id/queue`, `/sessions/:id/harness`, `/sessions/:id/git/changed_files`, `/sessions/:id/git/diff`, `/sessions/:id/git/file_versions`, `/sessions/:id/file/read`, `/sessions/:id/file/search`, `/sessions/:id/file/blob`, `/sessions/:id/messages/tail|history|live`, plus `POST /sessions`, `/sessions/:id/rename`, `/edit`, `/delete`, `/send`, `/interrupt`, `/enqueue`, `/harness`, `/inject_file`, `/inject_image`, `/file/write`, `/queue/delete`, `/queue/update`, and `/queue/move`.
- Public `GET /api/sessions` plus `GET /api/sessions/:id/diagnostics`, `/queue`, `/harness`, `/git/changed_files`, `/git/diff`, `/git/file_versions`, `/file/read`, `/file/search`, `/file/blob`, `/messages/tail|history|live`, `/api/settings/codex_config`, and `/api/notifications/subscription` now proxy to Rust. Public `POST /api/sessions`, `/api/sessions/:id/rename`, `/edit`, `/delete`, `/send`, `/interrupt`, `/enqueue`, `/harness`, `/inject_file`, `/inject_image`, `/file/write`, `/queue/delete`, `/queue/update`, `/queue/move`, `/api/settings/codex_config`, `/api/settings/restart_service`, `/api/notifications/subscription`, and `/api/notifications/subscription/toggle` now also proxy to Rust.
- Rust `git/changed_files` merges staged and unstaged `git diff --name-only` output and matching `--numstat` totals.
- Rust `git/diff` requires a repo-contained target path, honors `staged=1`, and returns the raw `git diff -U3 -- <path>` text envelope.
- Rust `git/file_versions` requires a repo-contained target path, returns strict text-only current worktree contents plus size metadata when the file exists, and pairs that with `git show HEAD:<rel>` output when the tracked base version exists.
- Rust `file/search` uses git-first repo search with walk fallback.
- Ralph-style MCP tool loops can publish progress display events; the tmux follow-up idle gate treats assistant `response_item` rows with `phase: final_answer` as turn-ending even when Codex omits `end_turn`.
- Ralph MCP tool results can attach `structuredContent.codoxear_display` progress payloads, and Rust transcript parsing upgrades those structured tool results into transcript `extension` progress events so the existing Nova floating progress card path can render Ralph state without frontend-only special cases.
- Rust `send` and `interrupt` own the live broker path: `send` forwards broker `{"cmd":"send","text":...}` and requires a `queue_len` field in the broker ack, while `interrupt` forwards broker `{"cmd":"keys","seq":"\\x1b"}` and preserves the wrapped `{ok, broker}` response shape.
- The Rust broker socket contract is pinned by tests: the sidecar metadata must publish the current `sock/json` pair plus tmux/spawn fields, and the socket command surface remains `state`, `tail`, `send`, `keys`, and `shutdown`.
- Rust `enqueue` and queue CRUD persist queue items, opportunistically promote the head into an immediate broker `send` only when broker `state` plus rollout-log idle checks say the session is truly idle, and otherwise leave the item queued; `queue/delete`, `queue/update`, and `queue/move` operate by persisted item `id` and reject mutations against the current in-flight `sending` row.
- In the local daemon, Rust runs the idle-drain queue sweep behind `CODOXEAR_ENABLE_QUEUE_SWEEP=1`, so queued sends have one owner.
- In the local daemon, Rust runs the harness sweep behind `CODOXEAR_ENABLE_HARNESS_SWEEP=1`, so unattended prompt injections also have one owner.
- In the local daemon, Rust runs voice scan behind `CODOXEAR_ENABLE_VOICE_SCAN=1` and the delivery worker behind `CODOXEAR_ENABLE_VOICE_WORKER=1`, tails new rollout/Pi log deltas from live sidecar sessions, consumes atomic `voice_inbox/*.json` files internally, calls OpenAI-compatible summary/TTS APIs, sends Web Push, merges audio into HLS, and persists the voice runtime snapshot.
- Rust `edit` validates alias/sidebar payloads, persists alias updates into `session_aliases.json`, persists `priority_offset` / `snooze_until` / `dependency_session_id` into `session_sidebar.json`, and rejects self-dependencies plus missing dependency sessions.
- Rust harness writes persist the normalized `harness.json` shape, reject the legacy `text` field in favor of `request`, keep integer validation for `cooldown_minutes` / `remaining_injections`, and use omitted-field defaults of `5` minutes and `10` injections.
- Rust attachment injection decodes the posted base64 payload, stages the file under the per-session uploads directory, preserves the readable `Attachment N: <path>` injected line, and sends that line through the broker socket as bracketed paste.
- Rust new-session creation reuses launch-default config/env inputs, cwd creation, resume validation, git-worktree creation, tmux metadata wait, and response shape, then launches the native Rust broker path for Codex/Pi sessions.
- The native Rust broker is the only broker path; `CODOXEAR_RUST_BROKER_BIN` selects the binary. The Rust broker owns PTY spawning, child process exit-code propagation, sidecar metadata, the pinned `state` / `tail` / `send` / `keys` / `shutdown` socket protocol, bracketed-paste sends, live token update propagation from rollout/Pi logs, Codex resume argument parsing, web-owned Codex headless config parity (`disable_response_storage=false` and `disable_paste_burst=true`), PTY-output busy hints, Codex session-switch detach detection, terminal-owned stdin forwarding, active-terminal PTY sizing, `SIGWINCH` resize propagation, and Pi launch parity including `PI_HOME`, explicit `--session` log injection, resume-log id extraction, initial Pi log binding, and backend-aware discovery of newly opened Pi session logs.
- Rust `rename` and `delete` now also mirror Python closely enough for public cutover: `rename` reuses Python's alias-cleaning behavior and persists into `session_aliases.json`, while `delete` issues broker `shutdown`, falls back to PID teardown when needed, and clears alias/sidebar/harness/queue/file state for the deleted session.
- Rust `/api/v1/me|login|logout|session_resume_candidates|cwd_suggestions` own helper behavior: `me` reports the current server pid, `login` / `logout` issue and clear the signed auth cookie contract, `session_resume_candidates` uses cwd/log scan plus alias and `last_user_message` enrichment, and `cwd_suggestions` uses recent-cwd plus directory-prefix autocomplete shape.
- Rust mounts route families directly on stable public `/api/*` paths, guarded by a Rust-side auth middleware that validates the signed `codoxear_auth` cookie Rust `login` issues.
- Rust now also serves `GET|POST /api/settings/voice`, `GET /api/notifications/message`, `GET /api/notifications/feed`, `POST /api/audio/listener`, `GET /api/audio/live.m3u8`, and `GET /api/audio/segments/*` directly from the shared voice-push settings, ledger, runtime snapshot, and listener-heartbeat files. Rust also now serves `/legacy/*` directly from `codoxear/static/`, including the legacy HTML placeholder substitution, so no public browser route needs Python anymore.
- The removed Python server no longer keeps shadow route implementations; Rust is the only browser API owner.
- Rust owns the daemon voice scan and delivery loops: it discovers sessions from socket sidecars, tails new rollout/Pi JSONL deltas from current end offsets, generates stable delivery message IDs, skips already-ledgered source messages, directly records disabled/no-worker rows into `voice_delivery_ledger.json`, consumes worker-required `voice_inbox/*.json` files itself, runs OpenAI-compatible summary/TTS requests, sends VAPID Web Push, merges AAC into ffmpeg/HLS audio output, updates subscription failure/success metadata, and persists a compact `voice_runtime.json` audio snapshot.
- The Rust Axum router now also has its own root/nova redirect and static-shell delivery paths for `/`, `/nova`, `/nova-preview/`, `/nova-preview/assets/*`, `/service-worker.js`, `/manifest.webmanifest`, and favicon files. The `/nova-preview/assets/*` handler now keeps the `assets/` prefix when reading from disk, so hashed Vite bundles resolve from `frontend/dist/assets/*` instead of 404ing against the dist root and blanking the page. Together with the daemon cutover, those routes now make Rust the live public HTTP entrypoint instead of preparatory infrastructure only.
- Rust transcript reads normalize visible events on `messages/live`: Codex and Pi tool calls emit `tool` rows, tool outputs emit `tool_result` rows, `ask_user` call/result pairs normalize into prompt rows, `update_plan` plus `codoxear_display` payloads emit `extension` progress events, and live responses fill `meta_delta.tool|thinking|system` plus `diag.tool_names` / `diag.last_tool`.
- Rust `messages/tail|history` keep single-record transcript selection for visible tool/ask-user/progress history rows, including `update_plan`/`codoxear_display` extension events and attachment/tool result rows when they are the primary record-level event.
- Nova preview file interactions stay on Rust-backed read/search/blob plus attachment injection; legacy static file writes now also stay on Rust behind the same stable `/api/sessions/:id/file/write` path.
- Nova preview new-session form preferences are frontend-owned: after a successful create, `frontend/src/app.tsx` persists the selected backend, per-backend provider/model/reasoning/fast-tier values, and tmux choice in browser `localStorage`, then reuses valid stored values on the next dialog open or backend-tab switch without changing the Rust/Python launch-default APIs.
- Nova preview keeps the owner shell and shared-session shell split at the frontend module boundary: `frontend/src/app.tsx` owns session state, routing, and authenticated actions, while `frontend/src/share.tsx` owns the shared-session login/workspace layout and consumes reusable transcript rendering from `frontend/src/lib/transcript.tsx`.

## Verification

- `cd frontend && npm run build`
- `cd backend-rs && cargo test`
- `cd backend-rs && cargo test message_live_normalizes_codex_tool_extension_and_ask_user_events message_tail_and_history_keep_single_event_tool_pages message_live_normalizes_pi_ask_user_events_with_iso_timestamps`
- `cd backend-rs && cargo test session_create_route -- --test-threads=1`
- `cd backend-rs && cargo test broker -- --test-threads=1`
- `cd backend-rs && cargo test root_and_nova_routes_redirect_to_nova_preview nova_preview_index_route_serves_html service_worker_route_serves_javascript`
- `cd backend-rs && cargo test legacy_routes_require_auth_and_serve_static_shell_from_rust file_write_route -- --test-threads=1`
- `cd backend-rs && cargo test -- --test-threads=1`

## Known Risks

- Rust create-session parity now depends on `codoxear-broker-rs` with no Python broker escape hatch.
- The Rust broker path is used for new web-owned Codex and Pi sessions; terminal-owned stdin forwarding exists, but wider live CLI parity review can still reveal Rust-only follow-up fixes.
- Rust owns the full public HTTP entry path, the local daemon's delayed queue draining, harness injection, voice scanning, OpenAI-compatible summary/TTS delivery, Web Push, ffmpeg/HLS audio merging, and web-owned Codex/Pi broker launches. The Python server/package/test suite has been removed.
- The public voice settings, listener heartbeat, notification feed/message, audio playlist/segment routes, daemon voice scanner, delivery ledger finalization, summary/TTS, Web Push, and HLS worker come from Rust-backed shared files/runtime snapshots.
- Rust transcript parsing should now be treated as the production source of truth; any newly discovered parser edge case needs a Rust regression instead of Python parity tests.
- Live browser proof of the Ralph floating card still requires a fresh real Ralph MCP tool call in an active session; the parser and source-path tests are covered, but this turn did not inject a live loop action into the browser transcript.
