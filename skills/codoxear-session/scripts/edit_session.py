#!/usr/bin/env python3
"""Edit Codoxear/Nova sidebar metadata for a visible session."""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.parse
from typing import Any

from common import (
    ApiFailure,
    add_connection_args,
    build_opener,
    choose_session,
    env_file_for,
    load_env_file,
    login,
    request_json,
    resolve_password,
)


STAR_PRIORITY = 0.85
DEFAULT_SHELVE_SECONDS = 24 * 60 * 60
DEFAULT_COMPLETE_SECONDS = 10 * 365 * 24 * 60 * 60


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Edit Nova marker/sidebar state for a session.")
    add_connection_args(parser)
    parser.add_argument("--session-id", default=None)
    parser.add_argument("--alias", default=None, help="current visible name to target")
    parser.add_argument("--cwd", default=None)
    parser.add_argument("--backend", choices=["codex", "pi"], default=None)
    parser.add_argument("--name", default=None, help="set visible name; omit to preserve current alias")
    star_group = parser.add_mutually_exclusive_group()
    star_group.add_argument("--star", action="store_true", help="mark as important")
    star_group.add_argument("--unstar", action="store_true", help="clear important marker")
    shelf_group = parser.add_mutually_exclusive_group()
    shelf_group.add_argument("--shelve", action="store_true", help="gray/fade temporarily")
    shelf_group.add_argument("--unshelve", action="store_true", help="clear gray/faded state")
    shelf_group.add_argument("--complete", action="store_true", help="gray as completed and clear star unless --star")
    parser.add_argument("--shelve-seconds", type=float, default=DEFAULT_SHELVE_SECONDS)
    parser.add_argument("--complete-seconds", type=float, default=DEFAULT_COMPLETE_SECONDS)
    dep_group = parser.add_mutually_exclusive_group()
    dep_group.add_argument("--dependency-session-id", default=None)
    dep_group.add_argument("--clear-dependency", action="store_true")
    return parser.parse_args()


def edit_payload(args: argparse.Namespace, session: dict[str, Any]) -> dict[str, Any]:
    priority = float(session.get("priority_offset") or 0.0)
    if args.star:
        priority = STAR_PRIORITY
    elif args.unstar or args.complete:
        priority = 0.0

    snooze_until = session.get("snooze_until")
    now = time.time()
    if args.shelve:
        snooze_until = now + max(1.0, args.shelve_seconds)
    elif args.complete:
        snooze_until = now + max(1.0, args.complete_seconds)
    elif args.unshelve:
        snooze_until = None

    dependency = session.get("dependency_session_id")
    if args.clear_dependency:
        dependency = None
    elif args.dependency_session_id is not None:
        dependency = args.dependency_session_id

    return {
        "name": args.name if args.name is not None else str(session.get("alias") or ""),
        "priority_offset": priority,
        "snooze_until": snooze_until,
        "dependency_session_id": dependency,
    }


def main() -> int:
    args = parse_args()
    file_env = load_env_file(env_file_for(args))
    opener = build_opener()
    result: dict[str, Any] = {"ok": False, "base_url": args.base_url}
    try:
        login(opener, args.base_url, resolve_password(args, file_env))
        sessions_payload = request_json(opener, args.base_url, "GET", "/api/sessions")
        session = choose_session(args, sessions_payload)
        session_id = str(session["session_id"])
        payload = edit_payload(args, session)
        edit_response = request_json(
            opener,
            args.base_url,
            "POST",
            f"/api/sessions/{urllib.parse.quote(session_id)}/edit",
            payload,
        )
        result.update({"ok": True, "session_id": session_id, "payload": payload, "edit_response": edit_response})
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0
    except ApiFailure as exc:
        result.update({"error": str(exc), "status": exc.status, "body": exc.body, "url": exc.url})
        print(json.dumps(result, indent=2, sort_keys=True), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
