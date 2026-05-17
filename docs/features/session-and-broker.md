# Session and Broker

## Purpose

Session and broker code connects browser actions to live Codex/Pi CLI processes. It owns PTY/socket communication, session metadata, tmux/web ownership, send/enqueue/interrupt actions, and resume helpers.

## Key Files

- `backend-rs/src/broker.rs` and `backend-rs/src/bin/codoxear-broker-rs.rs` — native Rust Codex/Pi broker implementation and binary.
- `scripts/codoxear-local` — local daemon wrapper that starts the Rust front door, background workers, and Rust broker binary.

## Call Chain

1. A terminal-owned or web-owned Codex/Pi session creates metadata and a socket under the Codoxear app directory.
2. The Rust public server discovers available sessions and returns them to the UI.
3. Browser actions call `/api/sessions/:id/send`, `/enqueue`, `/interrupt`, `/harness`, or creation/resume endpoints.
4. Server and broker code write to the session socket/PTY or persisted queue depending on current busy/idle state.
5. Rollout/log parsing feeds status and transcript data back to the browser.

## Current Behavior

- Browser-owned sessions can be created from the UI through Rust `POST /api/v1/sessions` behind the stable public `POST /api/sessions` path. When tmux launch is enabled, Rust derives a stable tmux session name from `workspace_cwd` when present and otherwise from the real `cwd`, so sessions in the same Nova workspace share one tmux session while each browser session gets a separate tmux window. API-created sessions may include `workspace_cwd`; the create route passes it to the broker as `CODEX_WEB_WORKSPACE_CWD`, the broker publishes it in sidecar metadata, and Nova uses it only as display/grouping metadata while the agent's real cwd remains `cwd`.
- Web-owned session creation uses the Rust broker; `CODOXEAR_RUST_BROKER_BIN` points at `backend-rs/target/release/codoxear-broker-rs` when overriding discovery. The Rust broker publishes the socket/metadata sidecar shape, owns a PTY child process, propagates the child agent exit code from the broker binary, handles the pinned `state`, `tail`, `send`, `keys`, and `shutdown` socket commands, includes the PTY output tail in socket `state` so the Rust runtime can surface terminal-only prompts such as Codex `/goal` replacement confirmation, writes socket `send` text as bracketed paste and waits briefly before the Enter suffix, has an end-to-end socket-input regression proving `send` and raw `keys` reach a live PTY child, incrementally scans the discovered rollout/Pi log for token updates so socket `state` can report the token payload shape Nova expects, prepends the web-owned Codex headless config flags (`disable_response_storage=false` and `disable_paste_burst=true`), tracks PTY busy hints, handles Codex session-switch prompts, trims PTY output on UTF-8 boundaries, forwards local terminal stdin for terminal-owned launches, sizes the child PTY from the active terminal, and handles `SIGWINCH`. For Pi launches it sets `PI_HOME`, injects an explicit `--session <log.jsonl>` for new sessions, preserves resume ids from `--session`, binds existing Pi logs, and discovers newly opened Pi logs under the broker-configured sessions directory.
- The public interrupt route now forwards the terminal interrupt byte (`Ctrl-C`, `\x03`) through the broker `keys` command instead of a plain ESC byte, so the Nova Stop action uses the same forceful signal users expect from a terminal. The terminal-response routes answer detected terminal prompts with raw keys without going through normal message send, which lets Nova owner and shared-session views confirm or decline Codex `/goal` replacement while the session is otherwise busy.
- Tmux-backed web session creation now waits longer for the spawned broker to publish socket metadata before failing, which avoids false startup failures on slower login-shell or repo-init paths; if that metadata still never appears, the failure now includes the last captured tmux pane output so the underlying broker/bootstrap error is visible instead of only a generic timeout.
- Tmux-backed web session creation strips inherited `TMUX`/`TMUX_PANE` from the server-side spawn environment before probing or launching tmux, so a daemon started from a stale or deleted parent tmux socket still creates a fresh Codoxear tmux session instead of reporting a metadata timeout.
- Tmux-backed web session creation ignores stale parent-process `CODEX_WEB_TMUX_SESSION` values when choosing where to launch; Rust computes the actual tmux session name per workspace/cwd and then passes that computed value to the broker in `CODEX_WEB_TMUX_SESSION` for sidecar metadata.
- Web-created Codex sessions use the model/provider/reasoning defaults returned by `/api/sessions`; deployments can override those defaults in `.env` with `CODEX_WEB_DEFAULT_*` keys before the broker is spawned. The Rust route reads the config/env defaults and launches the Rust broker directly.
- The live Codex/Pi broker compatibility contract is pinned by focused Rust tests: each session publishes a Unix socket at `APP_DIR/socks/<session-id>.sock` plus adjacent JSON metadata containing `session_id`, `broker_pid`, `codex_pid`, `cwd`, `log_path`, `agent_backend`, `resume_session_id`, and, when present, `workspace_cwd`, `transport`, `tmux_session`, `tmux_window`, and `spawn_nonce`. The socket command surface remains `state`, `tail`, `send`, `keys`, and `shutdown`.
- The local daemon has no Python HTTP listener, daemon-level Python `SessionManager`, or Python voice companion; Rust owns the public HTTP/session discovery surface plus queue, harness, voice scanning, voice delivery, and broker launches.
- Web-created sessions launched through an interactive login shell reassert the requested working directory in the final shell command before `exec`, so shell startup files cannot move Codex into a parent/default directory.
- Rollout log discovery first matches by requested cwd, then accepts a unique open main-session log from the Codex process tree when shell startup changed the cwd recorded by Codex.
- Terminal-owned sessions can be discovered and attached without forcing ownership transfer.
- Queue and harness behavior support idle-triggered injection while avoiding direct sends into busy sessions.
- In the local daemon, the delayed queue drain runs in a Rust worker, using the persisted `session_queues.json`, broker `state`, and idle-from-log gate plus the same grace window before injection.
- In the local daemon, the harness sweep runs in a Rust worker, using the persisted `harness.json`, broker `state`/queue guards, per-thread dedupe scope, and assistant-tail cooldown rule before injecting the unattended prompt.
- In the local daemon, the voice scan and delivery workers run in Rust, using socket sidecar discovery and the same rollout/Pi delivery-message classification rules, then consuming atomic `voice_inbox/*.json` handoff files internally for summary/TTS, Web Push, and HLS audio output.
- Dead-session pruning keeps the session cwd with every stale socket entry so cleanup can remove cwd-scoped queue/file/sidebar state without crashing background queue, harness, or voice sweeps.
- Deleting a session sends broker `shutdown`, immediately removes that session's socket/JSON sidecar from `socks/` so refreshes do not rediscover the closing session, and clears aliases, sidebar metadata/dependencies, harness state, queues, current `sid:` file history, and legacy `cwd:` file-history buckets for that session cwd.
- Tmux-backed sessions expose a copyable `tmux attach-session ...` command in the legacy and Nova preview shells.

## Verification

- Targeted Rust broker protocol: `cd backend-rs && cargo test broker -- --test-threads=1`
- Automated Rust session safety: `cd backend-rs && cargo test session_create_route session_action_routes_forward_send_and_interrupt_to_broker -- --test-threads=1`
- Manual: start or select a tmux-backed session in the browser, copy the tmux attach command, and verify it attaches from a terminal.

## Known Risks

- CLI schemas and terminal behavior can vary across Codex/Pi versions.
- Live send/interrupt paths depend on active sockets/PTYs and may require manual verification with real sessions.
- Queue/harness/voice-scan behavior is timing-sensitive around busy/idle transitions and is Rust-owned in daemon mode.
- Rust owns the public create-session route and the only broker implementation. Any remaining live CLI parity gaps must be fixed in `codoxear-broker-rs`.
