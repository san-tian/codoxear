from __future__ import annotations

import json
import queue
import subprocess
import threading
import time
from collections import deque
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _thread_status_is_busy(status: dict[str, Any] | None) -> bool:
    if not isinstance(status, dict):
        return False
    return str(status.get("type") or "") == "active"


def _thread_item_text(item: dict[str, Any]) -> str:
    item_type = str(item.get("type") or "")
    if item_type == "agentMessage":
        text = item.get("text")
        return text if isinstance(text, str) else ""
    if item_type != "userMessage":
        return ""
    content = item.get("content")
    if not isinstance(content, list):
        return ""
    out: list[str] = []
    for part in content:
        if not isinstance(part, dict):
            continue
        if str(part.get("type") or "") != "text":
            continue
        text = part.get("text")
        if isinstance(text, str) and text:
            out.append(text)
    return "".join(out)


def _thread_item_chat_event(item: dict[str, Any], *, ts: float) -> dict[str, Any] | None:
    item_type = str(item.get("type") or "")
    text = _thread_item_text(item)
    if not text:
        return None
    if item_type == "userMessage":
        return {"role": "user", "text": text, "ts": float(ts)}
    if item_type == "agentMessage":
        return {"role": "assistant", "text": text, "ts": float(ts)}
    return None


def _seed_chat_events_from_thread(thread: dict[str, Any]) -> tuple[list[dict[str, Any]], float | None, float | None]:
    turns = thread.get("turns")
    if not isinstance(turns, list):
        return [], None, None
    events: list[dict[str, Any]] = []
    last_chat_ts: float | None = None
    last_assistant_ts: float | None = None
    ts = time.time()
    for turn in turns:
        if not isinstance(turn, dict):
            continue
        items = turn.get("items")
        if not isinstance(items, list):
            continue
        for item in items:
            if not isinstance(item, dict):
                continue
            ev = _thread_item_chat_event(item, ts=ts)
            if ev is None:
                continue
            events.append(ev)
            last_chat_ts = ts
            if ev.get("role") == "assistant":
                last_assistant_ts = ts
            ts += 0.000001
    return events, last_chat_ts, last_assistant_ts


def _token_usage_to_legacy(payload: dict[str, Any], *, as_of: str) -> dict[str, Any] | None:
    if not isinstance(payload, dict):
        return None
    total = payload.get("total")
    if not isinstance(total, dict):
        return None
    context_window = payload.get("modelContextWindow")
    if not isinstance(context_window, int) or context_window <= 0:
        return None
    tokens_in_context = total.get("totalTokens")
    if not isinstance(tokens_in_context, int) or tokens_in_context < 0:
        return None
    percent_remaining = int(round(max(0.0, min(100.0, (context_window - tokens_in_context) * 100.0 / context_window))))
    return {
        "context_window": int(context_window),
        "tokens_in_context": int(tokens_in_context),
        "baseline_tokens": 0,
        "percent_remaining": int(percent_remaining),
        "as_of": as_of,
    }


def _request_kind(method: str) -> str:
    return {
        "item/commandExecution/requestApproval": "command_approval",
        "execCommandApproval": "command_approval",
        "item/fileChange/requestApproval": "file_change_approval",
        "applyPatchApproval": "file_change_approval",
        "item/permissions/requestApproval": "permissions_approval",
        "item/tool/requestUserInput": "request_user_input",
        "mcpServer/elicitation/request": "mcp_elicitation",
    }.get(method, "server_request")


def _normalize_pending_request(request: dict[str, Any]) -> dict[str, Any]:
    method = str(request.get("method") or "")
    params = request.get("params")
    if not isinstance(params, dict):
        params = {}
    out = {
        "request_id": int(request.get("id") or 0),
        "method": method,
        "kind": _request_kind(method),
        "received_ts": float(request.get("received_ts") or 0.0),
        "params": params,
    }
    for key in ("threadId", "turnId", "itemId"):
        val = params.get(key)
        if isinstance(val, str) and val:
            out[key[:-2].lower() + "_id" if key.endswith("Id") else key] = val
    return out


class CodexAppServerError(RuntimeError):
    pass


class CodexAppServerRequestError(CodexAppServerError):
    def __init__(self, method: str, error_obj: dict[str, Any]) -> None:
        self.method = method
        self.error_obj = dict(error_obj)
        message = str(error_obj.get("message") or f"{method} failed")
        super().__init__(message)


@dataclass
class _PendingResponse:
    event: threading.Event
    response: dict[str, Any] | None = None


class CodexAppServerSession:
    def __init__(
        self,
        *,
        cwd: str,
        env: dict[str, str],
        codex_bin: str,
        session_source: str = "codoxear",
    ) -> None:
        self._init_runtime(cwd=cwd, env=env, codex_bin=codex_bin, session_source=session_source)
        result = self._request(
            "thread/start",
            {
                "cwd": cwd,
                "experimentalRawEvents": False,
                "persistExtendedHistory": False,
            },
            timeout_s=20.0,
        )
        thread = result.get("thread")
        if not isinstance(thread, dict):
            raise CodexAppServerError("thread/start did not return thread")
        self._apply_thread(thread)
        self._busy = _thread_status_is_busy(thread.get("status") if isinstance(thread.get("status"), dict) else None)

    @classmethod
    def resume(
        cls,
        *,
        thread_id: str,
        cwd: str,
        env: dict[str, str],
        codex_bin: str,
        queue_items: list[str] | None = None,
        session_source: str = "codoxear",
    ) -> "CodexAppServerSession":
        self = cls.__new__(cls)
        self._init_runtime(cwd=cwd, env=env, codex_bin=codex_bin, session_source=session_source)
        result = self._request(
            "thread/resume",
            {
                "threadId": thread_id,
                "persistExtendedHistory": False,
            },
            timeout_s=20.0,
        )
        thread = result.get("thread")
        if not isinstance(thread, dict):
            self.close()
            raise CodexAppServerError("thread/resume did not return thread")
        with self._lock:
            self._apply_thread(thread)
            events, last_chat_ts, last_assistant_ts = _seed_chat_events_from_thread(thread)
            self._chat_events = events
            self._last_chat_ts = last_chat_ts
            self._last_assistant_ts = last_assistant_ts
            self._queue = list(queue_items or [])
        self._schedule_queue_drain()
        return self

    def _init_runtime(
        self,
        *,
        cwd: str,
        env: dict[str, str],
        codex_bin: str,
        session_source: str,
    ) -> None:
        self._lock = threading.Lock()
        self._event_cond = threading.Condition(self._lock)
        self._event_seq = 0
        self._last_event: dict[str, Any] = {"kind": "init", "ts": time.time()}
        self._pending: dict[int, _PendingResponse] = {}
        self._next_id = 1
        self._closed = False
        self._queue_drain_scheduled = False
        self._queue: list[str] = []
        self._chat_events: list[dict[str, Any]] = []
        self._raw_tail: deque[str] = deque(maxlen=200)
        self._stderr_tail: deque[str] = deque(maxlen=100)
        self._assistant_text_by_item: dict[str, str] = {}
        self._pending_server_requests: dict[int, dict[str, Any]] = {}
        self._busy = False
        self._last_chat_ts: float | None = None
        self._last_assistant_ts: float | None = None
        self._token: dict[str, Any] | None = None
        self._thread_id = ""
        self._current_turn_id = ""
        self._cwd = str(cwd)
        self._log_path: Path | None = None
        self._last_error: str | None = None
        self._proc = subprocess.Popen(
            [codex_bin, "app-server", "--listen", "stdio://", "--session-source", session_source],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
            cwd=cwd,
            env=env,
            start_new_session=True,
        )
        if self._proc.stdin is None or self._proc.stdout is None or self._proc.stderr is None:
            raise CodexAppServerError("failed to start codex app-server pipes")
        self._stdout_thread = threading.Thread(target=self._stdout_loop, name=f"codex-app-server-{self._proc.pid}-stdout", daemon=True)
        self._stderr_thread = threading.Thread(target=self._stderr_loop, name=f"codex-app-server-{self._proc.pid}-stderr", daemon=True)
        self._stdout_thread.start()
        self._stderr_thread.start()
        self._request(
            "initialize",
            {
                "clientInfo": {"name": "codoxear", "version": "0.1"},
                "capabilities": {"experimentalApi": False},
            },
            timeout_s=5.0,
        )
        self._notify("initialized", {})

    @property
    def pid(self) -> int:
        return int(self._proc.pid)

    @property
    def thread_id(self) -> str:
        with self._lock:
            return self._thread_id

    @property
    def cwd(self) -> str:
        with self._lock:
            return self._cwd

    @property
    def log_path(self) -> Path | None:
        with self._lock:
            return self._log_path

    def is_alive(self) -> bool:
        return self._proc.poll() is None

    def close(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._closed = True
        try:
            self._proc.terminate()
        except Exception:
            pass
        try:
            self._proc.wait(timeout=3.0)
        except subprocess.TimeoutExpired:
            try:
                self._proc.kill()
            except Exception:
                pass

    def snapshot(self) -> dict[str, Any]:
        with self._lock:
            return {
                "thread_id": self._thread_id,
                "cwd": self._cwd,
                "log_path": self._log_path,
                "busy": bool(self._busy),
                "queue_len": int(len(self._queue)),
                "queue": list(self._queue),
                "token": dict(self._token) if isinstance(self._token, dict) else None,
                "last_chat_ts": self._last_chat_ts,
                "last_assistant_ts": self._last_assistant_ts,
                "last_error": self._last_error,
                "current_turn_id": self._current_turn_id,
                "pending_server_requests": len(self._pending_server_requests),
                "event_seq": int(self._event_seq),
            }

    def get_state(self) -> dict[str, Any]:
        snap = self.snapshot()
        return {
            "busy": bool(snap["busy"]),
            "queue_len": int(snap["queue_len"]),
            "token": snap["token"],
        }

    def send_text(self, text: str) -> dict[str, Any]:
        with self._lock:
            busy = bool(self._busy)
            thread_id = self._thread_id
            turn_id = self._current_turn_id
        if not thread_id:
            raise CodexAppServerError("thread not started")
        input_item = [{"type": "text", "text": text}]
        if busy:
            if not turn_id:
                raise CodexAppServerError("active turn id unavailable for steer")
            self._request(
                "turn/steer",
                {"threadId": thread_id, "expectedTurnId": turn_id, "input": input_item},
                timeout_s=10.0,
            )
        else:
            result = self._request("turn/start", {"threadId": thread_id, "input": input_item}, timeout_s=10.0)
            turn = result.get("turn")
            if isinstance(turn, dict):
                turn_id_new = turn.get("id")
                if isinstance(turn_id_new, str):
                    with self._lock:
                        self._current_turn_id = turn_id_new
        with self._lock:
            self._busy = True
            queue_len = len(self._queue)
        return {"queued": False, "queue_len": int(queue_len)}

    def send_local_image(self, path: str) -> dict[str, Any]:
        with self._lock:
            busy = bool(self._busy)
            thread_id = self._thread_id
            turn_id = self._current_turn_id
        if not thread_id:
            raise CodexAppServerError("thread not started")
        input_item = [{"type": "localImage", "path": str(path)}]
        if busy:
            if not turn_id:
                raise CodexAppServerError("active turn id unavailable for steer")
            self._request(
                "turn/steer",
                {"threadId": thread_id, "expectedTurnId": turn_id, "input": input_item},
                timeout_s=10.0,
            )
        else:
            result = self._request("turn/start", {"threadId": thread_id, "input": input_item}, timeout_s=10.0)
            turn = result.get("turn")
            if isinstance(turn, dict):
                turn_id_new = turn.get("id")
                if isinstance(turn_id_new, str):
                    with self._lock:
                        self._current_turn_id = turn_id_new
        with self._lock:
            self._busy = True
            queue_len = len(self._queue)
        return {"queued": False, "queue_len": int(queue_len)}

    def interrupt(self) -> dict[str, Any]:
        with self._lock:
            thread_id = self._thread_id
            turn_id = self._current_turn_id
        if not thread_id:
            raise CodexAppServerError("thread not started")
        if not turn_id:
            raise CodexAppServerError("active turn id unavailable for interrupt")
        return self._request("turn/interrupt", {"threadId": thread_id, "turnId": turn_id}, timeout_s=10.0)

    def queue_get(self) -> dict[str, Any]:
        with self._lock:
            queue_copy = list(self._queue)
        return {"queue": queue_copy, "queue_len": len(queue_copy)}

    def queue_set(self, queue_items: list[str]) -> dict[str, Any]:
        with self._lock:
            self._queue = list(queue_items)
            queue_copy = list(self._queue)
        self._schedule_queue_drain()
        return {"queue": queue_copy, "queue_len": len(queue_copy)}

    def queue_push(self, text: str, *, front: bool = False) -> dict[str, Any]:
        with self._lock:
            if front:
                self._queue.insert(0, text)
            else:
                self._queue.append(text)
            queue_copy = list(self._queue)
        self._schedule_queue_drain()
        return {"queue": queue_copy, "queue_len": len(queue_copy)}

    def get_tail(self) -> str:
        with self._lock:
            parts = list(self._raw_tail)
            if self._stderr_tail:
                parts.extend(f"stderr: {line}" for line in self._stderr_tail)
        return "\n".join(parts)

    def list_pending_requests(self) -> list[dict[str, Any]]:
        with self._lock:
            items = [_normalize_pending_request(req) for req in self._pending_server_requests.values()]
        items.sort(key=lambda item: (float(item.get("received_ts") or 0.0), int(item.get("request_id") or 0)))
        return items

    def resolve_pending_request(self, request_id: int, result: dict[str, Any]) -> None:
        with self._lock:
            req = self._pending_server_requests.get(int(request_id))
        if req is None:
            raise KeyError("unknown pending request")
        self._send_json({"jsonrpc": "2.0", "id": int(request_id), "result": result})
        with self._lock:
            self._pending_server_requests.pop(int(request_id), None)
        self._publish_event("request_resolved", {"request_id": int(request_id)})

    def wait_for_event(self, after_seq: int, timeout_s: float) -> dict[str, Any] | None:
        deadline = time.time() + max(0.0, float(timeout_s))
        with self._event_cond:
            while True:
                if self._event_seq > int(after_seq):
                    return {"seq": int(self._event_seq), "event": dict(self._last_event)}
                remaining = deadline - time.time()
                if remaining <= 0:
                    return None
                self._event_cond.wait(timeout=remaining)

    def get_messages(
        self,
        *,
        offset: int,
        init: bool,
        limit: int,
        before: int,
    ) -> dict[str, Any]:
        with self._lock:
            total = len(self._chat_events)
            if init and offset == 0:
                end = total if before <= 0 or before > total else before
                start = max(0, end - limit)
                events = [dict(ev) for ev in self._chat_events[start:end]]
                has_older = start > 0
                next_before = start
                new_off = total
            else:
                start = max(0, min(offset, total))
                events = [dict(ev) for ev in self._chat_events[start:]]
                has_older = False
                next_before = 0
                new_off = total
            diag: dict[str, Any] = {
                "transport": "app_server",
                "pending_server_requests": len(self._pending_server_requests),
            }
            if self._last_error:
                diag["last_error"] = self._last_error
            return {
                "thread_id": self._thread_id,
                "log_path": str(self._log_path) if self._log_path is not None else None,
                "offset": int(new_off),
                "events": events,
                "meta_delta": {"thinking": 0, "tool": 0, "system": 0},
                "turn_start": False,
                "turn_end": False,
                "turn_aborted": False,
                "diag": diag,
                "busy": bool(self._busy),
                "queue_len": int(len(self._queue)),
                "token": dict(self._token) if isinstance(self._token, dict) else None,
                "has_older": bool(has_older),
                "next_before": int(next_before),
            }

    def _stdout_loop(self) -> None:
        assert self._proc.stdout is not None
        for raw in self._proc.stdout:
            line = raw.strip()
            if not line:
                continue
            with self._lock:
                self._raw_tail.append(line)
            try:
                msg = json.loads(line)
            except Exception:
                with self._lock:
                    self._last_error = f"invalid json from app-server: {line[:200]}"
                continue
            if isinstance(msg, dict) and "id" in msg and ("result" in msg or "error" in msg):
                self._resolve_pending(msg)
                continue
            if not isinstance(msg, dict):
                continue
            method = msg.get("method")
            if not isinstance(method, str) or not method:
                continue
            if "id" in msg:
                self._handle_server_request(msg)
            else:
                self._handle_notification(msg)
        with self._lock:
            self._closed = True

    def _stderr_loop(self) -> None:
        assert self._proc.stderr is not None
        for raw in self._proc.stderr:
            line = raw.rstrip("\n")
            if not line:
                continue
            with self._lock:
                self._stderr_tail.append(line)
                self._last_error = line

    def _resolve_pending(self, msg: dict[str, Any]) -> None:
        req_id = msg.get("id")
        if not isinstance(req_id, int):
            return
        with self._lock:
            pending = self._pending.get(req_id)
            if not pending:
                return
            pending.response = msg
            pending.event.set()

    def _handle_server_request(self, msg: dict[str, Any]) -> None:
        method = msg.get("method")
        req_id = msg.get("id")
        if not isinstance(method, str) or not method or not isinstance(req_id, int):
            return
        with self._lock:
            self._pending_server_requests[int(req_id)] = {
                "method": method,
                "id": int(req_id),
                "params": msg.get("params"),
                "received_ts": time.time(),
            }
        self._publish_event(
            "request_pending",
            {"request_id": int(req_id), "method": method, "request": _normalize_pending_request(self._pending_server_requests[int(req_id)])},
        )

    def _handle_notification(self, msg: dict[str, Any]) -> None:
        method = msg.get("method")
        params = msg.get("params")
        if not isinstance(method, str):
            return
        ts = time.time()
        if method == "thread/started":
            thread = params.get("thread") if isinstance(params, dict) else None
            if isinstance(thread, dict):
                with self._lock:
                    self._apply_thread(thread)
            self._publish_event("thread_started", {})
            return
        if method == "thread/status/changed":
            status = params.get("status") if isinstance(params, dict) else None
            with self._lock:
                self._busy = _thread_status_is_busy(status if isinstance(status, dict) else None)
                if not self._busy:
                    self._current_turn_id = ""
            self._schedule_queue_drain()
            self._publish_event("thread_status_changed", {"busy": self._busy})
            return
        if method == "turn/started":
            turn = params.get("turn") if isinstance(params, dict) else None
            if isinstance(turn, dict):
                turn_id = turn.get("id")
                if isinstance(turn_id, str):
                    with self._lock:
                        self._current_turn_id = turn_id
                        self._busy = True
            self._publish_event("turn_started", {})
            return
        if method == "turn/completed":
            with self._lock:
                self._busy = False
                self._current_turn_id = ""
            self._schedule_queue_drain()
            self._publish_event("turn_completed", {})
            return
        if method == "thread/tokenUsage/updated":
            token_usage = params.get("tokenUsage") if isinstance(params, dict) else None
            token = _token_usage_to_legacy(token_usage if isinstance(token_usage, dict) else {}, as_of=_now_iso())
            if token is not None:
                with self._lock:
                    self._token = token
            self._publish_event("token_updated", {})
            return
        if method == "item/agentMessage/delta":
            if not isinstance(params, dict):
                return
            item_id = params.get("itemId")
            delta = params.get("delta")
            if not isinstance(item_id, str) or not isinstance(delta, str):
                return
            with self._lock:
                self._assistant_text_by_item[item_id] = self._assistant_text_by_item.get(item_id, "") + delta
            return
        if method == "item/completed":
            item = params.get("item") if isinstance(params, dict) else None
            if not isinstance(item, dict):
                return
            item_type = str(item.get("type") or "")
            if item_type == "agentMessage":
                item_id = item.get("id")
                if isinstance(item_id, str):
                    with self._lock:
                        buffered = self._assistant_text_by_item.pop(item_id, "")
                    text = item.get("text")
                    if (not isinstance(text, str) or not text) and buffered:
                        item = dict(item)
                        item["text"] = buffered
            ev = _thread_item_chat_event(item, ts=ts)
            if ev is None:
                return
            with self._lock:
                self._chat_events.append(ev)
                self._last_chat_ts = ts
                if ev.get("role") == "assistant":
                    self._last_assistant_ts = ts
            self._publish_event("chat_event", {"role": ev.get("role"), "chat_event": dict(ev)})
            return
        if method == "serverRequest/resolved":
            if isinstance(params, dict):
                req_id = params.get("requestId")
                if isinstance(req_id, int):
                    with self._lock:
                        self._pending_server_requests.pop(int(req_id), None)
                    self._publish_event("request_resolved", {"request_id": int(req_id)})
            return

    def _apply_thread(self, thread: dict[str, Any]) -> None:
        thread_id = thread.get("id")
        if isinstance(thread_id, str) and thread_id:
            self._thread_id = thread_id
        cwd = thread.get("cwd")
        if isinstance(cwd, str) and cwd:
            self._cwd = cwd
        path = thread.get("path")
        if isinstance(path, str) and path:
            self._log_path = Path(path)
        status = thread.get("status")
        if isinstance(status, dict):
            self._busy = _thread_status_is_busy(status)
        turns = thread.get("turns")
        if isinstance(turns, list) and turns:
            last_turn = turns[-1]
            if isinstance(last_turn, dict):
                turn_id = last_turn.get("id")
                turn_status = str(last_turn.get("status") or "")
                if isinstance(turn_id, str) and turn_id and turn_status == "inProgress":
                    self._current_turn_id = turn_id

    def _schedule_queue_drain(self) -> None:
        with self._lock:
            if self._queue_drain_scheduled or self._closed:
                return
            if self._busy or not self._queue:
                return
            self._queue_drain_scheduled = True
        threading.Thread(target=self._queue_drain_worker, name=f"codex-app-server-{self._proc.pid}-queue", daemon=True).start()

    def _queue_drain_worker(self) -> None:
        try:
            with self._lock:
                if self._busy or not self._queue:
                    return
                text = self._queue.pop(0)
            self.send_text(text)
        except Exception as e:
            with self._lock:
                self._queue.insert(0, text)
                self._last_error = f"queue drain failed: {type(e).__name__}: {e}"
        finally:
            with self._lock:
                self._queue_drain_scheduled = False
            self._schedule_queue_drain()

    def _request(self, method: str, params: dict[str, Any] | None = None, *, timeout_s: float) -> dict[str, Any]:
        req_id: int
        pending = _PendingResponse(event=threading.Event())
        with self._lock:
            if self._closed:
                raise CodexAppServerError("app-server session closed")
            req_id = self._next_id
            self._next_id += 1
            self._pending[req_id] = pending
        try:
            self._send_json({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}})
            if not pending.event.wait(timeout_s):
                raise CodexAppServerError(f"{method} timed out after {timeout_s:.1f}s")
            response = pending.response or {}
            if "error" in response:
                error_obj = response.get("error")
                if isinstance(error_obj, dict):
                    raise CodexAppServerRequestError(method, error_obj)
                raise CodexAppServerError(f"{method} failed")
            result = response.get("result")
            if isinstance(result, dict):
                return result
            if result is None:
                return {}
            raise CodexAppServerError(f"{method} returned invalid result")
        finally:
            with self._lock:
                self._pending.pop(req_id, None)

    def _notify(self, method: str, params: dict[str, Any] | None = None) -> None:
        self._send_json({"jsonrpc": "2.0", "method": method, "params": params or {}})

    def _send_json(self, payload: dict[str, Any]) -> None:
        if self._proc.stdin is None:
            raise CodexAppServerError("app-server stdin unavailable")
        try:
            self._proc.stdin.write(json.dumps(payload, ensure_ascii=False) + "\n")
            self._proc.stdin.flush()
        except Exception as e:
            raise CodexAppServerError(f"failed to write app-server request: {e}") from e

    def _publish_event(self, kind: str, data: dict[str, Any] | None = None) -> None:
        with self._event_cond:
            self._event_seq += 1
            payload = {"kind": str(kind), "ts": time.time()}
            if isinstance(data, dict) and data:
                payload.update(data)
            self._last_event = payload
            self._event_cond.notify_all()
