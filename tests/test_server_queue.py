import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch

from codoxear.server import Session
from codoxear.server import SessionManager


def _make_manager() -> SessionManager:
    mgr = SessionManager.__new__(SessionManager)
    mgr._lock = threading.Lock()
    mgr._sessions = {}
    return mgr


def _make_session(*, sid: str, cli: str = "codex", owned: bool = False, start_ts: float = 0.0) -> Session:
    p = Path("/tmp") / f"{sid}.jsonl"
    return Session(
        session_id=sid,
        thread_id="thread-1",
        broker_pid=1,
        codex_pid=1,
        cli=cli,
        owned=owned,
        start_ts=start_ts,
        cwd="/tmp",
        log_path=p,
        sock_path=p.with_suffix(".sock"),
    )


class TestServerQueue(unittest.TestCase):
    def test_queue_get_updates_len(self) -> None:
        mgr = _make_manager()
        sess = _make_session(sid="sid")
        mgr._sessions["sid"] = sess

        seen = {}

        def sock_call(sock, req, timeout_s=0.0):
            seen["req"] = req
            return {"queue": ["a", " ", "b"]}

        mgr._sock_call = sock_call  # type: ignore[assignment]

        resp = mgr.queue_get("sid")
        self.assertEqual(resp["queue"], ["a", "b"])
        self.assertEqual(resp["queue_len"], 2)
        self.assertEqual(sess.queue_len, 2)
        self.assertEqual(seen["req"]["cmd"], "queue")
        self.assertEqual(seen["req"]["op"], "get")

    def test_queue_set_filters_empty(self) -> None:
        mgr = _make_manager()
        sess = _make_session(sid="sid")
        mgr._sessions["sid"] = sess

        seen = {}

        def sock_call(sock, req, timeout_s=0.0):
            seen["req"] = req
            return {"queue": list(req.get("queue") or [])}

        mgr._sock_call = sock_call  # type: ignore[assignment]

        resp = mgr.queue_set("sid", ["one", " ", "", "two"])
        self.assertEqual(resp["queue"], ["one", "two"])
        self.assertEqual(resp["queue_len"], 2)
        self.assertEqual(sess.queue_len, 2)
        self.assertEqual(seen["req"]["op"], "set")
        self.assertEqual(seen["req"]["queue"], ["one", "two"])

    def test_queue_push_passes_front(self) -> None:
        mgr = _make_manager()
        sess = _make_session(sid="sid")
        mgr._sessions["sid"] = sess

        seen = {}

        def sock_call(sock, req, timeout_s=0.0):
            seen["req"] = req
            return {"queue": ["a"]}

        mgr._sock_call = sock_call  # type: ignore[assignment]

        resp = mgr.queue_push("sid", "hello", front=True)
        self.assertEqual(resp["queue"], ["a"])
        self.assertEqual(resp["queue_len"], 1)
        self.assertEqual(seen["req"]["op"], "push")
        self.assertTrue(seen["req"]["front"])

    def test_send_keeps_markdown_image_prefix_literal(self) -> None:
        mgr = _make_manager()
        sess = _make_session(sid="sid")
        mgr._sessions["sid"] = sess

        seen = {}

        def sock_call(sock, req, timeout_s=0.0):
            seen["req"] = req
            return {"queued": False, "queue_len": 0}

        mgr._sock_call = sock_call  # type: ignore[assignment]

        mgr.send("sid", "![plot](figures/a.svg)")
        self.assertEqual(seen["req"]["cmd"], "send")
        self.assertEqual(seen["req"]["text"], "![plot](figures/a.svg)")

    def test_send_keeps_shell_prefix_when_not_markdown_image(self) -> None:
        mgr = _make_manager()
        sess = _make_session(sid="sid")
        mgr._sessions["sid"] = sess

        seen = {}

        def sock_call(sock, req, timeout_s=0.0):
            seen["req"] = req
            return {"queued": False, "queue_len": 0}

        mgr._sock_call = sock_call  # type: ignore[assignment]

        mgr.send("sid", "!ls -la")
        self.assertEqual(seen["req"]["text"], "!ls -la")

    def test_send_retries_transient_startup_failure_for_fresh_web_session(self) -> None:
        mgr = _make_manager()
        sess = _make_session(sid="sid", owned=True, start_ts=time.time())
        mgr._sessions["sid"] = sess

        calls = {"n": 0}

        def sock_call(sock, req, timeout_s=0.0):
            calls["n"] += 1
            if calls["n"] == 1:
                raise TimeoutError("broker not ready")
            return {"queued": False, "queue_len": 0}

        mgr._sock_call = sock_call  # type: ignore[assignment]

        with patch("codoxear.server._pid_alive", return_value=True), patch("codoxear.server.time.sleep", return_value=None):
            resp = mgr.send("sid", "hello")

        self.assertEqual(resp["queue_len"], 0)
        self.assertEqual(calls["n"], 2)

    def test_send_does_not_retry_for_nonfresh_session(self) -> None:
        mgr = _make_manager()
        sess = _make_session(sid="sid", owned=True, start_ts=time.time() - 60.0)
        mgr._sessions["sid"] = sess

        def sock_call(sock, req, timeout_s=0.0):
            raise TimeoutError("broker not ready")

        mgr._sock_call = sock_call  # type: ignore[assignment]

        with patch("codoxear.server._pid_alive", return_value=True), patch("codoxear.server.time.sleep", return_value=None):
            with self.assertRaises(TimeoutError):
                mgr.send("sid", "hello")

    def test_queue_push_keeps_markdown_image_prefix_literal(self) -> None:
        mgr = _make_manager()
        sess = _make_session(sid="sid")
        mgr._sessions["sid"] = sess

        seen = {}

        def sock_call(sock, req, timeout_s=0.0):
            seen["req"] = req
            return {"queue": [str(req.get("text") or "")]}

        mgr._sock_call = sock_call  # type: ignore[assignment]

        resp = mgr.queue_push("sid", "![plot](figures/a.svg)")
        self.assertEqual(seen["req"]["text"], "![plot](figures/a.svg)")
        self.assertEqual(resp["queue"], ["![plot](figures/a.svg)"])

    def test_queue_set_keeps_markdown_image_prefix_literal(self) -> None:
        mgr = _make_manager()
        sess = _make_session(sid="sid")
        mgr._sessions["sid"] = sess

        seen = {}

        def sock_call(sock, req, timeout_s=0.0):
            seen["req"] = req
            return {"queue": list(req.get("queue") or [])}

        mgr._sock_call = sock_call  # type: ignore[assignment]

        resp = mgr.queue_set("sid", ["![plot](figures/a.svg)", "next"])
        self.assertEqual(seen["req"]["queue"], ["![plot](figures/a.svg)", "next"])
        self.assertEqual(resp["queue"], ["![plot](figures/a.svg)", "next"])


if __name__ == "__main__":
    unittest.main()
