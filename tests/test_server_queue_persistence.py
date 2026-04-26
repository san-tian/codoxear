import json
import threading
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

from codoxear.server import Session
from codoxear.server import SessionManager
from codoxear.server import _match_session_route
from codoxear.server import _nova_incremental_v1_path


ROOT = Path(__file__).resolve().parents[1]
SERVER_PY = ROOT / "codoxear" / "server.py"


def _make_session(sid: str) -> Session:
    return Session(
        session_id=sid,
        thread_id="t1",
        broker_pid=1,
        codex_pid=1,
        agent_backend="codex",
        owned=False,
        start_ts=0.0,
        cwd="/tmp",
        log_path=None,
        sock_path=Path(f"/tmp/{sid}.sock"),
    )


def _queue_item(item_id: str, text: str) -> dict[str, object]:
    return {"id": item_id, "text": text, "created_ts": 1.0}


class TestServerQueuePersistence(unittest.TestCase):
    def _mgr(self) -> SessionManager:
        mgr = SessionManager.__new__(SessionManager)
        mgr._lock = threading.Lock()
        mgr._sessions = {}
        mgr._queues = {}
        mgr._save_queues = lambda: None
        return mgr

    def test_match_session_route_requires_exact_suffix(self) -> None:
        self.assertEqual(_match_session_route("/api/sessions/s1/delete", "delete"), "s1")
        self.assertIsNone(_match_session_route("/api/sessions/s1/queue/delete", "delete"))
        self.assertEqual(_match_session_route("/api/sessions/s1/queue/delete", "queue", "delete"), "s1")

    def test_nova_incremental_v1_path_only_rewrites_selected_routes(self) -> None:
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions", "GET"),
            "/api/v1/sessions",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions", "POST"),
            "/api/v1/sessions",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/diagnostics", "GET"),
            "/api/v1/sessions/s1/diagnostics",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/queue", "GET"),
            "/api/v1/sessions/s1/queue",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/harness", "GET"),
            "/api/v1/sessions/s1/harness",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/git/changed_files", "GET"),
            "/api/v1/sessions/s1/git/changed_files",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/git/diff", "GET"),
            "/api/v1/sessions/s1/git/diff",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/git/file_versions", "GET"),
            "/api/v1/sessions/s1/git/file_versions",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/file/read", "GET"),
            "/api/v1/sessions/s1/file/read",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/file/search", "GET"),
            "/api/v1/sessions/s1/file/search",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/file/blob", "GET"),
            "/api/v1/sessions/s1/file/blob",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/messages/tail", "GET"),
            "/api/v1/sessions/s1/messages/tail",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/messages/history", "GET"),
            "/api/v1/sessions/s1/messages/history",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/messages/live", "GET"),
            "/api/v1/sessions/s1/messages/live",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/rename", "POST"),
            "/api/v1/sessions/s1/rename",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/delete", "POST"),
            "/api/v1/sessions/s1/delete",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/edit", "POST"),
            "/api/v1/sessions/s1/edit",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/inject_file", "POST"),
            "/api/v1/sessions/s1/inject_file",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/inject_image", "POST"),
            "/api/v1/sessions/s1/inject_image",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/send", "POST"),
            "/api/v1/sessions/s1/send",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/interrupt", "POST"),
            "/api/v1/sessions/s1/interrupt",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/enqueue", "POST"),
            "/api/v1/sessions/s1/enqueue",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/harness", "POST"),
            "/api/v1/sessions/s1/harness",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/queue/delete", "POST"),
            "/api/v1/sessions/s1/queue/delete",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/queue/update", "POST"),
            "/api/v1/sessions/s1/queue/update",
        )
        self.assertEqual(
            _nova_incremental_v1_path("/api/sessions/s1/queue/move", "POST"),
            "/api/v1/sessions/s1/queue/move",
        )
    def test_do_post_proxies_incremental_routes_before_python_fallbacks(self) -> None:
        source = SERVER_PY.read_text(encoding="utf-8")
        self.assertIn('incremental_v1_path = _nova_incremental_v1_path(path, self.command)', source)
        self.assertIn('incremental_v1_path is not None', source)
        self.assertIn('if self._handle_nova_post(path, u, body):', source)

    def test_python_rename_delete_handlers_are_removed_after_cutover(self) -> None:
        source = SERVER_PY.read_text(encoding="utf-8")
        self.assertNotIn('res = MANAGER.spawn_web_session(', source)
        self.assertNotIn('resume_session_id_raw = obj.get("resume_session_id")', source)
        self.assertEqual(source.count('if path.startswith("/api/sessions/") and path.endswith("/rename"):'), 0)
        self.assertNotIn('session_id = _match_session_route(path, "delete")', source)
        self.assertEqual(source.count('if path.startswith("/api/sessions/") and path.endswith("/edit"):'), 0)
        self.assertEqual(source.count('if path.startswith("/api/sessions/") and (path.endswith("/inject_file") or path.endswith("/inject_image")):'), 0)
        self.assertNotIn('cfg = MANAGER.harness_set(', source)
        self.assertNotIn('session_id = _match_session_route(path, "messages", "tail")', source)
        self.assertNotIn('session_id = _match_session_route(path, "messages", "history")', source)
        self.assertNotIn('session_id = _match_session_route(path, "messages", "live")', source)

    def test_save_queues_persists_transient_sending_flag_for_runtime_readers(self) -> None:
        mgr = self._mgr()
        mgr._queues = {"s1": [{"id": "a", "text": "first", "created_ts": 1.0, "sending": True}]}
        with TemporaryDirectory() as td:
            queue_path = Path(td) / "session_queues.json"
            with patch("codoxear.server.QUEUE_PATH", queue_path), patch("codoxear.server.APP_DIR", Path(td)):
                SessionManager._save_queues(mgr)

            payload = json.loads(queue_path.read_text(encoding="utf-8"))

        self.assertEqual(payload["s1"][0]["id"], "a")
        self.assertTrue(payload["s1"][0]["sending"])

    def test_enqueue_sends_immediately_when_idle(self) -> None:
        mgr = self._mgr()
        sid = "s1"
        mgr._sessions[sid] = _make_session(sid)
        sent: list[tuple[str, str]] = []
        mgr.get_state = lambda _sid: {"busy": False, "queue_len": 0}  # type: ignore[method-assign]
        mgr.send = lambda _sid, text: sent.append((_sid, text)) or {"queued": False, "queue_len": 0}  # type: ignore[method-assign]

        resp = SessionManager.enqueue(mgr, sid, "hello now")

        self.assertFalse(resp.get("queued"))
        self.assertEqual(resp.get("queue_len"), 0)
        self.assertEqual(sent, [(sid, "hello now")])
        self.assertEqual(mgr._queues.get(sid, []), [])

    def test_enqueue_persists_when_busy(self) -> None:
        mgr = self._mgr()
        sid = "s1"
        mgr._sessions[sid] = _make_session(sid)
        mgr.get_state = lambda _sid: {"busy": True, "queue_len": 0}  # type: ignore[method-assign]
        sent: list[tuple[str, str]] = []
        mgr.send = lambda _sid, text: sent.append((_sid, text)) or {"queued": False, "queue_len": 0}  # type: ignore[method-assign]

        resp = SessionManager.enqueue(mgr, sid, "hello queued")

        self.assertTrue(resp.get("queued"))
        self.assertEqual(resp.get("queue_len"), 1)
        self.assertEqual(sent, [])
        items = mgr._queues.get(sid)
        self.assertIsInstance(items, list)
        self.assertEqual(len(items or []), 1)
        self.assertEqual((items or [])[0]["text"], "hello queued")

    def test_queue_update_delete_move_use_ids(self) -> None:
        mgr = self._mgr()
        sid = "s1"
        mgr._sessions[sid] = _make_session(sid)
        mgr._queues[sid] = [
            _queue_item("a", "first"),
            _queue_item("b", "second"),
            _queue_item("c", "third"),
        ]

        update = SessionManager.queue_update(mgr, sid, "b", "second edited")
        self.assertTrue(update["ok"])
        self.assertEqual(mgr._queues[sid][1]["text"], "second edited")

        move = SessionManager.queue_move(mgr, sid, "c", 1)
        self.assertTrue(move["ok"])
        self.assertEqual([item["id"] for item in mgr._queues[sid]], ["a", "c", "b"])

        delete = SessionManager.queue_delete(mgr, sid, "a")
        self.assertTrue(delete["ok"])
        self.assertEqual([item["id"] for item in mgr._queues[sid]], ["c", "b"])

    def test_queue_list_marks_sending_item(self) -> None:
        mgr = self._mgr()
        sid = "s1"
        session = _make_session(sid)
        session.queue_sending_item_id = "b"
        mgr._sessions[sid] = session
        mgr._queues[sid] = [_queue_item("a", "first"), _queue_item("b", "second")]

        items = SessionManager.queue_list(mgr, sid)

        self.assertEqual([item["id"] for item in items], ["a", "b"])
        self.assertFalse(items[0]["sending"])
        self.assertTrue(items[1]["sending"])

    def test_load_queues_migrates_legacy_string_entries(self) -> None:
        with TemporaryDirectory() as td:
            queue_path = Path(td) / "session_queues.json"
            queue_path.write_text(json.dumps({"s1": ["one", "two"]}), encoding="utf-8")
            mgr = self._mgr()
            with patch("codoxear.server.QUEUE_PATH", queue_path):
                SessionManager._load_queues(mgr)

        items = mgr._queues["s1"]
        self.assertEqual([item["text"] for item in items], ["one", "two"])
        self.assertTrue(all(isinstance(item["id"], str) and item["id"] for item in items))


if __name__ == "__main__":
    unittest.main()
