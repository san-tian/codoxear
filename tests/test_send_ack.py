import json
import socket
import threading
import time
import unittest
import errno
from pathlib import Path
from unittest.mock import patch

from codoxear.broker import Broker, State as BrokerState, _inject as broker_inject
from codoxear.sessiond import Sessiond, State as SessiondState, _inject as sessiond_inject


def _recv_line(sock: socket.socket) -> bytes:
    buf = b""
    while b"\n" not in buf:
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
    return buf.split(b"\n", 1)[0]


def _request_via_socket(handler, payload: dict[str, object]) -> dict[str, object]:
    server_sock, client_sock = socket.socketpair()
    try:
        thread = threading.Thread(target=handler, args=(server_sock,), daemon=True)
        thread.start()
        client_sock.settimeout(1.0)
        client_sock.sendall((json.dumps(payload) + "\n").encode("utf-8"))
        raw = _recv_line(client_sock)
        thread.join(1.0)
        if thread.is_alive():
            raise AssertionError("handler thread did not finish")
        return json.loads(raw.decode("utf-8"))
    finally:
        client_sock.close()


class _FakeReadFile:
    def __init__(self, line: bytes) -> None:
        self._line = line
        self.closed = False

    def readline(self) -> bytes:
        return self._line

    def close(self) -> None:
        self.closed = True


class _BrokenPipeConn:
    def __init__(self, line: bytes) -> None:
        self._line = line
        self.file = _FakeReadFile(line)
        self.sendall_calls = 0
        self.closed = False

    def makefile(self, _mode: str) -> _FakeReadFile:
        return self.file

    def sendall(self, _data: bytes) -> None:
        self.sendall_calls += 1
        raise BrokenPipeError(errno.EPIPE, "Broken pipe")

    def close(self) -> None:
        self.closed = True


class TestSendAck(unittest.TestCase):
    def test_broker_inject_uses_bracketed_paste_and_handles_partial_writes(self) -> None:
        writes: list[bytes] = []

        def fake_write(_fd: int, data: bytes | memoryview) -> int:
            chunk = bytes(data)
            n = min(7, len(chunk))
            writes.append(chunk[:n])
            return n

        with patch("codoxear.broker.os.write", side_effect=fake_write), patch("codoxear.broker.time.sleep"):
            broker_inject(1, text="hello world", suffix=b"\r", delay_s=0.0)

        payload = b"".join(writes)
        self.assertEqual(payload, b"\x1b[200~hello world\x1b[201~\r")

    def test_sessiond_inject_uses_bracketed_paste_and_handles_partial_writes(self) -> None:
        writes: list[bytes] = []

        def fake_write(_fd: int, data: bytes | memoryview) -> int:
            chunk = bytes(data)
            n = min(5, len(chunk))
            writes.append(chunk[:n])
            return n

        with patch("codoxear.sessiond.os.write", side_effect=fake_write), patch("codoxear.sessiond.time.sleep"):
            sessiond_inject(1, text="hello world", suffix=b"\r", delay_s=0.0)

        payload = b"".join(writes)
        self.assertEqual(payload, b"\x1b[200~hello world\x1b[201~\r")

    def test_broker_send_ack_does_not_wait_for_full_inject(self) -> None:
        broker = Broker(cwd="/tmp", codex_args=[])
        broker.state = BrokerState(
            codex_pid=1,
            pty_master_fd=1,
            cwd="/tmp",
            start_ts=0.0,
            codex_home=Path("/tmp"),
            sessions_dir=Path("/tmp"),
        )
        server_sock, client_sock = socket.socketpair()
        try:
            with patch("codoxear.broker._inject", side_effect=lambda *_a, **_k: time.sleep(0.5)):
                thread = threading.Thread(target=broker._handle_conn, args=(server_sock,), daemon=True)
                thread.start()
                client_sock.settimeout(0.2)
                client_sock.sendall((json.dumps({"cmd": "send", "text": "x" * 20000}) + "\n").encode("utf-8"))
                t0 = time.monotonic()
                line = _recv_line(client_sock)
                dt = time.monotonic() - t0
                self.assertLess(dt, 0.2)
                self.assertEqual(json.loads(line.decode("utf-8")), {"queued": False, "queue_len": 0})
                self.assertTrue(thread.is_alive())
                thread.join(1.0)
                self.assertFalse(thread.is_alive())
        finally:
            client_sock.close()

    def test_broker_state_socket_command_reports_token_contract(self) -> None:
        broker = Broker(cwd="/tmp", codex_args=[])
        broker.state = BrokerState(
            codex_pid=1,
            pty_master_fd=1,
            cwd="/tmp",
            start_ts=0.0,
            codex_home=Path("/tmp"),
            sessions_dir=Path("/tmp"),
            busy=True,
            token={"used": 123, "context_window": 456},
        )

        resp = _request_via_socket(broker._handle_conn, {"cmd": "state"})

        self.assertEqual(resp, {"busy": True, "queue_len": 0, "token": {"used": 123, "context_window": 456}})

    def test_broker_tail_socket_command_returns_output_tail(self) -> None:
        broker = Broker(cwd="/tmp", codex_args=[])
        broker.state = BrokerState(
            codex_pid=1,
            pty_master_fd=1,
            cwd="/tmp",
            start_ts=0.0,
            codex_home=Path("/tmp"),
            sessions_dir=Path("/tmp"),
            output_tail="last lines",
        )

        resp = _request_via_socket(broker._handle_conn, {"cmd": "tail"})

        self.assertEqual(resp, {"tail": "last lines"})

    def test_broker_send_sets_busy_turn_state_before_inject(self) -> None:
        broker = Broker(cwd="/tmp", codex_args=[])
        broker.state = BrokerState(
            codex_pid=1,
            pty_master_fd=1,
            cwd="/tmp",
            start_ts=0.0,
            codex_home=Path("/tmp"),
            sessions_dir=Path("/tmp"),
            busy=False,
            last_interrupt_hint_ts=9.0,
            last_turn_activity_ts=1.0,
            pending_calls={"call-1"},
            turn_open=False,
            turn_has_completion_candidate=True,
        )

        with patch("codoxear.broker._now", return_value=42.0), patch(
            "codoxear.broker._inject", side_effect=lambda *_a, **_k: time.sleep(0.05)
        ):
            resp = _request_via_socket(broker._handle_conn, {"cmd": "send", "text": "hello"})

        self.assertEqual(resp, {"queued": False, "queue_len": 0})
        assert broker.state is not None
        self.assertTrue(broker.state.busy)
        self.assertTrue(broker.state.turn_open)
        self.assertFalse(broker.state.turn_has_completion_candidate)
        self.assertEqual(broker.state.pending_calls, set())
        self.assertEqual(broker.state.last_interrupt_hint_ts, 0.0)
        self.assertEqual(broker.state.last_turn_activity_ts, 42.0)

    def test_broker_keys_socket_command_reports_written_byte_count(self) -> None:
        broker = Broker(cwd="/tmp", codex_args=[])
        broker.state = BrokerState(
            codex_pid=1,
            pty_master_fd=77,
            cwd="/tmp",
            start_ts=0.0,
            codex_home=Path("/tmp"),
            sessions_dir=Path("/tmp"),
            key_queue=[b"queued-key"],
        )
        seq = "\x1b[200~hi\x1b[201~"

        with patch("codoxear.broker._write_all") as write_all:
            resp = _request_via_socket(broker._handle_conn, {"cmd": "keys", "seq": seq})

        write_all.assert_called_once_with(77, seq.encode("utf-8"))
        self.assertEqual(resp, {"ok": True, "queued": False, "n": len(seq.encode("utf-8")), "key_queue_len": 1})

    def test_broker_shutdown_socket_command_acks_and_stops_process_group(self) -> None:
        broker = Broker(cwd="/tmp", codex_args=[])

        with patch.object(broker, "_teardown_managed_process_group") as teardown:
            resp = _request_via_socket(broker._handle_conn, {"cmd": "shutdown"})

        self.assertEqual(resp, {"ok": True})
        teardown.assert_called_once_with()

    def test_sessiond_send_ack_does_not_wait_for_full_inject(self) -> None:
        sessiond = Sessiond("/tmp", [])
        sessiond.state = SessiondState(
            session_id="sid",
            codex_pid=1,
            log_path=Path("/tmp/log.jsonl"),
            sock_path=Path("/tmp/test.sock"),
            pty_master_fd=1,
            start_ts=0.0,
        )
        server_sock, client_sock = socket.socketpair()
        try:
            with patch("codoxear.sessiond._inject", side_effect=lambda *_a, **_k: time.sleep(0.5)):
                thread = threading.Thread(target=sessiond._handle_conn, args=(server_sock,), daemon=True)
                thread.start()
                client_sock.settimeout(0.2)
                client_sock.sendall((json.dumps({"cmd": "send", "text": "x" * 20000}) + "\n").encode("utf-8"))
                t0 = time.monotonic()
                line = _recv_line(client_sock)
                dt = time.monotonic() - t0
                self.assertLess(dt, 0.2)
                self.assertEqual(json.loads(line.decode("utf-8")), {"queued": False, "queue_len": 0})
                self.assertTrue(thread.is_alive())
                thread.join(1.0)
                self.assertFalse(thread.is_alive())
        finally:
            client_sock.close()

    def test_broker_ignores_broken_pipe_while_replying(self) -> None:
        broker = Broker(cwd="/tmp", codex_args=[])
        broker.state = BrokerState(
            codex_pid=1,
            pty_master_fd=1,
            cwd="/tmp",
            start_ts=0.0,
            codex_home=Path("/tmp"),
            sessions_dir=Path("/tmp"),
        )
        conn = _BrokenPipeConn((json.dumps({"cmd": "state"}) + "\n").encode("utf-8"))

        with patch("codoxear.broker.traceback.print_exc") as print_exc:
            broker._handle_conn(conn)

        self.assertEqual(conn.sendall_calls, 1)
        self.assertTrue(conn.file.closed)
        self.assertTrue(conn.closed)
        print_exc.assert_not_called()

    def test_sessiond_ignores_broken_pipe_while_replying(self) -> None:
        sessiond = Sessiond("/tmp", [])
        sessiond.state = SessiondState(
            session_id="sid",
            codex_pid=1,
            log_path=Path("/tmp/log.jsonl"),
            sock_path=Path("/tmp/test.sock"),
            pty_master_fd=1,
            start_ts=0.0,
        )
        conn = _BrokenPipeConn((json.dumps({"cmd": "state"}) + "\n").encode("utf-8"))

        with patch("codoxear.sessiond.traceback.print_exc") as print_exc:
            sessiond._handle_conn(conn)

        self.assertEqual(conn.sendall_calls, 1)
        self.assertTrue(conn.file.closed)
        self.assertTrue(conn.closed)
        print_exc.assert_not_called()
