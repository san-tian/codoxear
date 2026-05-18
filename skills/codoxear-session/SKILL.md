---
name: codoxear-session
description: Creates and converses with Codoxear-visible, Codoxear-managed Codex/Pi sessions through the local Codoxear HTTP API. Use when the user asks to create, start, spawn, resume, send messages to, chat with, rename, mark, or manage a session that should appear in Codoxear/Nova, especially web-owned or tmux-backed Codex/Pi sessions.
---

# Codoxear Session

Use the bundled scripts in this skill to operate Codoxear/Nova sessions and scheduled messages through the public HTTP API. The scripts log in with `CODEX_WEB_PASSWORD`, `--password`, or the Codoxear repo `.env`.

## Quick Start

Create a visible Codoxear/Nova session:

```bash
python3 "${CODEX_HOME:-$HOME/.codex}/skills/codoxear-session/scripts/create_session.py" \
  --cwd "$PWD" \
  --workspace-cwd /absolute/sidebar/workspace \
  --alias "short visible name" \
  --message "initial task for the new agent"
```

Continue a conversation with an existing visible session:

```bash
python3 "${CODEX_HOME:-$HOME/.codex}/skills/codoxear-session/scripts/chat.py" \
  --alias "short visible name" \
  "follow-up message"
```

Rename an existing visible session:

```bash
python3 "${CODEX_HOME:-$HOME/.codex}/skills/codoxear-session/scripts/rename.py" \
  --session-id "<id>" \
  --name "new visible name"
```

Mark a session complete/gray or update star state:

```bash
python3 "${CODEX_HOME:-$HOME/.codex}/skills/codoxear-session/scripts/edit_session.py" \
  --alias "short visible name" \
  --complete
```

Create a scheduled message for an existing visible session:

```bash
python3 "${CODEX_HOME:-$HOME/.codex}/skills/codoxear-session/scripts/schedule.py" create \
  --session-id "<id>" \
  --kind interval \
  --interval-minutes 240 \
  --message "Check progress and continue if useful."
```

## Create Workflow

```bash
python3 "${CODEX_HOME:-$HOME/.codex}/skills/codoxear-session/scripts/create_session.py" \
  --backend codex \
  --cwd /absolute/worktree-or-subdir/path \
  --workspace-cwd /absolute/workspace/root \
  --tmux auto \
  --alias "feature worker" \
  --message "Work in this cwd and report progress in Codoxear."
```

Read the JSON output for `session.session_id`, `session.cwd`, and `session.transport`. Use `--backend pi` for Pi sessions; do not include Codex-only auth/service-tier options for Pi. Use `--resume-session-id <id>` for resume and do not combine it with `--worktree-branch`.

## Conversation Workflow

Prefer `--session-id` when automation already has it; use `--alias` or `--cwd` only when they select one intended session.

```bash
python3 "${CODEX_HOME:-$HOME/.codex}/skills/codoxear-session/scripts/chat.py" --session-id <id> "next instruction"
```

Use `--enqueue` when the session may be busy and the message should wait for the idle queue. Set `--wait-seconds 0` for fire-and-forget sends, or raise it for long-running turns.

## Rename Workflow

Target by `--session-id` when possible; `--alias` and `--cwd` are accepted when they identify one intended session.

```bash
python3 "${CODEX_HOME:-$HOME/.codex}/skills/codoxear-session/scripts/rename.py" --session-id <id> --name "worker 1"
```

Clear the visible name with `--name ""`.

## Marker Workflow

Use `edit_session.py --session-id <id> --star` or `--unstar` for the Nova star marker. Use `--shelve` for temporary gray/faded state, `--unshelve` to clear it, and `--complete` to mark the task done by graying the session for a long duration and clearing star unless `--star` is also passed.

## Schedule Workflow

Use `schedule.py list` to inspect schedules visible to the authenticated owner. Use `schedule.py create --target-mode existing_session --session-id <id> --message ...` to schedule against a current session. For new-session schedules, pass `--target-mode new_session_each_run` or `--target-mode create_once_reuse` plus either `--cwd` or `--template-json '{"cwd":"/repo","agent_backend":"codex","create_in_tmux":true}'`.

Use `schedule.py run-now --schedule-id <id>` for an immediate run without changing the next scheduled trigger. Use `enable`, `disable`, `delete`, and `mark-done --run-id <id>` to manage lifecycle. Add `--share-id <share_id>` to manage schedules through a shared-session scope; share routes only allow schedules that target sessions already in that share.

## Options

- Create: `--cwd`, `--workspace-cwd`, `--backend codex|pi`, `--tmux auto|yes|no`, `--alias`, `--message`, and repeatable `--arg`.
- `--model-provider`, `--preferred-auth-method`, `--model`, `--reasoning-effort`, `--service-tier`: launch overrides accepted by Codoxear for the chosen backend.
- `chat.py --session-id|--alias|--cwd`: target an existing visible session.
- `chat.py --enqueue`: queue instead of immediate send.
- `chat.py --wait-seconds`: max time to poll for assistant output.
- `rename.py --session-id|--alias|--cwd --name`: set or clear an existing session's visible Nova name.
- `edit_session.py --star|--unstar|--shelve|--unshelve|--complete`: update Nova sidebar markers.
- `schedule.py list|create|run-now|enable|disable|delete|mark-done`: manage daemon-owned scheduled messages.

## Configuration

- `CODOXEAR_BASE_URL` overrides the default `http://127.0.0.1:8743`.
- `CODOXEAR_REPO_ROOT` points the scripts at the Codoxear checkout for `.env` discovery; otherwise they use the current working directory.
- `CODEX_WEB_PASSWORD` is preferred over command-line passwords so credentials do not appear in shell history.

## Rules

- Prefer this helper and the public API. Codoxear owns sidecar metadata, socket paths, tmux launch, queue/harness state, and aliases.
- Keep the password out of command arguments when possible; prefer `CODEX_WEB_PASSWORD` or the repo `.env`.
- Use absolute `--cwd` paths for orchestration so the spawned session is unambiguous.
- If creation fails, inspect `error`, `field`, and `detail` in the JSON output before retrying with different launch options.
