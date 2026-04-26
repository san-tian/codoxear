import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from codoxear.pi_log import pi_token_update
from codoxear.server import _compute_idle_from_log
from codoxear.server import _extract_chat_events


class TestServerChatFlags(unittest.TestCase):
    def test_token_count_does_not_set_turn_end(self) -> None:
        _events, _meta, flags, _diag = _extract_chat_events(
            [
                {"type": "event_msg", "payload": {"type": "user_message", "message": "hello"}},
                {"type": "event_msg", "payload": {"type": "token_count", "info": {}}},
            ]
        )
        self.assertTrue(flags["turn_start"])
        self.assertFalse(flags["turn_end"])
        self.assertFalse(flags["turn_aborted"])

    def test_task_complete_sets_turn_end(self) -> None:
        _events, _meta, flags, _diag = _extract_chat_events(
            [
                {"type": "event_msg", "payload": {"type": "user_message", "message": "hello"}},
                {"type": "event_msg", "payload": {"type": "task_complete"}},
            ]
        )
        self.assertTrue(flags["turn_start"])
        self.assertTrue(flags["turn_end"])
        self.assertFalse(flags["turn_aborted"])

    def test_turn_aborted_sets_abort_flag(self) -> None:
        _events, _meta, flags, _diag = _extract_chat_events(
            [
                {"type": "event_msg", "payload": {"type": "user_message", "message": "hello"}},
                {"type": "event_msg", "payload": {"type": "turn_aborted"}},
            ]
        )
        self.assertTrue(flags["turn_start"])
        self.assertFalse(flags["turn_end"])
        self.assertTrue(flags["turn_aborted"])

    def test_task_complete_sets_turn_end_flag(self) -> None:
        _events, _meta, flags, _diag = _extract_chat_events(
            [
                {"type": "event_msg", "payload": {"type": "user_message", "message": "hello"}},
                {"type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t1"}},
            ]
        )
        self.assertTrue(flags["turn_start"])
        self.assertTrue(flags["turn_end"])
        self.assertFalse(flags["turn_aborted"])

    def test_turn_complete_sets_turn_end_flag(self) -> None:
        _events, _meta, flags, _diag = _extract_chat_events(
            [
                {"type": "event_msg", "payload": {"type": "user_message", "message": "hello"}},
                {"type": "event_msg", "payload": {"type": "turn_complete", "turn_id": "t1"}},
            ]
        )
        self.assertTrue(flags["turn_start"])
        self.assertTrue(flags["turn_end"])
        self.assertFalse(flags["turn_aborted"])

    def test_local_shell_and_web_search_increment_tool_count(self) -> None:
        _events, meta, _flags, _diag = _extract_chat_events(
            [
                {"type": "response_item", "payload": {"type": "local_shell_call", "status": "completed"}},
                {"type": "response_item", "payload": {"type": "web_search_call", "status": "completed"}},
            ]
        )
        self.assertEqual(meta["tool"], 2)

    def test_assistant_message_carries_message_class(self) -> None:
        events, _meta, _flags, _diag = _extract_chat_events(
            [
                {
                    "type": "response_item",
                    "payload": {
                        "type": "message",
                        "role": "assistant",
                        "phase": "final_answer",
                        "content": [{"type": "output_text", "text": "done"}],
                    },
                }
            ]
        )
        self.assertEqual(events[0]["message_class"], "final_response")
        self.assertIsInstance(events[0]["message_id"], str)

    def test_function_call_emits_visible_tool_event(self) -> None:
        events, meta, _flags, diag = _extract_chat_events(
            [
                {
                    "type": "response_item",
                    "payload": {
                        "type": "function_call",
                        "name": "bash",
                        "call_id": "tool-1",
                        "arguments": {"command": "pwd"},
                    },
                    "ts": 10.0,
                }
            ]
        )
        self.assertEqual(meta["tool"], 1)
        self.assertEqual(diag["last_tool"], "bash")
        self.assertEqual(events[0]["type"], "tool")
        self.assertEqual(events[0]["name"], "bash")
        self.assertEqual(events[0]["tool_call_id"], "tool-1")
        self.assertEqual(events[0]["text"], "pwd")

    def test_exec_command_function_call_uses_cmd_argument_as_visible_text(self) -> None:
        events, meta, _flags, diag = _extract_chat_events(
            [
                {
                    "type": "response_item",
                    "payload": {
                        "type": "function_call",
                        "name": "exec_command",
                        "call_id": "tool-2",
                        "arguments": {"cmd": "rtk git status --short"},
                    },
                    "ts": 10.0,
                }
            ]
        )
        self.assertEqual(meta["tool"], 1)
        self.assertEqual(diag["last_tool"], "exec_command")
        self.assertEqual(events[0]["type"], "tool")
        self.assertEqual(events[0]["name"], "exec_command")
        self.assertEqual(events[0]["tool_call_id"], "tool-2")
        self.assertEqual(events[0]["text"], "rtk git status --short")

    def test_update_plan_function_call_emits_extension_progress(self) -> None:
        events, meta, _flags, diag = _extract_chat_events(
            [
                {
                    "type": "response_item",
                    "payload": {
                        "type": "function_call",
                        "name": "update_plan",
                        "call_id": "plan-1",
                        "arguments": {
                            "plan": [
                                {"step": "Inspect logs", "status": "completed"},
                                {"step": "Render progress", "status": "in_progress"},
                            ]
                        },
                    },
                    "ts": 10.0,
                }
            ]
        )
        self.assertEqual(meta["tool"], 1)
        self.assertEqual(diag["last_tool"], "update_plan")
        self.assertEqual(events[0]["type"], "extension")
        self.assertEqual(events[0]["extension_kind"], "progress")
        self.assertEqual(events[0]["title"], "Todo")
        self.assertEqual(events[0]["status"], "running")
        self.assertEqual(events[0]["summary"], "1/2 completed")
        self.assertEqual(events[0]["progress_current"], 1)
        self.assertEqual(events[0]["progress_total"], 2)
        self.assertEqual(events[0]["items"][1]["label"], "Render progress")

    def test_codoxear_display_protocol_emits_extension_event(self) -> None:
        events, meta, _flags, diag = _extract_chat_events(
            [
                {
                    "type": "response_item",
                    "payload": {
                        "type": "function_call",
                        "name": "ralph_loop",
                        "call_id": "ralph-1",
                        "arguments": {
                            "codoxear_display": {
                                "version": 1,
                                "kind": "progress",
                                "source": "ralph-loop",
                                "title": "Ralph loop",
                                "status": "running",
                                "summary": "Iteration 2",
                                "progress": {"current": 2, "total": 5, "label": "iterations"},
                                "items": [
                                    {"label": "Patch parser", "status": "completed"},
                                    {"label": "Run checks", "status": "pending"},
                                ],
                            }
                        },
                    },
                    "ts": 10.0,
                }
            ]
        )
        self.assertEqual(meta["tool"], 1)
        self.assertEqual(diag["last_tool"], "ralph_loop")
        self.assertEqual(events[0]["type"], "extension")
        self.assertEqual(events[0]["source"], "ralph-loop")
        self.assertEqual(events[0]["title"], "Ralph loop")
        self.assertEqual(events[0]["progress_label"], "iterations")
        self.assertEqual(events[0]["items"][0]["status"], "completed")

    def test_codoxear_display_tool_result_emits_extension_event(self) -> None:
        events, meta, _flags, diag = _extract_chat_events(
            [
                {
                    "type": "response_item",
                    "payload": {
                        "type": "function_call_output",
                        "name": "ralph_status",
                        "call_id": "ralph-2",
                        "output": "Loop: demo-loop",
                        "structuredContent": {
                            "codoxear_display": {
                                "version": 1,
                                "kind": "progress",
                                "source": "ralph-loop",
                                "title": "Ralph loop",
                                "status": "running",
                                "summary": "Iteration 2/5",
                                "progress": {"current": 2, "total": 5, "label": "iterations"},
                                "items": [
                                    {"label": "Done item", "status": "completed"},
                                    {"label": "Next item", "status": "in_progress"},
                                ],
                                "text": "Loop: demo-loop",
                            }
                        },
                    },
                    "ts": 10.0,
                }
            ]
        )
        self.assertEqual(meta["tool"], 1)
        self.assertEqual(diag["last_tool"], "ralph_status")
        self.assertEqual(events[0]["type"], "extension")
        self.assertEqual(events[0]["source"], "ralph-loop")
        self.assertEqual(events[0]["summary"], "Iteration 2/5")
        self.assertEqual(events[0]["progress_current"], 2)
        self.assertEqual(events[0]["items"][1]["status"], "in_progress")

    def test_pi_ask_user_call_and_result_are_normalized(self) -> None:
        events, meta, _flags, diag = _extract_chat_events(
            [
                {
                    "type": "message",
                    "timestamp": "2026-04-24T10:00:00Z",
                    "message": {
                        "role": "assistant",
                        "content": [
                            {
                                "type": "toolCall",
                                "id": "ask-4",
                                "name": "AskUserQuestion",
                                "arguments": {
                                    "questions": [
                                        {
                                            "header": "Testing",
                                            "question": "How should we test this?",
                                            "options": ["Single", "Freeform", "Multiple"],
                                        }
                                    ]
                                },
                            }
                        ],
                    },
                },
                {
                    "type": "message",
                    "timestamp": "2026-04-24T10:00:01Z",
                    "message": {
                        "role": "toolResult",
                        "toolCallId": "ask-4",
                        "toolName": "AskUserQuestion",
                        "isError": False,
                        "details": {
                            "answer": "Single",
                            "wasCustom": False,
                        },
                    },
                },
            ]
        )
        self.assertEqual(meta["tool"], 2)
        self.assertEqual(diag["last_tool"], "pi_tool")
        self.assertEqual(events[0]["type"], "ask_user")
        self.assertFalse(events[0]["resolved"])
        self.assertEqual(events[0]["question"], "How should we test this?")
        self.assertEqual(events[0]["context"], "Testing")
        self.assertEqual(events[0]["options"], ["Single", "Freeform", "Multiple"])
        self.assertEqual(events[1]["type"], "ask_user")
        self.assertTrue(events[1]["resolved"])
        self.assertEqual(events[1]["answer"], "Single")
        self.assertFalse(events[1]["was_custom"])

    def test_pi_message_sets_turn_end_for_final_text(self) -> None:
        events, meta, flags, diag = _extract_chat_events(
            [
                {"type": "message", "message": {"role": "user", "content": [{"type": "text", "text": "hello"}]}},
                {"type": "message", "message": {"role": "assistant", "content": [{"type": "text", "text": "done"}]}},
            ]
        )
        self.assertTrue(flags["turn_start"])
        self.assertTrue(flags["turn_end"])
        self.assertEqual(events[1]["message_class"], "final_response")
        self.assertEqual(meta["thinking"], 0)
        self.assertEqual(meta["tool"], 0)
        self.assertEqual(diag["tool_names"], [])

    def test_pi_message_counts_thinking_and_tool_use_as_narration(self) -> None:
        events, meta, flags, diag = _extract_chat_events(
            [
                {
                    "type": "message",
                    "message": {
                        "role": "assistant",
                        "content": [
                            {"type": "text", "text": "working"},
                            {"type": "thinking", "thinking": "hmm"},
                            {"type": "toolCall", "id": "t1", "name": "bash", "arguments": {"command": "pwd"}},
                        ],
                    },
                }
            ]
        )
        self.assertFalse(flags["turn_end"])
        self.assertEqual(events[0]["message_class"], "narration")
        self.assertEqual(meta["thinking"], 1)
        self.assertEqual(meta["tool"], 1)
        self.assertEqual(diag["last_tool"], "pi_tool")

    def test_pi_final_message_with_thinking_sets_turn_end(self) -> None:
        events, meta, flags, _diag = _extract_chat_events(
            [
                {"type": "message", "message": {"role": "user", "content": [{"type": "text", "text": "test"}]}},
                {
                    "type": "message",
                    "message": {
                        "role": "assistant",
                        "stopReason": "stop",
                        "content": [
                            {"type": "thinking", "thinking": ""},
                            {"type": "text", "text": "done", "textSignature": "{\"v\":1,\"phase\":\"final_answer\"}"},
                        ],
                    },
                },
            ]
        )
        self.assertTrue(flags["turn_end"])
        self.assertEqual(events[-1]["message_class"], "final_response")
        self.assertEqual(meta["thinking"], 1)

    def test_compute_idle_from_log_pi_final_message_with_thinking_is_idle(self) -> None:
        with TemporaryDirectory() as td:
            path = Path(td) / "pi.jsonl"
            path.write_text(
                "\n".join(
                    [
                        '{"type":"session","id":"s1","cwd":"/tmp"}',
                        '{"type":"message","message":{"role":"user","content":[{"type":"text","text":"test"}]}}',
                        '{"type":"message","message":{"role":"assistant","stopReason":"stop","content":[{"type":"thinking","thinking":""},{"type":"text","text":"done","textSignature":"{\\"v\\":1,\\"phase\\":\\"final_answer\\"}"}]}}',
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            self.assertTrue(_compute_idle_from_log(path))

    def test_pi_token_update_uses_models_json_context_window(self) -> None:
        with TemporaryDirectory() as td:
            models_path = Path(td) / "models.json"
            models_path.write_text(
                '{"providers":{"macaron":{"models":[{"id":"gpt-5.4","contextWindow":1000000}]}}}\n',
                encoding="utf-8",
            )
            obj = {
                "type": "message",
                "timestamp": "2026-03-30T08:44:03.883Z",
                "message": {
                    "role": "assistant",
                    "provider": "macaron",
                    "model": "gpt-5.4",
                    "usage": {"totalTokens": 11817},
                    "content": [{"type": "text", "text": "done"}],
                },
            }
            token = pi_token_update(obj, models_path=models_path)
        self.assertIsNotNone(token)
        self.assertEqual(token["context_window"], 1000000)
        self.assertEqual(token["tokens_in_context"], 11817)


if __name__ == "__main__":
    unittest.main()
