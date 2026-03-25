import json
import socket
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

from codoxear.broker import Broker, State as BrokerState, _inject
from codoxear.sessiond import Sessiond, State as SessiondState


def _broker_state() -> BrokerState:
    return BrokerState(
        codex_pid=1,
        pty_master_fd=1,
        cwd="/tmp",
        start_ts=0.0,
        codex_home=Path("/tmp"),
        sessions_dir=Path("/tmp"),
    )


def _sessiond_state() -> SessiondState:
    path = Path("/tmp/sessiond-send-test")
    return SessiondState(
        session_id="sid",
        codex_pid=1,
        log_path=path.with_suffix(".jsonl"),
        sock_path=path.with_suffix(".sock"),
        pty_master_fd=1,
        start_ts=0.0,
    )


def _read_json_line(sock: socket.socket) -> dict:
    buf = b""
    while b"\n" not in buf:
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
    line = buf.split(b"\n", 1)[0]
    if not line:
        raise AssertionError("expected JSON response line")
    return json.loads(line.decode("utf-8"))


class TestSendInjection(unittest.TestCase):
    def test_broker_inject_retries_partial_pty_writes(self) -> None:
        writes: list[bytes] = []

        def fake_write(fd: int, data) -> int:
            raw = bytes(data)
            if not raw:
                return 0
            n = min(3, len(raw))
            writes.append(raw[:n])
            return n

        with patch("codoxear.broker.os.write", side_effect=fake_write), patch("codoxear.broker.time.sleep", return_value=None):
            _inject(1, text="abcdefghij", suffix=b"\r", delay_s=0.0)

        self.assertEqual(b"".join(writes), b"abcdefghij\r\r\r")

    def test_broker_send_acknowledges_before_injection_finishes(self) -> None:
        broker = Broker(cwd="/tmp", codex_args=[])
        broker.state = _broker_state()

        inject_started = threading.Event()
        release_inject = threading.Event()

        def fake_inject(fd: int, *, text: str, suffix: bytes, delay_s: float = 0.2) -> None:
            inject_started.set()
            self.assertTrue(release_inject.wait(1.0))

        server_sock, client_sock = socket.socketpair()
        client_sock.settimeout(0.2)
        thread = threading.Thread(target=broker._handle_conn, args=(server_sock,), daemon=True)
        with patch("codoxear.broker._inject", side_effect=fake_inject):
            thread.start()
            client_sock.sendall(b'{"cmd":"send","text":"hello"}\n')
            self.assertTrue(inject_started.wait(0.2))
            resp = _read_json_line(client_sock)
            self.assertEqual(resp, {"queued": False, "queue_len": 0})
            release_inject.set()
            thread.join(1.0)
        client_sock.close()
        server_sock.close()

    def test_sessiond_send_acknowledges_before_injection_finishes(self) -> None:
        sessiond = Sessiond(cwd="/tmp", codex_args=[])
        sessiond.state = _sessiond_state()

        write_started = threading.Event()
        release_write = threading.Event()

        def fake_write_all(fd: int, data: bytes) -> None:
            write_started.set()
            self.assertTrue(release_write.wait(1.0))

        server_sock, client_sock = socket.socketpair()
        client_sock.settimeout(0.2)
        thread = threading.Thread(target=sessiond._handle_conn, args=(server_sock,), daemon=True)
        with patch("codoxear.sessiond._write_all", side_effect=fake_write_all), patch(
            "codoxear.sessiond.time.sleep", return_value=None
        ):
            thread.start()
            client_sock.sendall(b'{"cmd":"send","text":"hello"}\n')
            self.assertTrue(write_started.wait(0.2))
            resp = _read_json_line(client_sock)
            self.assertEqual(resp, {"queued": False, "queue_len": 0})
            release_write.set()
            thread.join(1.0)
        client_sock.close()
        server_sock.close()


if __name__ == "__main__":
    unittest.main()
