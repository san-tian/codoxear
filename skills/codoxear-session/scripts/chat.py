#!/usr/bin/env python3
"""Send a message to an existing Codoxear-managed session and optionally read replies."""

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


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Send a message through Codoxear to a visible Codex/Pi session.")
    parser.add_argument("message_arg", nargs="?", help="message text")
    parser.add_argument("--message", default=None, help="message text, overrides the positional text")
    add_connection_args(parser)
    parser.add_argument("--session-id", default=None)
    parser.add_argument("--alias", default=None)
    parser.add_argument("--cwd", default=None)
    parser.add_argument("--backend", choices=["codex", "pi"], default=None)
    parser.add_argument("--enqueue", action="store_true", help="use /enqueue instead of direct /send")
    parser.add_argument("--wait-seconds", type=float, default=60.0)
    parser.add_argument("--poll-seconds", type=float, default=1.0)
    parser.add_argument("--tail-limit", type=int, default=80)
    return parser.parse_args()


def tail(opener, base_url: str, session_id: str, limit: int) -> dict[str, Any]:
    return request_json(
        opener,
        base_url,
        "GET",
        f"/api/sessions/{urllib.parse.quote(session_id)}/messages/tail?limit={max(1, limit)}",
    )


def live(opener, base_url: str, session_id: str, cursor: str) -> dict[str, Any]:
    cursor_q = urllib.parse.quote(cursor, safe="")
    return request_json(
        opener,
        base_url,
        "GET",
        f"/api/sessions/{urllib.parse.quote(session_id)}/messages/live?cursor={cursor_q}",
    )


def event_text(event: dict[str, Any]) -> str:
    text = event.get("text")
    if isinstance(text, str):
        return text
    if event.get("type") == "ask_user":
        question = event.get("question")
        if isinstance(question, str):
            return question
    return ""


def summarize_events(events: list[Any]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for event in events:
        if not isinstance(event, dict):
            continue
        role = event.get("role") or event.get("type") or event.get("kind")
        text = event_text(event)
        if role and text:
            out.append(
                {
                    "role": role,
                    "text": text,
                    "message_class": event.get("message_class"),
                    "ts": event.get("ts"),
                }
            )
    return out


def wait_for_reply(
    opener,
    base_url: str,
    session_id: str,
    cursor: str | None,
    wait_seconds: float,
    poll_seconds: float,
) -> tuple[list[dict[str, Any]], dict[str, Any] | None]:
    deadline = time.monotonic() + max(0.0, wait_seconds)
    collected: list[dict[str, Any]] = []
    last_payload: dict[str, Any] | None = None
    current_cursor = cursor or "0"
    while time.monotonic() <= deadline:
        payload = live(opener, base_url, session_id, current_cursor)
        last_payload = payload
        events = summarize_events(payload.get("events", []))
        if events:
            collected.extend(events)
        next_cursor = payload.get("live_cursor")
        if isinstance(next_cursor, str) and next_cursor:
            current_cursor = next_cursor
        has_assistant = any(event.get("role") == "assistant" for event in collected)
        has_final = any(
            event.get("role") == "assistant" and event.get("message_class") == "final_response"
            for event in collected
        )
        if has_final or (has_assistant and payload.get("turn_end")):
            break
        time.sleep(max(0.2, poll_seconds))
    return collected, last_payload


def main() -> int:
    args = parse_args()
    message = args.message if args.message is not None else args.message_arg
    if not message or not message.strip():
        raise SystemExit("message required")

    file_env = load_env_file(env_file_for(args))
    password = resolve_password(args, file_env)
    opener = build_opener()
    result: dict[str, Any] = {"ok": False, "base_url": args.base_url}
    try:
        login(opener, args.base_url, password)
        sessions_payload = request_json(opener, args.base_url, "GET", "/api/sessions")
        session = choose_session(args, sessions_payload)
        session_id = str(session["session_id"])
        before_tail = tail(opener, args.base_url, session_id, args.tail_limit)
        cursor = before_tail.get("live_cursor")
        endpoint = "enqueue" if args.enqueue else "send"
        send_response = request_json(
            opener,
            args.base_url,
            "POST",
            f"/api/sessions/{urllib.parse.quote(session_id)}/{endpoint}",
            {"text": message},
            timeout=30.0,
        )
        reply_events: list[dict[str, Any]] = []
        last_live = None
        if args.wait_seconds > 0:
            reply_events, last_live = wait_for_reply(
                opener,
                args.base_url,
                session_id,
                cursor if isinstance(cursor, str) else None,
                args.wait_seconds,
                args.poll_seconds,
            )
        assistant_text = "\n\n".join(event["text"] for event in reply_events if event.get("role") == "assistant")
        result.update(
            {
                "ok": True,
                "session": session,
                "endpoint": endpoint,
                "send_response": send_response,
                "reply_events": reply_events,
                "assistant_text": assistant_text,
                "last_live": last_live,
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
