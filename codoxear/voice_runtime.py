from __future__ import annotations

import argparse
import json
import os
import signal
import sys
import threading
import time
import traceback
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from . import rollout_log
from .util import default_app_dir
from .util import proc_find_open_rollout_log
from .util import read_jsonl_from_offset
from .voice_push import ClassifiedAssistantMessage
from .voice_push import VoicePushCoordinator


PROC_ROOT = Path("/proc")
VOICE_PUSH_SWEEP_SECONDS = float(os.environ.get("CODEX_WEB_VOICE_PUSH_SWEEP_SECONDS", "1.0"))


def _app_dir_from_env() -> Path:
    raw = os.environ.get("CODOXEAR_APP_DIR")
    if isinstance(raw, str) and raw.strip():
        return Path(raw).expanduser()
    return default_app_dir()


def _pid_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    except Exception:
        return False
    return True


def _clean_optional_text(raw: Any) -> str | None:
    if not isinstance(raw, str):
        return None
    value = raw.strip()
    return value or None


def _read_json_object(path: Path) -> dict[str, Any] | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return None
    if isinstance(value, dict):
        return value
    return None


def _read_aliases(path: Path) -> dict[str, str]:
    raw = _read_json_object(path)
    if raw is None:
        return {}
    out: dict[str, str] = {}
    for key, value in raw.items():
        if not isinstance(key, str) or not key:
            continue
        if not isinstance(value, str):
            continue
        cleaned = " ".join(value.split()).strip()
        if cleaned:
            out[key] = cleaned
    return out


@dataclass
class VoiceSession:
    session_id: str
    broker_pid: int
    agent_pid: int
    agent_backend: str
    cwd: str
    log_path: Path | None
    delivery_log_off: int
    resume_session_id: str | None


class VoiceRuntime:
    def __init__(
        self,
        *,
        app_dir: Path | None = None,
        stop_event: threading.Event | None = None,
        coordinator: Any | None = None,
    ) -> None:
        self.app_dir = Path(app_dir) if app_dir is not None else _app_dir_from_env()
        self.stop_event = stop_event if stop_event is not None else threading.Event()
        self.sock_dir = self.app_dir / "socks"
        self.inbox_dir = self.app_dir / "voice_inbox"
        self.aliases_path = self.app_dir / "session_aliases.json"
        self.disable_log_scan = os.environ.get("CODEX_WEB_DISABLE_VOICE_SCAN", "").strip() == "1"
        self.sessions: dict[str, VoiceSession] = {}
        self.coordinator = coordinator
        if self.coordinator is None:
            self.coordinator = VoicePushCoordinator(
                app_dir=self.app_dir,
                stop_event=self.stop_event,
                settings_path=self.app_dir / "voice_settings.json",
                subscriptions_path=self.app_dir / "push_subscriptions.json",
                delivery_ledger_path=self.app_dir / "voice_delivery_ledger.json",
                vapid_private_key_path=self.app_dir / "webpush_vapid_private.pem",
            )

    def stop(self) -> None:
        self.stop_event.set()

    def run(self) -> None:
        while not self.stop_event.is_set():
            try:
                self.scan_once()
            except Exception as exc:
                sys.stderr.write(f"error: voice runtime scan failed: {type(exc).__name__}: {exc}\n")
                traceback.print_exc(file=sys.stderr)
                sys.stderr.flush()
            self.stop_event.wait(VOICE_PUSH_SWEEP_SECONDS)

    def scan_once(self) -> None:
        self.consume_inbox()
        if self.disable_log_scan:
            return
        self.discover_sessions()
        aliases = _read_aliases(self.aliases_path)
        for session_id in list(self.sessions.keys()):
            session = self.sessions.get(session_id)
            if session is None:
                continue
            self._scan_session(session, aliases=aliases)

    def discover_sessions(self) -> None:
        self.sock_dir.mkdir(parents=True, exist_ok=True)
        seen: set[str] = set()
        for meta_path in sorted(self.sock_dir.glob("*.json")):
            meta = _read_json_object(meta_path)
            if meta is None:
                continue
            session_id = _clean_optional_text(meta.get("session_id")) or meta_path.stem
            if not session_id:
                continue
            broker_pid = int(meta.get("broker_pid")) if isinstance(meta.get("broker_pid"), int) else 0
            agent_pid = int(meta.get("codex_pid")) if isinstance(meta.get("codex_pid"), int) else 0
            if broker_pid <= 0 and agent_pid <= 0:
                continue
            if (not _pid_alive(broker_pid)) and (not _pid_alive(agent_pid)):
                continue
            cwd = _clean_optional_text(meta.get("cwd")) or ""
            agent_backend = _clean_optional_text(meta.get("agent_backend")) or "codex"
            log_path = self._log_path_from_meta(meta, agent_pid=agent_pid, cwd=cwd, agent_backend=agent_backend)
            resume_session_id = _clean_optional_text(meta.get("resume_session_id"))
            seen.add(session_id)
            previous = self.sessions.get(session_id)
            if previous is None:
                self.sessions[session_id] = VoiceSession(
                    session_id=session_id,
                    broker_pid=broker_pid,
                    agent_pid=agent_pid,
                    agent_backend=agent_backend,
                    cwd=cwd,
                    log_path=log_path,
                    delivery_log_off=self._current_log_size(log_path),
                    resume_session_id=resume_session_id,
                )
                continue
            if previous.log_path != log_path:
                previous.delivery_log_off = self._current_log_size(log_path)
            previous.broker_pid = broker_pid
            previous.agent_pid = agent_pid
            previous.agent_backend = agent_backend
            previous.cwd = cwd
            previous.log_path = log_path
            previous.resume_session_id = resume_session_id
        for stale_id in [session_id for session_id in self.sessions.keys() if session_id not in seen]:
            self.sessions.pop(stale_id, None)

    def consume_inbox(self) -> None:
        if not self.inbox_dir.exists():
            return
        grouped: dict[tuple[str, str], list[tuple[Path, ClassifiedAssistantMessage]]] = {}
        for path in sorted(self.inbox_dir.glob("*.json")):
            payload = _read_json_object(path)
            if payload is None:
                raise ValueError(f"invalid voice inbox payload: {path}")
            session_id = _clean_optional_text(payload.get("session_id"))
            display_name = _clean_optional_text(payload.get("session_display_name")) or "Session"
            message_id = _clean_optional_text(payload.get("message_id"))
            message_class = _clean_optional_text(payload.get("message_class"))
            text = payload.get("text")
            raw_ts = payload.get("ts")
            if not session_id or not message_id or message_class not in ("narration", "final_response"):
                raise ValueError(f"invalid voice inbox payload: {path}")
            if not isinstance(text, str) or not text.strip():
                raise ValueError(f"invalid voice inbox text: {path}")
            if raw_ts is None:
                ts = None
            elif isinstance(raw_ts, (int, float)):
                ts = float(raw_ts)
            else:
                raise ValueError(f"invalid voice inbox timestamp: {path}")
            message = ClassifiedAssistantMessage(
                message_id=message_id,
                message_class=message_class,
                text=text,
                ts=ts,
            )
            grouped.setdefault((session_id, display_name), []).append((path, message))
        for (session_id, display_name), rows in grouped.items():
            self.coordinator.observe_messages(
                session_id=session_id,
                session_display_name=display_name,
                messages=[message for _path, message in rows],
            )
            for path, _message in rows:
                path.unlink(missing_ok=True)

    def _log_path_from_meta(
        self,
        meta: dict[str, Any],
        *,
        agent_pid: int,
        cwd: str,
        agent_backend: str,
    ) -> Path | None:
        raw = _clean_optional_text(meta.get("log_path"))
        if raw:
            path = Path(raw)
            if path.exists():
                return path
        if agent_pid > 0 and _pid_alive(agent_pid):
            return proc_find_open_rollout_log(
                proc_root=PROC_ROOT,
                root_pid=agent_pid,
                agent_backend=agent_backend,
                cwd=cwd or None,
                ignored_paths=set(),
            )
        return None

    def _current_log_size(self, log_path: Path | None) -> int:
        if log_path is None:
            return 0
        try:
            return int(log_path.stat().st_size)
        except FileNotFoundError:
            return 0

    def _session_display_name(self, session: VoiceSession, *, aliases: dict[str, str]) -> str:
        alias = aliases.get(session.session_id)
        if isinstance(alias, str) and alias.strip():
            return alias.strip()
        cwd_name = Path(session.cwd).expanduser().name.strip()
        return cwd_name or "Session"

    def _scan_session(self, session: VoiceSession, *, aliases: dict[str, str]) -> None:
        log_path = session.log_path
        if log_path is None or not log_path.exists():
            return
        try:
            size = int(log_path.stat().st_size)
        except FileNotFoundError:
            return
        if size < session.delivery_log_off:
            session.delivery_log_off = 0
        loops = 0
        while session.delivery_log_off < size and loops < 16:
            objs, new_off = read_jsonl_from_offset(log_path, session.delivery_log_off, max_bytes=256 * 1024)
            if new_off <= session.delivery_log_off:
                break
            if session.resume_session_id:
                session.delivery_log_off = max(int(session.delivery_log_off), int(new_off))
                loops += 1
                continue
            messages = rollout_log._extract_delivery_messages(objs)
            if messages:
                self.coordinator.observe_messages(
                    session_id=session.session_id,
                    session_display_name=self._session_display_name(session, aliases=aliases),
                    messages=messages,
                )
            session.delivery_log_off = max(int(session.delivery_log_off), int(new_off))
            loops += 1


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description="Codoxear voice/push runtime scanner")
    parser.parse_args(argv)

    runtime = VoiceRuntime()

    def _sigterm(_signo: int, _frame: Any) -> None:
        runtime.stop()

    signal.signal(signal.SIGTERM, _sigterm)
    signal.signal(signal.SIGINT, _sigterm)
    runtime.run()


if __name__ == "__main__":
    main()
