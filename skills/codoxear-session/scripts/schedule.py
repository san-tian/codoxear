#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from common import add_connection_args, build_opener, login, request_json, resolve_password, load_env_file, env_file_for


def schedule_path(args: argparse.Namespace, suffix: str = "") -> str:
    if not args.share_id:
        return f"/api/schedules{suffix}"
    return f"/share/{args.share_id}/schedules{suffix}"


def parse_template_json(raw: str | None) -> dict[str, Any]:
    if not raw:
        return {}
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise SystemExit(f"--template-json is not valid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise SystemExit("--template-json must be a JSON object")
    return value


def create_payload(args: argparse.Namespace) -> dict[str, Any]:
    if not args.message:
        raise SystemExit("--message is required for create")
    if args.target_mode == "existing_session":
        if not args.session_id:
            raise SystemExit("--session-id is required for existing_session")
        target: dict[str, Any] = {"mode": "existing_session", "session_id": args.session_id}
    else:
        template = parse_template_json(args.template_json)
        if not template:
            if not args.cwd:
                raise SystemExit("--cwd or --template-json is required for new-session schedules")
            template = {
                "cwd": args.cwd,
                "workspace_cwd": args.workspace_cwd or args.cwd,
                "agent_backend": args.backend,
                "create_in_tmux": args.tmux,
            }
        target = {"mode": args.target_mode, "session_template": template}
    if args.kind == "interval":
        rule: dict[str, Any] = {
            "kind": "interval",
            "interval_seconds": max(60, int(args.interval_minutes) * 60),
        }
        next_run_at = args.next_run_at
    else:
        if args.next_run_at is None:
            raise SystemExit("--next-run-at epoch seconds is required unless --kind interval")
        rule = {"kind": args.kind, "next_run_at": args.next_run_at}
        next_run_at = args.next_run_at
    payload: dict[str, Any] = {
        "name": args.name,
        "enabled": not args.disabled,
        "target": target,
        "rule": rule,
        "next_run_at": next_run_at,
        "message_template": args.message,
        "alias_template": args.alias_template,
        "busy_policy": "enqueue",
        "timeout_minutes": None if args.no_timeout else args.timeout_minutes,
    }
    return payload


def main() -> int:
    parser = argparse.ArgumentParser(description="Manage Codoxear scheduled messages.")
    add_connection_args(parser)
    parser.add_argument("--share-id", default=None, help="Use scoped share schedule routes.")
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("list")

    create = sub.add_parser("create")
    create.add_argument("--name", default="Scheduled run")
    create.add_argument("--message", required=True)
    create.add_argument("--alias-template", default=None)
    create.add_argument("--target-mode", choices=["existing_session", "new_session_each_run", "create_once_reuse"], default="existing_session")
    create.add_argument("--session-id", default=None)
    create.add_argument("--cwd", default=None)
    create.add_argument("--workspace-cwd", default=None)
    create.add_argument("--backend", choices=["codex", "pi"], default="codex")
    create.add_argument("--tmux", action=argparse.BooleanOptionalAction, default=True)
    create.add_argument("--template-json", default=None)
    create.add_argument("--kind", choices=["once", "daily", "weekly", "monthly", "interval"], default="once")
    create.add_argument("--next-run-at", type=float, default=None, help="Epoch seconds for the next trigger.")
    create.add_argument("--interval-minutes", type=int, default=60)
    create.add_argument("--timeout-minutes", type=float, default=240)
    create.add_argument("--no-timeout", action="store_true")
    create.add_argument("--disabled", action="store_true")

    for name in ["run-now", "enable", "disable", "delete"]:
        item = sub.add_parser(name)
        item.add_argument("--schedule-id", required=True)

    mark = sub.add_parser("mark-done")
    mark.add_argument("--schedule-id", required=True)
    mark.add_argument("--run-id", required=True)

    args = parser.parse_args()
    opener = build_opener()
    file_env = load_env_file(env_file_for(args))
    login(opener, args.base_url, resolve_password(args, file_env))

    if args.command == "list":
        result = request_json(opener, args.base_url, "GET", schedule_path(args))
    elif args.command == "create":
        result = request_json(opener, args.base_url, "POST", schedule_path(args), create_payload(args))
    elif args.command == "run-now":
        result = request_json(opener, args.base_url, "POST", schedule_path(args, f"/{args.schedule_id}/run_now"))
    elif args.command == "enable":
        result = request_json(opener, args.base_url, "POST", schedule_path(args, f"/{args.schedule_id}/enable"))
    elif args.command == "disable":
        result = request_json(opener, args.base_url, "POST", schedule_path(args, f"/{args.schedule_id}/disable"))
    elif args.command == "delete":
        result = request_json(opener, args.base_url, "DELETE", schedule_path(args, f"/{args.schedule_id}"))
    elif args.command == "mark-done":
        result = request_json(opener, args.base_url, "POST", schedule_path(args, f"/{args.schedule_id}/runs/{args.run_id}/mark_done"))
    else:
        raise SystemExit(f"unsupported command: {args.command}")
    print(json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
