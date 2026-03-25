from __future__ import annotations

import os
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

from codoxear.cli_support import cli_bin
from codoxear.cli_support import cli_home
from codoxear.cli_support import cli_logs_dir
from codoxear.cli_support import infer_cli_from_log_path
from codoxear.cli_support import is_codex_rollout_log_path
from codoxear.cli_support import normalize_cli_name
from codoxear.cli_support import parse_cli_name
from codoxear.cli_support import session_id_from_log_path


class TestCliSupport(unittest.TestCase):
    def test_normalize_and_parse_codex(self) -> None:
        self.assertEqual(normalize_cli_name("codex"), "codex")
        self.assertEqual(normalize_cli_name("openai-codex"), "codex")
        self.assertEqual(parse_cli_name("codex"), "codex")
        with self.assertRaises(ValueError):
            parse_cli_name("claude")
        with self.assertRaises(ValueError):
            parse_cli_name("gemini")

    def test_codex_home_bin_and_logs_dir(self) -> None:
        with TemporaryDirectory() as td:
            home = Path(td) / ".codex"
            with patch.dict(os.environ, {"CODEX_HOME": str(home), "CODEX_BIN": "/usr/local/bin/codex"}, clear=False):
                self.assertEqual(cli_home("codex"), home)
                self.assertEqual(cli_logs_dir("codex"), home / "sessions")
                self.assertEqual(cli_bin("codex"), "/usr/local/bin/codex")

    def test_codex_log_helpers(self) -> None:
        log = Path("/tmp/rollout-2026-03-20T00-00-00-aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb.jsonl")
        self.assertTrue(is_codex_rollout_log_path(log))
        self.assertEqual(infer_cli_from_log_path(log), "codex")
        self.assertEqual(session_id_from_log_path(log), "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb")


if __name__ == "__main__":
    unittest.main()
