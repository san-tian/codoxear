from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from codoxear.server import _read_cli_config
from codoxear.server import _save_cli_config


class TestServerConfig(unittest.TestCase):
    def test_read_cli_config_returns_raw_file_text(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            home = root / "home"
            codex_dir = home / ".codex"
            codex_dir.mkdir(parents=True, exist_ok=True)
            config_text = 'model = "gpt-5"\n'
            auth_text = '{\n  "auth_mode": "apikey"\n}\n'
            (codex_dir / "config.toml").write_text(config_text, encoding="utf-8")
            (codex_dir / "auth.json").write_text(auth_text, encoding="utf-8")

            with patch("codoxear.server.Path.home", return_value=home):
                config = _read_cli_config()

        codex = config["codex"]
        self.assertEqual(codex["config_toml_text"], config_text)
        self.assertEqual(codex["auth_json_text"], auth_text)
        self.assertEqual(codex["config_toml_path"], str(codex_dir / "config.toml"))
        self.assertEqual(codex["auth_json_path"], str(codex_dir / "auth.json"))

    def test_save_cli_config_writes_raw_file_text(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            home = root / "home"
            codex_dir = home / ".codex"
            codex_dir.mkdir(parents=True, exist_ok=True)
            (codex_dir / "config.toml").write_text('model = "old"\n', encoding="utf-8")
            (codex_dir / "auth.json").write_text('{"auth_mode":"old"}\n', encoding="utf-8")

            new_config = 'model = "gpt-5"\napproval_policy = "never"\n'
            new_auth = '{\n  "auth_mode": "apikey",\n  "OPENAI_API_KEY": "sk-test"\n}\n'

            with patch("codoxear.server.Path.home", return_value=home):
                result = _save_cli_config(
                    {
                        "codex": {
                            "config_toml_text": new_config,
                            "auth_json_text": new_auth,
                        }
                    }
                )

            self.assertTrue(bool(result.get("ok")))
            self.assertIn("codex_config", result.get("updated", []))
            self.assertIn("codex_auth", result.get("updated", []))
            self.assertEqual((codex_dir / "config.toml").read_text(encoding="utf-8"), new_config)
            self.assertEqual((codex_dir / "auth.json").read_text(encoding="utf-8"), new_auth)

    def test_save_cli_config_creates_missing_files(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            home = root / "home"
            new_config = "model = \"gpt-5\"\n"
            new_auth = "{}\n"

            with patch("codoxear.server.Path.home", return_value=home):
                result = _save_cli_config(
                    {
                        "codex": {
                            "config_toml_text": new_config,
                            "auth_json_text": new_auth,
                        }
                    }
                )

            codex_dir = home / ".codex"
            self.assertTrue(bool(result.get("ok")))
            self.assertEqual((codex_dir / "config.toml").read_text(encoding="utf-8"), new_config)
            self.assertEqual((codex_dir / "auth.json").read_text(encoding="utf-8"), new_auth)


if __name__ == "__main__":
    unittest.main()
