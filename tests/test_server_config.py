from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from codoxear.server import _read_cli_config
from codoxear.server import _save_cli_config


class TestServerConfig(unittest.TestCase):
    def test_save_config_persists_claude_env_and_model(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            home = root / "home"
            home.mkdir(parents=True, exist_ok=True)
            dotenv = root / ".env"
            dotenv.write_text("CODEX_WEB_PASSWORD=test\n", encoding="utf-8")

            with patch("codoxear.server.Path.home", return_value=home), patch(
                "codoxear.server._DOTENV", dotenv
            ), patch.dict("codoxear.server.os.environ", {}, clear=True) as env:
                result = _save_cli_config(
                    {
                        "claude": {
                            "api_key": "sk-ant-1234567890",
                            "base_url": "https://claude.example/v1",
                            "model": "claude-3-7-sonnet-20250219",
                        }
                    }
                )

                self.assertTrue(bool(result.get("ok")))
                self.assertIn("claude_env", result.get("updated", []))
                self.assertIn("claude_settings", result.get("updated", []))

                env_text = dotenv.read_text(encoding="utf-8")
                self.assertIn("ANTHROPIC_API_KEY=sk-ant-1234567890", env_text)
                self.assertIn("ANTHROPIC_BASE_URL=https://claude.example/v1", env_text)
                self.assertNotIn("ANTHROPIC_AUTH_TOKEN=", env_text)
                self.assertEqual(env.get("ANTHROPIC_API_KEY"), "sk-ant-1234567890")

                settings_path = home / ".claude" / "settings.json"
                self.assertTrue(settings_path.exists())
                settings = json.loads(settings_path.read_text(encoding="utf-8"))
                self.assertEqual(settings.get("model"), "claude-3-7-sonnet-20250219")

    def test_save_config_clears_claude_env_when_empty(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            home = root / "home"
            home.mkdir(parents=True, exist_ok=True)
            dotenv = root / ".env"
            dotenv.write_text(
                "\n".join(
                    [
                        "ANTHROPIC_API_KEY=sk-ant-old",
                        "ANTHROPIC_AUTH_TOKEN=auth-old",
                        "ANTHROPIC_BASE_URL=https://old.example",
                        "",
                    ]
                ),
                encoding="utf-8",
            )

            with patch("codoxear.server.Path.home", return_value=home), patch(
                "codoxear.server._DOTENV", dotenv
            ), patch.dict(
                "codoxear.server.os.environ",
                {
                    "ANTHROPIC_API_KEY": "sk-ant-old",
                    "ANTHROPIC_AUTH_TOKEN": "auth-old",
                    "ANTHROPIC_BASE_URL": "https://old.example",
                },
                clear=True,
            ) as env:
                result = _save_cli_config({"claude": {"api_key": "", "base_url": ""}})

                self.assertTrue(bool(result.get("ok")))
                self.assertIn("claude_env", result.get("updated", []))
                env_text = dotenv.read_text(encoding="utf-8")
                self.assertNotIn("ANTHROPIC_API_KEY=", env_text)
                self.assertNotIn("ANTHROPIC_AUTH_TOKEN=", env_text)
                self.assertNotIn("ANTHROPIC_BASE_URL=", env_text)
                self.assertNotIn("ANTHROPIC_API_KEY", env)
                self.assertNotIn("ANTHROPIC_AUTH_TOKEN", env)
                self.assertNotIn("ANTHROPIC_BASE_URL", env)

    def test_read_cli_config_masks_secret_env_values(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            dotenv = Path(td) / ".env"
            with patch("codoxear.server._DOTENV", dotenv), patch.dict(
                "codoxear.server.os.environ",
                {
                    "ANTHROPIC_API_KEY": "sk-ant-1234567890",
                    "ANTHROPIC_BASE_URL": "https://claude.example/v1",
                },
                clear=True,
            ):
                config = _read_cli_config()

        self.assertEqual(config["env"]["ANTHROPIC_API_KEY"], "sk-ant-1234567890")
        self.assertEqual(config["env"]["ANTHROPIC_BASE_URL"], "https://claude.example/v1")
        self.assertEqual(config["env_masked"]["ANTHROPIC_API_KEY"], "sk-a******7890")

    def test_read_cli_config_prefers_dotenv_for_provider_values(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            dotenv = root / ".env"
            dotenv.write_text(
                "\n".join(
                    [
                        "ANTHROPIC_API_KEY=",
                        "ANTHROPIC_AUTH_TOKEN=token-from-dotenv",
                        "ANTHROPIC_BASE_URL=https://dotenv.example",
                        "",
                    ]
                ),
                encoding="utf-8",
            )

            with patch("codoxear.server._DOTENV", dotenv), patch.dict(
                "codoxear.server.os.environ",
                {
                    "ANTHROPIC_API_KEY": "api-from-env",
                    "ANTHROPIC_AUTH_TOKEN": "token-from-env",
                    "ANTHROPIC_BASE_URL": "https://env.example",
                },
                clear=True,
            ):
                config = _read_cli_config()

        self.assertNotIn("ANTHROPIC_API_KEY", config["env"])
        self.assertEqual(config["env"]["ANTHROPIC_AUTH_TOKEN"], "token-from-dotenv")
        self.assertEqual(config["env"]["ANTHROPIC_BASE_URL"], "https://dotenv.example")

    def test_save_config_persists_gemini_env(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            home = root / "home"
            home.mkdir(parents=True, exist_ok=True)
            dotenv = root / ".env"

            with patch("codoxear.server.Path.home", return_value=home), patch(
                "codoxear.server._DOTENV", dotenv
            ), patch.dict("codoxear.server.os.environ", {}, clear=True) as env:
                result = _save_cli_config(
                    {
                        "gemini": {
                            "api_key": "AIza123",
                            "base_url": "https://gemini.example",
                            "model": "gemini-2.5-pro",
                        }
                    }
                )

                self.assertTrue(bool(result.get("ok")))
                self.assertIn("gemini_env", result.get("updated", []))
                env_text = dotenv.read_text(encoding="utf-8")
                self.assertIn("GEMINI_API_KEY=AIza123", env_text)
                self.assertIn("GOOGLE_GEMINI_BASE_URL=https://gemini.example", env_text)
                self.assertIn("GEMINI_MODEL=gemini-2.5-pro", env_text)
                self.assertEqual(env.get("GEMINI_API_KEY"), "AIza123")
                self.assertEqual(env.get("GOOGLE_GEMINI_BASE_URL"), "https://gemini.example")
                self.assertEqual(env.get("GEMINI_MODEL"), "gemini-2.5-pro")

    def test_save_config_persists_provider_yolo_flags(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            home = root / "home"
            home.mkdir(parents=True, exist_ok=True)
            dotenv = root / ".env"

            with patch("codoxear.server.Path.home", return_value=home), patch(
                "codoxear.server._DOTENV", dotenv
            ), patch.dict("codoxear.server.os.environ", {}, clear=True) as env:
                result = _save_cli_config(
                    {
                        "codex": {"yolo": True},
                        "claude": {"yolo": False},
                        "gemini": {"yolo": "1"},
                    }
                )

                self.assertTrue(bool(result.get("ok")))
                self.assertIn("codex_env", result.get("updated", []))
                self.assertIn("claude_env", result.get("updated", []))
                self.assertIn("gemini_env", result.get("updated", []))
                env_text = dotenv.read_text(encoding="utf-8")
                self.assertIn("CODEX_WEB_CODEX_YOLO=1", env_text)
                self.assertIn("CODEX_WEB_CLAUDE_YOLO=0", env_text)
                self.assertIn("CODEX_WEB_GEMINI_YOLO=1", env_text)
                self.assertEqual(env.get("CODEX_WEB_CODEX_YOLO"), "1")
                self.assertEqual(env.get("CODEX_WEB_CLAUDE_YOLO"), "0")
                self.assertEqual(env.get("CODEX_WEB_GEMINI_YOLO"), "1")


if __name__ == "__main__":
    unittest.main()
