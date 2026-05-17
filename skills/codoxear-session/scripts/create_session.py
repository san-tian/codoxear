#!/usr/bin/env python3
"""Create a Codoxear-managed web session through Codoxear's public API."""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path
from typing import Any

from common import ApiFailure, add_connection_args, build_opener, env_file_for, load_env_file, login, request_json, resolve_password


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Create a Codoxear-visible Codex/Pi session through the local HTTP API."
    )
    add_connection_args(parser)
    parser.add_argument("--cwd", default=os.getcwd())
    parser.add_argument("--workspace-cwd", default=None)
    parser.add_argument("--backend", choices=["codex", "pi"], default="codex")
    parser.add_argument("--tmux", choices=["auto", "yes", "no"], default="auto")
    parser.add_argument("--alias", default=None)
    parser.add_argument("--message", default=None)
    parser.add_argument("--arg", action="append", default=[])
    parser.add_argument("--resume-session-id", default=None)
    parser.add_argument("--worktree-branch", default=None)
    parser.add_argument("--model-provider", default=None)
    parser.add_argument("--preferred-auth-method", default=None)
    parser.add_argument("--model", default=None)
    parser.add_argument("--reasoning-effort", default=None)
    parser.add_argument("--service-tier", default=None)
    parser.add_argument("--wait-seconds", type=float, default=20.0)
    return parser.parse_args()


def create_payload(args: argparse.Namespace, sessions_payload: dict[str, Any]) -> dict[str, Any]:
    tmux_available = bool(sessions_payload.get("tmux_available"))
    create_in_tmux = tmux_available if args.tmux == "auto" else args.tmux == "yes"
    payload: dict[str, Any] = {
        "cwd": str(Path(args.cwd).expanduser()),
        "agent_backend": args.backend,
        "create_in_tmux": create_in_tmux,
    }
    if args.arg:
        payload["args"] = args.arg
    optional_fields = {
        "workspace_cwd": args.workspace_cwd,
        "resume_session_id": args.resume_session_id,
        "worktree_branch": args.worktree_branch,
        "model_provider": args.model_provider,
        "preferred_auth_method": args.preferred_auth_method,
        "model": args.model,
        "reasoning_effort": args.reasoning_effort,
        "service_tier": args.service_tier,
    }
    for key, value in optional_fields.items():
        if value is not None and value != "":
            payload[key] = value
    return payload


def find_created_session(
    opener,
    base_url: str,
    broker_pid: int | None,
    cwd: str,
    backend: str,
    wait_seconds: float,
) -> tuple[dict[str, Any] | None, dict[str, Any]]:
    deadline = time.monotonic() + max(0.0, wait_seconds)
    last_payload: dict[str, Any] = {}
    cwd_resolved = str(Path(cwd).expanduser().resolve())
    while True:
        last_payload = request_json(opener, base_url, "GET", "/api/sessions")
        sessions = last_payload.get("sessions", [])
        if isinstance(sessions, list):
            if broker_pid is not None:
                for session in sessions:
                    if isinstance(session, dict) and session.get("broker_pid") == broker_pid:
                        return session, last_payload
            fallback = []
            for session in sessions:
                if not isinstance(session, dict) or session.get("agent_backend") != backend:
                    continue
                session_cwd = str(Path(str(session.get("cwd", ""))).expanduser().resolve())
                if session_cwd == cwd_resolved:
                    fallback.append(session)
            if fallback:
                fallback.sort(key=lambda item: float(item.get("updated_ts") or 0), reverse=True)
                return fallback[0], last_payload
        if time.monotonic() >= deadline:
            return None, last_payload
        time.sleep(0.5)


def main() -> int:
    args = parse_args()
    file_env = load_env_file(env_file_for(args))
    password = resolve_password(args, file_env)
    opener = build_opener()
    result: dict[str, Any] = {"ok": False, "base_url": args.base_url}
    try:
        login(opener, args.base_url, password)
        sessions_before = request_json(opener, args.base_url, "GET", "/api/sessions")
        payload = create_payload(args, sessions_before)
        create_response = request_json(opener, args.base_url, "POST", "/api/sessions", payload, timeout=60.0)
        broker_pid_raw = create_response.get("broker_pid")
        broker_pid = broker_pid_raw if isinstance(broker_pid_raw, int) else None
        session, sessions_after = find_created_session(
            opener, args.base_url, broker_pid, str(payload["cwd"]), args.backend, args.wait_seconds
        )

        edit_response = None
        send_response = None
        if session and args.alias:
            edit_response = request_json(
                opener,
                args.base_url,
                "POST",
                f"/api/sessions/{session['session_id']}/edit",
                {"name": args.alias},
            )
            session["alias"] = edit_response.get("alias", args.alias)
        if session and args.message:
            send_response = request_json(
                opener,
                args.base_url,
                "POST",
                f"/api/sessions/{session['session_id']}/send",
                {"text": args.message},
                timeout=30.0,
            )

        result.update(
            {
                "ok": session is not None,
                "create_payload": payload,
                "create_response": create_response,
                "session": session,
                "edit_response": edit_response,
                "send_response": send_response,
            }
        )
        if session is None:
            result["error"] = "created broker did not appear in /api/sessions before timeout"
            result["sessions_seen"] = sessions_after.get("sessions", [])
            print(json.dumps(result, indent=2, sort_keys=True), file=sys.stderr)
            return 2
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0
    except ApiFailure as exc:
        result.update({"error": str(exc), "status": exc.status, "body": exc.body, "url": exc.url})
        print(json.dumps(result, indent=2, sort_keys=True), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
