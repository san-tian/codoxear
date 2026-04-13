from __future__ import annotations

import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

from codoxear.server import Session
from codoxear.server import SessionManager


class _FakeAppSession:
    def __init__(self, *, cwd: str, env: dict[str, str], codex_bin: str) -> None:
        self.pid = 6543
        self._alive = True
        self.sent: list[str] = []
        self.queue: list[str] = []
        self.state = {
            "thread_id": "thread-app-1",
            "cwd": cwd,
            "log_path": Path("/tmp/fake-rollout.jsonl"),
            "busy": False,
            "queue_len": 0,
            "token": {"context_window": 1000, "tokens_in_context": 100, "percent_remaining": 90, "baseline_tokens": 0},
            "last_chat_ts": 10.0,
            "last_assistant_ts": 11.0,
            "last_error": None,
            "current_turn_id": "",
            "pending_server_requests": 1,
        }

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
    ) -> "_FakeAppSession":
        sess = cls(cwd=cwd, env=env, codex_bin=codex_bin)
        sess.state["thread_id"] = thread_id
        sess.queue = list(queue_items or [])
        return sess

    def snapshot(self) -> dict[str, object]:
        out = dict(self.state)
        out["queue_len"] = len(self.queue)
        out["queue"] = list(self.queue)
        return out

    def is_alive(self) -> bool:
        return self._alive

    def close(self) -> None:
        self._alive = False

    def send_text(self, text: str) -> dict[str, object]:
        self.sent.append(text)
        self.state["busy"] = True
        return {"queued": False, "queue_len": len(self.queue)}

    def get_state(self) -> dict[str, object]:
        return {"busy": bool(self.state["busy"]), "queue_len": len(self.queue), "token": self.state["token"]}

    def queue_get(self) -> dict[str, object]:
        return {"queue": list(self.queue), "queue_len": len(self.queue)}

    def queue_set(self, queue_items: list[str]) -> dict[str, object]:
        self.queue = list(queue_items)
        return {"queue": list(self.queue), "queue_len": len(self.queue)}

    def queue_push(self, text: str, *, front: bool = False) -> dict[str, object]:
        if front:
            self.queue.insert(0, text)
        else:
            self.queue.append(text)
        return {"queue": list(self.queue), "queue_len": len(self.queue)}

    def get_messages(self, *, offset: int, init: bool, limit: int, before: int) -> dict[str, object]:
        return {
            "thread_id": "thread-app-1",
            "log_path": "/tmp/fake-rollout.jsonl",
            "offset": 1,
            "events": [{"role": "assistant", "text": "hello", "ts": 12.0}],
            "meta_delta": {"thinking": 0, "tool": 0, "system": 0},
            "turn_start": False,
            "turn_end": False,
            "turn_aborted": False,
            "diag": {"transport": "app_server"},
            "busy": bool(self.state["busy"]),
            "queue_len": len(self.queue),
            "token": self.state["token"],
            "has_older": False,
            "next_before": 0,
        }

    def list_pending_requests(self) -> list[dict[str, object]]:
        return [
            {
                "request_id": 77,
                "method": "item/commandExecution/requestApproval",
                "kind": "command_approval",
                "received_ts": 1.0,
                "params": {"command": "echo hi"},
            }
        ]

    def resolve_pending_request(self, request_id: int, result: dict[str, object]) -> None:
        self.state["last_resolved"] = {"request_id": request_id, "result": result}
        self.state["pending_server_requests"] = 0

    def interrupt(self) -> dict[str, object]:
        self.state["busy"] = False
        return {"ok": True}

    def get_tail(self) -> str:
        return "tail"

    def send_local_image(self, path: str) -> dict[str, object]:
        self.sent.append(f"image:{path}")
        return {"queued": False, "queue_len": len(self.queue)}


class TestServerAppServer(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self._dotenv = Path(self._tmp.name) / ".env"
        self._app_session_dir = Path(self._tmp.name) / "app_sessions"
        self._dotenv_patcher = patch("codoxear.server._DOTENV", self._dotenv)
        self._app_session_dir_patcher = patch("codoxear.server.APP_SESSION_DIR", self._app_session_dir)
        self._dotenv_patcher.start()
        self._app_session_dir_patcher.start()

    def tearDown(self) -> None:
        self._app_session_dir_patcher.stop()
        self._dotenv_patcher.stop()
        self._tmp.cleanup()

    def _mgr(self) -> SessionManager:
        mgr = SessionManager.__new__(SessionManager)
        mgr._lock = threading.Lock()
        mgr._sessions = {}
        mgr._app_sessions = {}
        mgr._stop = threading.Event()
        mgr._last_discover_ts = 0.0
        mgr._harness = {}
        mgr._aliases = {}
        mgr._files = {}
        mgr._harness_last_injected = {}
        mgr._harness_last_injected_scope = {}
        return mgr

    def test_spawn_web_session_registers_app_server_session(self) -> None:
        mgr = self._mgr()
        with patch("codoxear.server.CodexAppServerSession", _FakeAppSession):
            res = mgr.spawn_web_session(cwd="/tmp", cli="codex", transport="app_server")

        self.assertEqual(res["transport"], "app_server")
        self.assertEqual(res["session_id"], "thread-app-1")
        session = mgr.get_session("thread-app-1")
        self.assertIsNotNone(session)
        self.assertEqual(session.transport, "app_server")
        self.assertEqual(session.log_path, Path("/tmp/fake-rollout.jsonl"))
        sessions = mgr.list_sessions()
        self.assertEqual(sessions[0]["pending_request_count"], 1)

    def test_send_queue_state_and_tail_route_to_app_server(self) -> None:
        mgr = self._mgr()
        fake = _FakeAppSession(cwd="/tmp", env={}, codex_bin="codex")
        session = Session(
            session_id="thread-app-1",
            thread_id="thread-app-1",
            broker_pid=fake.pid,
            codex_pid=fake.pid,
            cli="codex",
            owned=True,
            start_ts=1.0,
            cwd="/tmp",
            log_path=Path("/tmp/fake-rollout.jsonl"),
            sock_path=Path("/tmp/app-server-thread-app-1.jsonrpc"),
            transport="app_server",
        )
        mgr._sessions[session.session_id] = session
        mgr._app_sessions[session.session_id] = fake

        send_resp = mgr.send("thread-app-1", "hello")
        queue_resp = mgr.queue_push("thread-app-1", "later")
        state_resp = mgr.get_state("thread-app-1")
        tail = mgr.get_tail("thread-app-1")
        msgs = mgr.get_app_server_messages("thread-app-1", offset=0, init=False, limit=80, before=0)
        requests = mgr.get_pending_requests("thread-app-1")
        resolve_resp = mgr.resolve_pending_request("thread-app-1", 77, {"decision": "accept"})
        interrupt_resp = mgr.inject_keys("thread-app-1", "\\x1b")

        self.assertEqual(send_resp["queue_len"], 0)
        self.assertEqual(fake.sent, ["hello"])
        self.assertEqual(queue_resp["queue_len"], 1)
        self.assertEqual(state_resp["queue_len"], 1)
        self.assertEqual(tail, "tail")
        self.assertEqual(msgs["thread_id"], "thread-app-1")
        self.assertEqual(requests["count"], 1)
        self.assertEqual(resolve_resp, {"ok": True})
        self.assertEqual(fake.state["last_resolved"]["request_id"], 77)
        self.assertEqual(interrupt_resp, {"ok": True})

    def test_discover_existing_resumes_persisted_app_server_session(self) -> None:
        mgr = self._mgr()
        self._app_session_dir.mkdir(parents=True, exist_ok=True)
        meta_path = self._app_session_dir / "thread-app-9.json"
        meta_path.write_text(
            (
                '{\n'
                '  "session_id": "thread-app-9",\n'
                '  "thread_id": "thread-app-9",\n'
                '  "owner": "web",\n'
                '  "cli": "codex",\n'
                '  "transport": "app_server",\n'
                '  "cwd": "/tmp",\n'
                '  "start_ts": 123.0,\n'
                '  "log_path": "/tmp/fake-rollout.jsonl",\n'
                '  "queue": ["next one"]\n'
                '}\n'
            ),
            encoding="utf-8",
        )
        with patch("codoxear.server.CodexAppServerSession", _FakeAppSession):
            mgr._discover_existing(force=True)

        session = mgr.get_session("thread-app-9")
        self.assertIsNotNone(session)
        self.assertEqual(session.transport, "app_server")
        self.assertEqual(session.queue_len, 1)
        self.assertIn("thread-app-9", mgr._app_sessions)


if __name__ == "__main__":
    unittest.main()
