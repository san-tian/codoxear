from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
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

    def test_spawn_web_session_ignores_legacy_yolo_env(self) -> None:
        mgr = self._mgr()
        with patch.dict("codoxear.server.os.environ", {"CODEX_WEB_CODEX_YOLO": "1"}, clear=True), patch(
            "codoxear.server._env_flag", return_value=False
        ), patch("codoxear.server._wait_or_raise", return_value=None), patch(
            "codoxear.server.subprocess.Popen", return_value=_DummyProc(5433)
        ) as popen:
            mgr.spawn_web_session(cwd="/tmp", cli="codex")

        argv = popen.call_args.args[0]
        self.assertNotIn("--dangerously-bypass-approvals-and-sandbox", argv)

    def test_spawn_web_session_tmux_wraps_codex_broker(self) -> None:
        mgr = self._mgr()
        with patch("codoxear.server._env_flag", return_value=True), patch(
            "codoxear.server.shutil.which", return_value="/usr/bin/tmux"
        ), patch("codoxear.server._tmux_pane_pid", return_value=7777), patch(
            "codoxear.server.subprocess.run", return_value=_DummyRun(0)
        ) as run_mock:
            res = mgr.spawn_web_session(cwd="/tmp", cli="codex")

        self.assertEqual(res.get("broker_pid"), 7777)
        tmux_cmd = run_mock.call_args.args[0]
        self.assertIn("env", tmux_cmd)
        env = run_mock.call_args.kwargs["env"]
        self.assertEqual(env.get("CODEX_WEB_CLI"), "codex")

    def test_spawn_web_session_rejects_removed_clis(self) -> None:
        mgr = self._mgr()
        for cli in ("claude", "gemini", "unknown"):
            with self.assertRaises(ValueError):
                mgr.spawn_web_session(cwd="/tmp", cli=cli)


if __name__ == "__main__":
    unittest.main()
