from __future__ import annotations

import threading
import time
import unittest

from codoxear.codex_app_server import CodexAppServerSession
from codoxear.codex_app_server import _thread_item_chat_event
from codoxear.codex_app_server import _token_usage_to_legacy


class _DummyProc:
    pid = 4321


def _blank_session() -> CodexAppServerSession:
    sess = CodexAppServerSession.__new__(CodexAppServerSession)
    sess._lock = threading.Lock()
    sess._event_cond = threading.Condition(sess._lock)
    sess._event_seq = 0
    sess._last_event = {"kind": "init", "ts": time.time()}
    sess._pending = {}
    sess._next_id = 1
    sess._closed = False
    sess._queue_drain_scheduled = False
    sess._queue = []
    sess._chat_events = []
    sess._raw_tail = []
    sess._stderr_tail = []
    sess._assistant_text_by_item = {}
    sess._pending_server_requests = {}
    sess._busy = False
    sess._last_chat_ts = None
    sess._last_assistant_ts = None
    sess._token = None
    sess._thread_id = "thread-1"
    sess._current_turn_id = ""
    sess._cwd = "/tmp"
    sess._log_path = None
    sess._last_error = None
    sess._proc = _DummyProc()
    return sess


class TestCodexAppServerHelpers(unittest.TestCase):
    def test_thread_item_chat_event_extracts_user_message(self) -> None:
        ev = _thread_item_chat_event(
            {
                "type": "userMessage",
                "content": [
                    {"type": "text", "text": "hello"},
                    {"type": "text", "text": " world"},
                ],
            },
            ts=123.0,
        )
        self.assertEqual(ev, {"role": "user", "text": "hello world", "ts": 123.0})

    def test_thread_item_chat_event_extracts_agent_message(self) -> None:
        ev = _thread_item_chat_event(
            {
                "type": "agentMessage",
                "text": "OK",
            },
            ts=456.0,
        )
        self.assertEqual(ev, {"role": "assistant", "text": "OK", "ts": 456.0})

    def test_token_usage_to_legacy_converts_shape(self) -> None:
        tok = _token_usage_to_legacy(
            {
                "total": {"totalTokens": 250},
                "modelContextWindow": 1000,
            },
            as_of="2026-03-24T00:00:00Z",
        )
        self.assertEqual(tok["context_window"], 1000)
        self.assertEqual(tok["tokens_in_context"], 250)
        self.assertEqual(tok["percent_remaining"], 75)


class TestCodexAppServerSessionNotifications(unittest.TestCase):
    def test_item_completed_appends_chat_event(self) -> None:
        sess = _blank_session()
        sess._handle_notification(
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "type": "agentMessage",
                        "id": "msg-1",
                        "text": "done",
                    }
                },
            }
        )
        payload = sess.get_messages(offset=0, init=False, limit=80, before=0)
        self.assertEqual(len(payload["events"]), 1)
        self.assertEqual(payload["events"][0]["role"], "assistant")
        self.assertEqual(payload["events"][0]["text"], "done")
        self.assertFalse(payload["busy"])

    def test_delta_then_completed_uses_buffered_text(self) -> None:
        sess = _blank_session()
        sess._handle_notification(
            {
                "method": "item/agentMessage/delta",
                "params": {"itemId": "msg-2", "delta": "hel"},
            }
        )
        sess._handle_notification(
            {
                "method": "item/agentMessage/delta",
                "params": {"itemId": "msg-2", "delta": "lo"},
            }
        )
        sess._handle_notification(
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "type": "agentMessage",
                        "id": "msg-2",
                        "text": "",
                    }
                },
            }
        )
        payload = sess.get_messages(offset=0, init=False, limit=80, before=0)
        self.assertEqual(payload["events"][0]["text"], "hello")

    def test_status_and_token_notifications_update_snapshot(self) -> None:
        sess = _blank_session()
        sess._handle_notification(
            {
                "method": "thread/status/changed",
                "params": {"status": {"type": "active", "activeFlags": []}},
            }
        )
        sess._handle_notification(
            {
                "method": "thread/tokenUsage/updated",
                "params": {
                    "tokenUsage": {
                        "total": {"totalTokens": 500},
                        "modelContextWindow": 1000,
                    }
                },
            }
        )
        snap = sess.snapshot()
        self.assertTrue(snap["busy"])
        self.assertEqual(snap["token"]["tokens_in_context"], 500)

    def test_pending_request_list_and_resolved_notification(self) -> None:
        sess = _blank_session()
        sess._handle_server_request(
            {
                "jsonrpc": "2.0",
                "id": 9,
                "method": "item/commandExecution/requestApproval",
                "params": {"threadId": "thread-1", "turnId": "turn-1", "itemId": "item-1", "command": "rm -rf /tmp/x"},
            }
        )
        requests = sess.list_pending_requests()
        self.assertEqual(len(requests), 1)
        self.assertEqual(requests[0]["kind"], "command_approval")
        self.assertEqual(requests[0]["request_id"], 9)
        waited = sess.wait_for_event(after_seq=0, timeout_s=0.1)
        self.assertEqual(waited["event"]["kind"], "request_pending")
        self.assertEqual(waited["event"]["request"]["request_id"], 9)
        sess._handle_notification({"method": "serverRequest/resolved", "params": {"requestId": 9}})
        self.assertEqual(sess.list_pending_requests(), [])

    def test_interrupt_requires_turn_id_and_sends_thread_and_turn(self) -> None:
        sess = _blank_session()
        sess._thread_id = "thread-1"
        sess._current_turn_id = "turn-1"
        called = {}

        def fake_request(method, params, *, timeout_s):
            called["method"] = method
            called["params"] = params
            return {}

        sess._request = fake_request  # type: ignore[method-assign]
        resp = sess.interrupt()
        self.assertEqual(resp, {})
        self.assertEqual(called["method"], "turn/interrupt")
        self.assertEqual(called["params"], {"threadId": "thread-1", "turnId": "turn-1"})

    def test_apply_thread_restores_in_progress_turn_id(self) -> None:
        sess = _blank_session()
        sess._apply_thread(
            {
                "id": "thread-2",
                "cwd": "/tmp/proj",
                "path": "/tmp/rollout.jsonl",
                "status": {"type": "active", "activeFlags": []},
                "turns": [
                    {"id": "turn-old", "status": "completed"},
                    {"id": "turn-live", "status": "inProgress"},
                ],
            }
        )
        self.assertEqual(sess.thread_id, "thread-2")
        self.assertTrue(sess.snapshot()["busy"])
        self.assertEqual(sess.snapshot()["current_turn_id"], "turn-live")

    def test_wait_for_event_returns_after_publish(self) -> None:
        sess = _blank_session()

        def publish():
            time.sleep(0.01)
            sess._publish_event("chat_event", {"role": "assistant"})

        threading.Thread(target=publish, daemon=True).start()
        item = sess.wait_for_event(after_seq=0, timeout_s=1.0)
        self.assertIsNotNone(item)
        self.assertEqual(item["event"]["kind"], "chat_event")

    def test_item_completed_publishes_chat_event_payload(self) -> None:
        sess = _blank_session()
        sess._handle_notification(
            {
                "method": "item/completed",
                "params": {
                    "item": {
                        "type": "agentMessage",
                        "id": "msg-3",
                        "text": "done",
                    }
                },
            }
        )
        item = sess.wait_for_event(after_seq=0, timeout_s=0.1)
        self.assertEqual(item["event"]["kind"], "chat_event")
        self.assertEqual(item["event"]["chat_event"]["text"], "done")


if __name__ == "__main__":
    unittest.main()
