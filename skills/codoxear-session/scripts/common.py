from __future__ import annotations

import argparse
import http.cookiejar
import json
import os
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


DEFAULT_BASE_URL = "http://127.0.0.1:8743"


class ApiFailure(RuntimeError):
    def __init__(self, status: int, body: str, url: str) -> None:
        super().__init__(f"HTTP {status} from {url}: {body}")
        self.status = status
        self.body = body
        self.url = url


def default_repo_root() -> str:
    return os.environ.get("CODOXEAR_REPO_ROOT") or os.getcwd()


def add_connection_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--base-url", default=os.environ.get("CODOXEAR_BASE_URL", DEFAULT_BASE_URL))
    parser.add_argument("--repo-root", default=default_repo_root())
    parser.add_argument("--env-file", default=None)
    parser.add_argument("--password", default=None)


def load_env_file(path: Path) -> dict[str, str]:
    values: dict[str, str] = {}
    if not path.exists():
        return values
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
            value = value[1:-1]
        if key:
            values[key] = value
    return values


def env_file_for(args: argparse.Namespace) -> Path:
    if args.env_file:
        return Path(args.env_file).expanduser()
    return Path(args.repo_root).expanduser() / ".env"


def resolve_password(args: argparse.Namespace, file_env: dict[str, str]) -> str:
    password = args.password or os.environ.get("CODEX_WEB_PASSWORD") or file_env.get("CODEX_WEB_PASSWORD")
    if not password:
        raise SystemExit(
            "CODEX_WEB_PASSWORD was not found. Set it in the environment, pass --password, "
            "or point --env-file at the Codoxear .env file."
        )
    return password


def api_url(base_url: str, path: str) -> str:
    return f"{base_url.rstrip('/')}/{path.lstrip('/')}"


def request_json(
    opener: urllib.request.OpenerDirector,
    base_url: str,
    method: str,
    path: str,
    payload: dict[str, Any] | None = None,
    timeout: float = 20.0,
) -> dict[str, Any]:
    data = None
    headers = {"Accept": "application/json"}
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        headers["Content-Type"] = "application/json"
    url = api_url(base_url, path)
    request = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with opener.open(request, timeout=timeout) as response:
            body = response.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        raise ApiFailure(exc.code, body, url) from exc
    if not body.strip():
        return {}
    try:
        parsed = json.loads(body)
    except json.JSONDecodeError as exc:
        raise ApiFailure(200, f"non-JSON response: {body}", url) from exc
    if not isinstance(parsed, dict):
        raise ApiFailure(200, f"expected JSON object: {body}", url)
    return parsed


def login(opener: urllib.request.OpenerDirector, base_url: str, password: str) -> None:
    request_json(opener, base_url, "POST", "/api/login", {"password": password})


def build_opener() -> urllib.request.OpenerDirector:
    cookie_jar = http.cookiejar.CookieJar()
    return urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cookie_jar))


def normalize_cwd(raw: str) -> str:
    return str(Path(raw).expanduser().resolve())


def choose_session(args: argparse.Namespace, sessions_payload: dict[str, Any]) -> dict[str, Any]:
    sessions = [item for item in sessions_payload.get("sessions", []) if isinstance(item, dict)]
    if getattr(args, "session_id", None):
        matches = [item for item in sessions if item.get("session_id") == args.session_id]
    elif getattr(args, "alias", None):
        matches = [item for item in sessions if item.get("alias") == args.alias]
    elif getattr(args, "cwd", None):
        cwd = normalize_cwd(args.cwd)
        matches = [item for item in sessions if normalize_cwd(str(item.get("cwd", ""))) == cwd]
    else:
        matches = sessions
    if getattr(args, "backend", None):
        matches = [item for item in matches if item.get("agent_backend") == args.backend]
    if not matches:
        raise SystemExit("No matching Codoxear session found.")
    matches.sort(key=lambda item: float(item.get("updated_ts") or 0), reverse=True)
    if len(matches) > 1 and not (
        getattr(args, "session_id", None) or getattr(args, "alias", None) or getattr(args, "cwd", None)
    ):
        raise SystemExit("Multiple sessions are visible; pass --session-id, --alias, or --cwd.")
    return matches[0]
