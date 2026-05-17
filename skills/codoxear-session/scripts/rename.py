#!/usr/bin/env python3
"""Rename an existing Codoxear-visible session through Codoxear's public API."""

from __future__ import annotations

import argparse
import json
import sys
import urllib.parse

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


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Set or clear the visible Codoxear/Nova name for a session.")
    parser.add_argument("--name", required=True, help="new visible name; pass an empty string to clear")
    add_connection_args(parser)
    parser.add_argument("--session-id", default=None)
    parser.add_argument("--alias", default=None, help="current visible name to target")
    parser.add_argument("--cwd", default=None)
    parser.add_argument("--backend", choices=["codex", "pi"], default=None)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    file_env = load_env_file(env_file_for(args))
    password = resolve_password(args, file_env)
    opener = build_opener()
    result: dict[str, object] = {"ok": False, "base_url": args.base_url}
    try:
        login(opener, args.base_url, password)
        sessions_payload = request_json(opener, args.base_url, "GET", "/api/sessions")
        session = choose_session(args, sessions_payload)
        session_id = str(session["session_id"])
        rename_response = request_json(
            opener,
            args.base_url,
            "POST",
            f"/api/sessions/{urllib.parse.quote(session_id)}/rename",
            {"name": args.name},
        )
        result.update(
            {
                "ok": True,
                "session": session,
                "rename_response": rename_response,
                "session_id": session_id,
                "alias": rename_response.get("alias", args.name),
            }
        )
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0
    except ApiFailure as exc:
        result.update({"error": str(exc), "status": exc.status, "body": exc.body, "url": exc.url})
        print(json.dumps(result, indent=2, sort_keys=True), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
