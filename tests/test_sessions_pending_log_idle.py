import os
import threading
import time
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

from codoxear.server import Session
from codoxear.server import SessionManager


def _make_manager() -> SessionManager:
    mgr = SessionManager.__new__(SessionManager)
    mgr._lock = threading.Lock()
    mgr._sessions = {}
    mgr._harness = {}
    mgr._aliases = {}
    mgr._files = {}
    mgr._discover_existing_if_stale = lambda *args, **kwargs: None  # type: ignore[method-assign]
    mgr._prune_dead_sessions = lambda *args, **kwargs: None  # type: ignore[method-assign]
    mgr._update_meta_counters = lambda *args, **kwargs: None  # type: ignore[method-assign]
    return mgr


class TestSessionsPendingLogIdle(unittest.TestCase):
    def test_list_sessions_forces_idle_when_log_is_none(self) -> None:
        mgr = _make_manager()
        s = Session(
            session_id="broker-1",
            thread_id="broker-1",
            broker_pid=1,
            codex_pid=2,
            cli="codex",
            owned=False,
            start_ts=123.0,
            cwd="/tmp",
            log_path=None,
            sock_path=Path("/tmp/broker-1.sock"),
            busy=True,
            queue_len=0,
        )
        mgr._sessions[s.session_id] = s

        out = mgr.list_sessions()
        self.assertEqual(len(out), 1)
        self.assertIs(out[0].get("busy"), False)

    def test_list_sessions_codex_recent_log_does_not_rearm_busy_when_broker_idle(self) -> None:
        mgr = _make_manager()
        with TemporaryDirectory() as td:
            lp = Path(td) / "rollout-2026-03-20T00-00-00-44444444-4444-4444-4444-444444444444.jsonl"
            lp.write_text('{"type":"event_msg","payload":{"type":"user_message","message":"hello"}}\n', encoding="utf-8")
            now_ts = time.time()
            lp.touch()
            os.utime(lp, (now_ts, now_ts))
            s = Session(
                session_id="broker-4",
                thread_id="broker-4",
                broker_pid=31,
                codex_pid=32,
                cli="codex",
                owned=False,
                start_ts=123.0,
                cwd="/tmp",
                log_path=lp,
                sock_path=Path("/tmp/broker-4.sock"),
                busy=False,
                queue_len=0,
            )
            mgr._sessions[s.session_id] = s
            mgr.idle_from_log = lambda _sid: False  # type: ignore[method-assign]
            with patch("codoxear.server.LOG_BUSY_FROM_LOG_STALE_SECONDS", 45.0):
                out = mgr.list_sessions()
            self.assertEqual(len(out), 1)
            self.assertIs(out[0].get("busy"), False)


if __name__ == "__main__":
    unittest.main()
