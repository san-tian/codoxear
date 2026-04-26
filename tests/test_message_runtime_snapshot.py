import unittest
from pathlib import Path
from unittest.mock import patch

from codoxear import server
from codoxear.server import Session


class TestMessageRuntimeSnapshot(unittest.TestCase):
    def test_stale_state_socket_uses_cached_state_for_message_polling(self) -> None:
        session = Session(
            session_id="broker-1",
            thread_id="broker-1",
            broker_pid=11,
            codex_pid=12,
            agent_backend="codex",
            owned=True,
            start_ts=123.0,
            cwd="/tmp",
            log_path=None,
            sock_path=Path("/tmp/broker-1.sock"),
            busy=True,
            queue_len=0,
            token={"used": 1},
        )

        with patch.object(server.MANAGER, "get_state", side_effect=ConnectionRefusedError("refused")), patch.object(
            server.MANAGER, "_queue_len", return_value=2
        ), patch("codoxear.server._pid_alive", return_value=True):
            state, busy, queue_len, token = server._message_runtime_snapshot("broker-1", session)

        self.assertEqual(state, {"busy": True, "queue_len": 2, "token": {"used": 1}})
        self.assertIs(busy, False)
        self.assertEqual(queue_len, 2)
        self.assertEqual(token, {"used": 1})

    def test_codex_log_idle_overrides_stale_broker_busy_state(self) -> None:
        session = Session(
            session_id="broker-1",
            thread_id="broker-1",
            broker_pid=11,
            codex_pid=12,
            agent_backend="codex",
            owned=True,
            start_ts=123.0,
            cwd="/tmp",
            log_path=Path(__file__),
            sock_path=Path("/tmp/broker-1.sock"),
            busy=True,
            queue_len=0,
        )

        with patch.object(server.MANAGER, "get_state", return_value={"busy": True, "queue_len": 0}), patch.object(
            server.MANAGER, "_queue_len", return_value=0
        ), patch.object(server.MANAGER, "idle_from_log", return_value=True):
            _state, busy, queue_len, _token = server._message_runtime_snapshot("broker-1", session)

        self.assertIs(busy, False)
        self.assertEqual(queue_len, 0)


if __name__ == "__main__":
    unittest.main()
