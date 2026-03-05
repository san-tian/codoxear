from __future__ import annotations

import unittest
import uuid
from pathlib import Path
import tempfile
from unittest.mock import patch

from codoxear.server import SessionManager


class _DummyProc:
    def __init__(self, pid: int) -> None:
        self.pid = int(pid)
        self.stderr = None

    def wait(self) -> int:
        return 0


class _DummyRun:
    def __init__(self, returncode: int = 0, *, stdout: str = "", stderr: str = "") -> None:
        self.returncode = int(returncode)
        self.stdout = stdout
        self.stderr = stderr


class TestServerSpawnCli(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self._dotenv = Path(self._tmp.name) / ".env"
        self._dotenv_patcher = patch("codoxear.server._DOTENV", self._dotenv)
        self._dotenv_patcher.start()

    def tearDown(self) -> None:
        self._dotenv_patcher.stop()
        self._tmp.cleanup()

    def _mgr(self) -> SessionManager:
        return SessionManager.__new__(SessionManager)

    def test_spawn_web_session_sets_claude_env(self) -> None:
        mgr = self._mgr()
        with patch.dict(
            "codoxear.server.os.environ",
            {"ANTHROPIC_API_KEY": "api-key", "ANTHROPIC_AUTH_TOKEN": "auth-token"},
            clear=False,
        ), patch("codoxear.server._env_flag", return_value=False), patch(
            "codoxear.server._wait_or_raise", return_value=None
        ), patch("codoxear.server.subprocess.Popen", return_value=_DummyProc(4321)) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="claude")

        self.assertEqual(res.get("broker_pid"), 4321)
        self.assertEqual(res.get("cli"), "claude")
        env = popen.call_args.kwargs["env"]
        self.assertEqual(env.get("CODEX_WEB_CLI"), "claude")
        self.assertTrue(bool(env.get("CLAUDE_HOME")))
        self.assertTrue(bool(env.get("CLAUDE_BIN")))
        self.assertEqual(env.get("ANTHROPIC_API_KEY"), "api-key")
        self.assertNotIn("ANTHROPIC_AUTH_TOKEN", env)
        self.assertEqual(env.get("CODEX_WEB_UNSET_ANTHROPIC_AUTH_TOKEN"), "1")
        argv = popen.call_args.args[0]
        self.assertIn("--session-id", argv)
        sid = argv[argv.index("--session-id") + 1]
        self.assertEqual(str(uuid.UUID(sid)), sid)

    def test_spawn_web_session_claude_keeps_auth_token_when_opted_out(self) -> None:
        mgr = self._mgr()
        with patch.dict(
            "codoxear.server.os.environ",
            {
                "ANTHROPIC_API_KEY": "api-key",
                "ANTHROPIC_AUTH_TOKEN": "auth-token",
                "CODEX_WEB_CLAUDE_PREFER_API_KEY": "0",
            },
            clear=False,
        ), patch("codoxear.server._env_flag", return_value=False), patch(
            "codoxear.server._wait_or_raise", return_value=None
        ), patch("codoxear.server.subprocess.Popen", return_value=_DummyProc(4322)) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="claude")

        self.assertEqual(res.get("broker_pid"), 4322)
        self.assertEqual(res.get("cli"), "claude")
        env = popen.call_args.kwargs["env"]
        self.assertEqual(env.get("ANTHROPIC_AUTH_TOKEN"), "auth-token")
        self.assertNotIn("CODEX_WEB_UNSET_ANTHROPIC_AUTH_TOKEN", env)

    def test_spawn_web_session_claude_tmux_unsets_auth_token(self) -> None:
        mgr = self._mgr()
        with patch.dict(
            "codoxear.server.os.environ",
            {"ANTHROPIC_API_KEY": "api-key", "ANTHROPIC_AUTH_TOKEN": "auth-token"},
            clear=False,
        ), patch("codoxear.server._env_flag", return_value=True), patch(
            "codoxear.server.shutil.which", return_value="/usr/bin/tmux"
        ), patch("codoxear.server._tmux_pane_pid", return_value=7777), patch(
            "codoxear.server.subprocess.run", return_value=_DummyRun(0)
        ) as run_mock:
            res = mgr.spawn_web_session(cwd="/tmp", cli="claude")

        self.assertEqual(res.get("broker_pid"), 7777)
        self.assertEqual(res.get("cli"), "claude")
        tmux_cmd = run_mock.call_args.args[0]
        self.assertIn("env", tmux_cmd)
        env_idx = tmux_cmd.index("env")
        self.assertEqual(tmux_cmd[env_idx + 1 : env_idx + 3], ["-u", "ANTHROPIC_AUTH_TOKEN"])
        env = run_mock.call_args.kwargs["env"]
        self.assertNotIn("ANTHROPIC_AUTH_TOKEN", env)
        self.assertEqual(env.get("CODEX_WEB_UNSET_ANTHROPIC_AUTH_TOKEN"), "1")

    def test_spawn_web_session_claude_tmux_detects_conflict_from_tmux_env(self) -> None:
        mgr = self._mgr()
        with patch.dict(
            "codoxear.server.os.environ",
            {"ANTHROPIC_API_KEY": "", "ANTHROPIC_AUTH_TOKEN": ""},
            clear=False,
        ), patch("codoxear.server._env_flag", return_value=True), patch(
            "codoxear.server.shutil.which", return_value="/usr/bin/tmux"
        ), patch("codoxear.server._tmux_pane_pid", return_value=7788), patch(
            "codoxear.server.subprocess.run", return_value=_DummyRun(0)
        ) as run_mock, patch(
            "codoxear.server._tmux_global_env_has_nonempty",
            side_effect=lambda _tmux, key, _env: key in ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"),
        ) as has_env_mock:
            res = mgr.spawn_web_session(cwd="/tmp", cli="claude")

        self.assertEqual(res.get("broker_pid"), 7788)
        self.assertEqual(res.get("cli"), "claude")
        self.assertGreaterEqual(has_env_mock.call_count, 2)
        tmux_cmd = run_mock.call_args.args[0]
        env_idx = tmux_cmd.index("env")
        self.assertEqual(tmux_cmd[env_idx + 1 : env_idx + 3], ["-u", "ANTHROPIC_AUTH_TOKEN"])

    def test_spawn_web_session_sets_codex_env(self) -> None:
        mgr = self._mgr()
        with patch("codoxear.server._env_flag", return_value=False), patch(
            "codoxear.server._wait_or_raise", return_value=None
        ), patch("codoxear.server.subprocess.Popen", return_value=_DummyProc(5432)) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="codex")

        self.assertEqual(res.get("broker_pid"), 5432)
        self.assertEqual(res.get("cli"), "codex")
        env = popen.call_args.kwargs["env"]
        self.assertEqual(env.get("CODEX_WEB_CLI"), "codex")
        self.assertTrue(bool(env.get("CODEX_HOME")))
        self.assertTrue(bool(env.get("CODEX_BIN")))

    def test_spawn_web_session_codex_appends_yolo_arg(self) -> None:
        mgr = self._mgr()
        with patch.dict("codoxear.server.os.environ", {"CODEX_WEB_CODEX_YOLO": "1"}, clear=True), patch(
            "codoxear.server._env_flag", return_value=False
        ), patch("codoxear.server._wait_or_raise", return_value=None), patch(
            "codoxear.server.subprocess.Popen", return_value=_DummyProc(5433)
        ) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="codex")

        self.assertEqual(res.get("broker_pid"), 5433)
        argv = popen.call_args.args[0]
        self.assertIn("--dangerously-bypass-approvals-and-sandbox", argv)

    def test_spawn_web_session_codex_respects_explicit_approval_args(self) -> None:
        mgr = self._mgr()
        with patch.dict("codoxear.server.os.environ", {"CODEX_WEB_CODEX_YOLO": "1"}, clear=True), patch(
            "codoxear.server._env_flag", return_value=False
        ), patch("codoxear.server._wait_or_raise", return_value=None), patch(
            "codoxear.server.subprocess.Popen", return_value=_DummyProc(5434)
        ) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="codex", args=["-a", "on-request"])

        self.assertEqual(res.get("broker_pid"), 5434)
        argv = popen.call_args.args[0]
        self.assertNotIn("--dangerously-bypass-approvals-and-sandbox", argv)

    def test_spawn_web_session_claude_respects_explicit_resume_args(self) -> None:
        mgr = self._mgr()
        with patch("codoxear.server._env_flag", return_value=False), patch(
            "codoxear.server._wait_or_raise", return_value=None
        ), patch("codoxear.server.subprocess.Popen", return_value=_DummyProc(7654)) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="claude", args=["--resume", "sid-1"])

        self.assertEqual(res.get("broker_pid"), 7654)
        self.assertEqual(res.get("cli"), "claude")
        argv = popen.call_args.args[0]
        self.assertIn("--resume", argv)
        self.assertNotIn("--session-id", argv)

    def test_spawn_web_session_claude_appends_yolo_arg(self) -> None:
        mgr = self._mgr()
        with patch.dict("codoxear.server.os.environ", {"CODEX_WEB_CLAUDE_YOLO": "1"}, clear=True), patch(
            "codoxear.server._running_as_root", return_value=False
        ), patch(
            "codoxear.server._env_flag", return_value=False
        ), patch("codoxear.server._wait_or_raise", return_value=None), patch(
            "codoxear.server.subprocess.Popen", return_value=_DummyProc(7655)
        ) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="claude")

        self.assertEqual(res.get("broker_pid"), 7655)
        argv = popen.call_args.args[0]
        self.assertIn("--dangerously-skip-permissions", argv)

    def test_spawn_web_session_claude_respects_explicit_permission_mode(self) -> None:
        mgr = self._mgr()
        with patch.dict("codoxear.server.os.environ", {"CODEX_WEB_CLAUDE_YOLO": "1"}, clear=True), patch(
            "codoxear.server._env_flag", return_value=False
        ), patch("codoxear.server._wait_or_raise", return_value=None), patch(
            "codoxear.server.subprocess.Popen", return_value=_DummyProc(7656)
        ) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="claude", args=["--permission-mode", "plan"])

        self.assertEqual(res.get("broker_pid"), 7656)
        argv = popen.call_args.args[0]
        self.assertNotIn("--dangerously-skip-permissions", argv)

    def test_spawn_web_session_claude_yolo_prefers_dotenv_value(self) -> None:
        mgr = self._mgr()
        self._dotenv.write_text("CODEX_WEB_CLAUDE_YOLO=1\n", encoding="utf-8")
        with patch.dict("codoxear.server.os.environ", {"CODEX_WEB_CLAUDE_YOLO": "0"}, clear=True), patch(
            "codoxear.server._running_as_root", return_value=False
        ), patch(
            "codoxear.server._env_flag", return_value=False
        ), patch("codoxear.server._wait_or_raise", return_value=None), patch(
            "codoxear.server.subprocess.Popen", return_value=_DummyProc(7657)
        ) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="claude")

        self.assertEqual(res.get("broker_pid"), 7657)
        argv = popen.call_args.args[0]
        self.assertIn("--dangerously-skip-permissions", argv)

    def test_spawn_web_session_claude_skips_yolo_arg_on_root(self) -> None:
        mgr = self._mgr()
        with patch.dict("codoxear.server.os.environ", {"CODEX_WEB_CLAUDE_YOLO": "1"}, clear=True), patch(
            "codoxear.server._running_as_root", return_value=True
        ), patch("codoxear.server._env_flag", return_value=False), patch(
            "codoxear.server._wait_or_raise", return_value=None
        ), patch("codoxear.server.subprocess.Popen", return_value=_DummyProc(7658)) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="claude")

        self.assertEqual(res.get("broker_pid"), 7658)
        argv = popen.call_args.args[0]
        self.assertNotIn("--dangerously-skip-permissions", argv)

    def test_spawn_web_session_sets_gemini_env(self) -> None:
        mgr = self._mgr()
        with patch("codoxear.server._env_flag", return_value=False), patch(
            "codoxear.server._wait_or_raise", return_value=None
        ), patch("codoxear.server.subprocess.Popen", return_value=_DummyProc(6543)) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="gemini")

        self.assertEqual(res.get("broker_pid"), 6543)
        self.assertEqual(res.get("cli"), "gemini")
        env = popen.call_args.kwargs["env"]
        self.assertEqual(env.get("CODEX_WEB_CLI"), "gemini")
        self.assertTrue(bool(env.get("GEMINI_HOME")))
        self.assertTrue(bool(env.get("GEMINI_BIN")))

    def test_spawn_web_session_gemini_appends_yolo_arg(self) -> None:
        mgr = self._mgr()
        with patch.dict("codoxear.server.os.environ", {"CODEX_WEB_GEMINI_YOLO": "1"}, clear=True), patch(
            "codoxear.server._env_flag", return_value=False
        ), patch("codoxear.server._wait_or_raise", return_value=None), patch(
            "codoxear.server.subprocess.Popen", return_value=_DummyProc(6544)
        ) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="gemini")

        self.assertEqual(res.get("broker_pid"), 6544)
        argv = popen.call_args.args[0]
        self.assertIn("--yolo", argv)

    def test_spawn_web_session_gemini_skips_yolo_for_custom_wrapper_bin(self) -> None:
        mgr = self._mgr()
        with patch.dict(
            "codoxear.server.os.environ",
            {"CODEX_WEB_GEMINI_YOLO": "1", "GEMINI_BIN": "/usr/local/bin/gemini-web"},
            clear=True,
        ), patch("codoxear.server._env_flag", return_value=False), patch(
            "codoxear.server._wait_or_raise", return_value=None
        ), patch("codoxear.server.subprocess.Popen", return_value=_DummyProc(6546)) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="gemini")

        self.assertEqual(res.get("broker_pid"), 6546)
        argv = popen.call_args.args[0]
        self.assertNotIn("--yolo", argv)

    def test_spawn_web_session_gemini_respects_explicit_approval_mode(self) -> None:
        mgr = self._mgr()
        with patch.dict("codoxear.server.os.environ", {"CODEX_WEB_GEMINI_YOLO": "1"}, clear=True), patch(
            "codoxear.server._env_flag", return_value=False
        ), patch("codoxear.server._wait_or_raise", return_value=None), patch(
            "codoxear.server.subprocess.Popen", return_value=_DummyProc(6545)
        ) as popen:
            res = mgr.spawn_web_session(cwd="/tmp", cli="gemini", args=["--approval-mode", "plan"])

        self.assertEqual(res.get("broker_pid"), 6545)
        argv = popen.call_args.args[0]
        self.assertNotIn("--yolo", argv)

    def test_spawn_web_session_rejects_unknown_cli(self) -> None:
        mgr = self._mgr()
        with self.assertRaises(ValueError):
            mgr.spawn_web_session(cwd="/tmp", cli="unknown")


if __name__ == "__main__":
    unittest.main()
