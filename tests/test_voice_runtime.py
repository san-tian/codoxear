import json
import os
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from codoxear.voice_runtime import VoiceRuntime


class _FakeCoordinator:
    def __init__(self) -> None:
        self.observed = []

    def observe_messages(self, **kwargs):
        self.observed.append(kwargs)


def _append_jsonl(path: Path, obj: dict) -> None:
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(obj) + "\n")


class TestVoiceRuntime(unittest.TestCase):
    def test_scans_new_log_delta_without_full_session_manager(self) -> None:
        with TemporaryDirectory() as td:
            app_dir = Path(td) / "app"
            sock_dir = app_dir / "socks"
            sock_dir.mkdir(parents=True)
            log_path = Path(td) / "rollout-2026-04-28-00000000-0000-0000-0000-000000000001.jsonl"
            _append_jsonl(
                log_path,
                {
                    "type": "event_msg",
                    "timestamp": "2026-04-28T00:00:00Z",
                    "payload": {"type": "agent_message", "message": "old text", "phase": "final_answer"},
                },
            )
            (app_dir / "session_aliases.json").write_text(
                json.dumps({"00000000-0000-0000-0000-000000000001": "Repo Alias"}),
                encoding="utf-8",
            )
            (sock_dir / "broker.json").write_text(
                json.dumps(
                    {
                        "session_id": "00000000-0000-0000-0000-000000000001",
                        "broker_pid": os.getpid(),
                        "codex_pid": 0,
                        "agent_backend": "codex",
                        "cwd": str(Path(td) / "repo"),
                        "log_path": str(log_path),
                    }
                ),
                encoding="utf-8",
            )
            fake = _FakeCoordinator()
            runtime = VoiceRuntime(app_dir=app_dir, coordinator=fake)

            runtime.discover_sessions()
            _append_jsonl(
                log_path,
                {
                    "type": "event_msg",
                    "timestamp": "2026-04-28T00:00:01Z",
                    "payload": {"type": "agent_message", "message": "new final", "phase": "final_answer"},
                },
            )
            runtime.scan_once()

        self.assertEqual(len(fake.observed), 1)
        self.assertEqual(fake.observed[0]["session_display_name"], "Repo Alias")
        self.assertEqual(fake.observed[0]["messages"][0].message_class, "final_response")
        self.assertEqual(fake.observed[0]["messages"][0].text, "new final")

    def test_resume_sessions_advance_offset_without_delivery(self) -> None:
        with TemporaryDirectory() as td:
            app_dir = Path(td) / "app"
            sock_dir = app_dir / "socks"
            sock_dir.mkdir(parents=True)
            log_path = Path(td) / "rollout-2026-04-28-00000000-0000-0000-0000-000000000002.jsonl"
            (sock_dir / "broker.json").write_text(
                json.dumps(
                    {
                        "session_id": "00000000-0000-0000-0000-000000000002",
                        "broker_pid": os.getpid(),
                        "codex_pid": 0,
                        "agent_backend": "codex",
                        "cwd": str(Path(td) / "repo"),
                        "log_path": str(log_path),
                        "resume_session_id": "resume-source",
                    }
                ),
                encoding="utf-8",
            )
            fake = _FakeCoordinator()
            runtime = VoiceRuntime(app_dir=app_dir, coordinator=fake)

            runtime.discover_sessions()
            _append_jsonl(
                log_path,
                {
                    "type": "event_msg",
                    "timestamp": "2026-04-28T00:00:01Z",
                    "payload": {"type": "agent_message", "message": "muted final", "phase": "final_answer"},
                },
            )
            runtime.scan_once()
            session = runtime.sessions["00000000-0000-0000-0000-000000000002"]

        self.assertEqual(fake.observed, [])
        self.assertGreater(session.delivery_log_off, 0)


if __name__ == "__main__":
    unittest.main()
