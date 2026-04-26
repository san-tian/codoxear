import threading
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


def _make_prune_manager() -> SessionManager:
    mgr = SessionManager.__new__(SessionManager)
    mgr._lock = threading.Lock()
    mgr._sessions = {}
    mgr._harness = {}
    mgr._aliases = {}
    mgr._files = {}
    return mgr


class TestSessionsPendingLogIdle(unittest.TestCase):
    def test_list_sessions_forces_idle_when_log_is_none(self) -> None:
        mgr = _make_manager()
        s = Session(
            session_id="broker-1",
            thread_id="broker-1",
            broker_pid=1,
            codex_pid=2,
            agent_backend="codex",
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

    def test_list_sessions_uses_log_idle_for_codex_even_when_cached_busy(self) -> None:
        mgr = _make_manager()
        with TemporaryDirectory() as td:
            log_path = Path(td) / "rollout.jsonl"
            log_path.write_text("", encoding="utf-8")
            s = Session(
                session_id="broker-1",
                thread_id="broker-1",
                broker_pid=1,
                codex_pid=2,
                agent_backend="codex",
                owned=False,
                start_ts=123.0,
                cwd="/tmp",
                log_path=log_path,
                sock_path=Path(td) / "broker-1.sock",
                busy=True,
                queue_len=0,
            )
            mgr._sessions[s.session_id] = s

            with patch.object(mgr, "idle_from_log", return_value=True):
                out = mgr.list_sessions()

        self.assertEqual(len(out), 1)
        self.assertIs(out[0].get("busy"), False)

    def test_prune_keeps_live_session_on_transient_stale_socket_error(self) -> None:
        mgr = _make_prune_manager()
        with TemporaryDirectory() as td:
            sock = Path(td) / "broker-1.sock"
            sock.write_text("", encoding="utf-8")
            s = Session(
                session_id="broker-1",
                thread_id="broker-1",
                broker_pid=1,
                codex_pid=2,
                agent_backend="codex",
                owned=False,
                start_ts=123.0,
                cwd="/tmp",
                log_path=None,
                sock_path=sock,
                busy=True,
                queue_len=0,
            )
            mgr._sessions[s.session_id] = s

            with patch.object(mgr, "_refresh_session_state", return_value=(False, ConnectionRefusedError("refused"))), patch(
                "codoxear.server._pid_alive", return_value=True
            ):
                mgr._prune_dead_sessions()

            self.assertIn("broker-1", mgr._sessions)


if __name__ == "__main__":
    unittest.main()
