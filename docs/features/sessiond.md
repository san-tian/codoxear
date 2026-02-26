# Sessiond

Sessiond is a headless helper that can launch a Codex session without an interactive terminal and expose the same socket metadata as the broker.

## Headless launch
How users use it:
Operators can run it directly in scripts when they need a headless session.

Effect:
Creates a PTY-backed Codex process, writes socket metadata under `~/.local/share/codoxear/socks/`, and manages message injection.

Files:
- `codoxear/sessiond.py`
- `codoxear/pty_util.py`

Key flow:
1. Spawn Codex in a PTY with fixed rows and columns.
2. Write the socket metadata sidecar.
3. Start a socket server for `state`, `send`, and `tail` commands.
4. Track busy state from rollout logs for UI status.

Call stack:
1. `Sessiond.run`
2. `Sessiond._sock_server`
3. `Sessiond._log_watcher`
4. `Sessiond._pty_reader`

Notes:
- Uses the same metadata schema as the broker for server compatibility.
- Inputs are injected immediately; the queue field is currently unused by the send path.
- Busy/idle in sessiond is inferred from rollout events (`user_message` starts a turn, `token_count` clears it).

## Metadata sidecar
How users use it:
Server discovery reads the sidecar JSON next to the socket.

Effect:
Exposes session identity and process metadata.

Files:
- `codoxear/sessiond.py`

Key data:
- Path: `~/.local/share/codoxear/socks/<socket>.json` (sidecar matches the socket filename).
- Socket name: `broker-<pid>.sock`.
- Fields: `session_id`, `owner`, `broker_pid`, `sessiond_pid`, `codex_pid`, `cwd`, `start_ts`, `log_path`, `sock_path`
