import os
import tomllib
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

from codoxear.server import _normalize_requested_model_provider
from codoxear.server import _normalize_requested_preferred_auth_method
from codoxear.server import _normalize_requested_service_tier
from codoxear.server import _list_directory_suggestions
from codoxear.server import _read_codex_launch_defaults
from codoxear.server import _read_codex_config_for_settings
from codoxear.server import _read_new_session_defaults
from codoxear.server import _schedule_local_service_restart
from codoxear.server import _write_codex_config_for_settings
from codoxear.server import _read_pi_launch_defaults


CODEX_LAUNCH_DEFAULT_ENV_KEYS = (
    "CODEX_WEB_DEFAULT_MODEL_PROVIDER",
    "CODEX_WEB_DEFAULT_PREFERRED_AUTH_METHOD",
    "CODEX_WEB_DEFAULT_MODEL",
    "CODEX_WEB_DEFAULT_REASONING_EFFORT",
    "CODEX_WEB_DEFAULT_SERVICE_TIER",
)


def _codex_launch_default_env(**values: str):
    env = {key: "" for key in CODEX_LAUNCH_DEFAULT_ENV_KEYS}
    env.update(values)
    return patch.dict(os.environ, env)


class TestLaunchDefaults(unittest.TestCase):
    def test_server_main_runtime_only_skips_http_bind(self) -> None:
        import codoxear.server as server_module

        class _StopEvent:
            def __init__(self) -> None:
                self.wait_calls: list[object | None] = []

            def wait(self, timeout: object | None = None) -> bool:
                self.wait_calls.append(timeout)
                return True

        class _Manager:
            def __init__(self) -> None:
                self._stop = _StopEvent()

            def stop(self) -> None:
                self._stop.wait_calls.append("stopped")

        fake_manager = _Manager()

        with patch.object(server_module, "MANAGER", fake_manager), \
            patch.object(server_module.os, "makedirs", lambda *args, **kwargs: None), \
            patch.object(server_module, "_require_password", lambda: None), \
            patch.object(server_module, "ThreadingHTTPServer") as http_mock, \
            patch.object(server_module, "ThreadingHTTPServerV6") as http6_mock, \
            patch.object(server_module.signal, "signal") as signal_mock:
            server_module.main(["--runtime-only"])

        self.assertEqual(fake_manager._stop.wait_calls, [None])
        http_mock.assert_not_called()
        http6_mock.assert_not_called()
        self.assertEqual(
            [call.args[0] for call in signal_mock.call_args_list],
            [server_module.signal.SIGTERM, server_module.signal.SIGINT],
        )

    def test_local_daemon_uses_rust_voice_worker(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        script_path = repo_root / "scripts" / "codoxear-local"
        source = script_path.read_text(encoding="utf-8")

        self.assertNotIn('"$PYTHON_BIN" -m codoxear.voice_runtime', source)
        self.assertNotIn('"$PYTHON_BIN" -m codoxear.server --runtime-only', source)
        self.assertNotIn('CODEX_WEB_DISABLE_HARNESS_SWEEP=1', source)
        self.assertNotIn('CODEX_WEB_DISABLE_QUEUE_SWEEP=1', source)
        self.assertNotIn('CODEX_WEB_DISABLE_VOICE_SCAN=1', source)
        self.assertIn('CODOXEAR_ENABLE_HARNESS_SWEEP=1', source)
        self.assertIn('CODOXEAR_ENABLE_QUEUE_SWEEP=1', source)
        self.assertIn('CODOXEAR_ENABLE_VOICE_SCAN=1', source)
        self.assertIn('CODOXEAR_ENABLE_VOICE_WORKER=1', source)
        self.assertIn('BROKER_BIN="$ROOT_DIR/backend-rs/target/release/codoxear-broker-rs"', source)
        self.assertNotIn('CODOXEAR_ENABLE_RUST_BROKER', source)
        self.assertIn('CODOXEAR_RUST_BROKER_BIN="$BROKER_BIN"', source)
        self.assertIn('cargo build --release --bins', source)
        self.assertNotIn('CODEX_WEB_PORT=8744', source)
        self.assertNotIn('CODEX_WEB_NOVA_LEGACY_BASE=http://127.0.0.1:8744', source)
        self.assertNotIn('runtime python pid', source)
        self.assertNotIn('legacy python pid', source)

    def test_list_directory_suggestions_returns_children_for_existing_directory(self) -> None:
        with TemporaryDirectory() as td:
            workspace = Path(td) / "workspace"
            alpha = workspace / "alpha"
            beta = workspace / "beta"
            workspace.mkdir()
            alpha.mkdir()
            beta.mkdir()
            (workspace / "notes.txt").write_text("ignore", encoding="utf-8")

            payload = _list_directory_suggestions(str(workspace), limit=8)

        self.assertEqual(payload["query"], str(workspace))
        self.assertEqual(
            [row["value"] for row in payload["suggestions"]],
            [str(alpha.resolve()), str(beta.resolve())],
        )
        self.assertEqual({row["kind"] for row in payload["suggestions"]}, {"directory"})

    def test_list_directory_suggestions_matches_partial_leaf_name(self) -> None:
        with TemporaryDirectory() as td:
            workspace = Path(td) / "workspace"
            frontend = workspace / "frontend"
            docs = workspace / "docs"
            workspace.mkdir()
            frontend.mkdir()
            docs.mkdir()

            payload = _list_directory_suggestions(str(workspace / "fr"), limit=8)

        self.assertEqual([row["value"] for row in payload["suggestions"]], [str(frontend.resolve())])
        self.assertEqual(payload["suggestions"][0]["label"], "frontend")

    def test_codex_config_settings_round_trip_raw_toml(self) -> None:
        with TemporaryDirectory() as td:
            config_path = Path(td) / "config.toml"
            with patch("codoxear.server.CODEX_CONFIG_PATH", config_path):
                missing = _read_codex_config_for_settings()
                self.assertFalse(missing["exists"])
                self.assertEqual(missing["text"], "")
                saved = _write_codex_config_for_settings('model = "gpt-5.4"\n')
                self.assertTrue(saved["exists"])
                self.assertEqual(saved["path"], str(config_path))
                self.assertEqual(saved["text"], 'model = "gpt-5.4"\n')
                self.assertEqual(config_path.read_text(encoding="utf-8"), 'model = "gpt-5.4"\n')

    def test_codex_config_settings_rejects_invalid_toml(self) -> None:
        with TemporaryDirectory() as td:
            config_path = Path(td) / "config.toml"
            with patch("codoxear.server.CODEX_CONFIG_PATH", config_path):
                with self.assertRaises(tomllib.TOMLDecodeError):
                    _write_codex_config_for_settings("model = [\n")
                self.assertFalse(config_path.exists())

    def test_schedule_local_service_restart_spawns_detached_restart_shell(self) -> None:
        with TemporaryDirectory() as td:
            repo_root = Path(td)
            script_path = repo_root / "scripts" / "codoxear-local"
            script_path.parent.mkdir(parents=True, exist_ok=True)
            script_path.write_text("#!/usr/bin/env bash\n", encoding="utf-8")
            script_path.chmod(0o755)

            proc = type("_Proc", (), {"pid": 4321})()
            with patch("codoxear.server.REPO_ROOT", repo_root), patch("codoxear.server.LOCAL_DAEMON_SCRIPT_PATH", script_path), patch(
                "codoxear.server.subprocess.Popen", return_value=proc
            ) as popen_mock, patch("codoxear.server.os.getpid", return_value=9876):
                payload = _schedule_local_service_restart()

        self.assertTrue(payload["scheduled"])
        self.assertEqual(payload["restart_pid"], 4321)
        self.assertEqual(payload["server_pid"], 9876)
        self.assertEqual(payload["script"], str(script_path))
        argv = popen_mock.call_args.args[0]
        self.assertEqual(argv[:2], ["/bin/bash", "-lc"])
        self.assertIn(str(script_path), argv[2])
        self.assertIn("restart", argv[2])
        self.assertEqual(popen_mock.call_args.kwargs["cwd"], repo_root)
        self.assertTrue(popen_mock.call_args.kwargs["start_new_session"])

    def test_schedule_local_service_restart_requires_local_daemon_script(self) -> None:
        with TemporaryDirectory() as td:
            repo_root = Path(td)
            script_path = repo_root / "scripts" / "codoxear-local"
            with patch("codoxear.server.REPO_ROOT", repo_root), patch("codoxear.server.LOCAL_DAEMON_SCRIPT_PATH", script_path):
                with self.assertRaisesRegex(FileNotFoundError, "missing"):
                    _schedule_local_service_restart()

    def test_read_codex_launch_defaults_includes_provider_list_and_service_tier(self) -> None:
        with TemporaryDirectory() as td:
            config_path = Path(td) / "config.toml"
            models_cache_path = Path(td) / "models.json"
            config_path.write_text(
                """
model = "gpt-5.4"
model_provider = "crs"
preferred_auth_method = "apikey"
service_tier = "fast"

[model_providers.crs]
name = "CRS"

[model_providers.right]
name = "Right"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            models_cache_path.write_text(
                '{"models":[{"slug":"gpt-5.4","default_reasoning_level":"medium","priority":1}]}',
                encoding="utf-8",
            )

            with patch("codoxear.server.CODEX_CONFIG_PATH", config_path), patch("codoxear.server.MODELS_CACHE_PATH", models_cache_path), _codex_launch_default_env():
                defaults = _read_codex_launch_defaults()

        self.assertEqual(defaults["model_provider"], "crs")
        self.assertEqual(defaults["preferred_auth_method"], "apikey")
        self.assertEqual(defaults["provider_choice"], "crs")
        self.assertEqual(defaults["model"], "gpt-5.4")
        self.assertEqual(defaults["model_providers"], ["chatgpt", "openai-api", "crs", "right"])
        self.assertEqual(defaults["service_tier"], "fast")
        self.assertEqual(defaults["reasoning_effort"], "medium")

    def test_read_codex_launch_defaults_falls_back_to_openai_and_flex(self) -> None:
        with TemporaryDirectory() as td:
            config_path = Path(td) / "missing-config.toml"
            models_cache_path = Path(td) / "missing-models.json"
            with patch("codoxear.server.CODEX_CONFIG_PATH", config_path), patch("codoxear.server.MODELS_CACHE_PATH", models_cache_path), _codex_launch_default_env():
                defaults = _read_codex_launch_defaults()

        self.assertEqual(defaults["model_provider"], "openai")
        self.assertEqual(defaults["preferred_auth_method"], "apikey")
        self.assertEqual(defaults["provider_choice"], "openai-api")
        self.assertIsNone(defaults["model"])
        self.assertEqual(defaults["model_providers"], ["chatgpt", "openai-api"])
        self.assertEqual(defaults["service_tier"], "flex")
        self.assertIsNone(defaults["reasoning_effort"])

    def test_normalize_requested_model_provider_rejects_unknown_value(self) -> None:
        with self.assertRaisesRegex(ValueError, "model_provider must be one of openai, right"):
            _normalize_requested_model_provider("bytecat", allowed={"openai", "right"})

    def test_normalize_requested_service_tier_rejects_unknown_value(self) -> None:
        with self.assertRaisesRegex(ValueError, "service_tier must be one of fast, flex"):
            _normalize_requested_service_tier("slow")

    def test_normalize_requested_preferred_auth_method_rejects_unknown_value(self) -> None:
        with self.assertRaisesRegex(ValueError, "preferred_auth_method must be one of chatgpt, apikey"):
            _normalize_requested_preferred_auth_method("oauth")

    def test_read_codex_launch_defaults_maps_openai_chatgpt_choice(self) -> None:
        with TemporaryDirectory() as td:
            config_path = Path(td) / "config.toml"
            models_cache_path = Path(td) / "models.json"
            config_path.write_text(
                """
model_provider = "openai"
preferred_auth_method = "chatgpt"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            models_cache_path.write_text('{"models":[]}', encoding="utf-8")

            with patch("codoxear.server.CODEX_CONFIG_PATH", config_path), patch("codoxear.server.MODELS_CACHE_PATH", models_cache_path), _codex_launch_default_env():
                defaults = _read_codex_launch_defaults()

        self.assertEqual(defaults["provider_choice"], "chatgpt")

    def test_read_codex_launch_defaults_collects_provider_names_by_section_key(self) -> None:
        with TemporaryDirectory() as td:
            config_path = Path(td) / "config.toml"
            models_cache_path = Path(td) / "models.json"
            config_path.write_text(
                """
service_tier = "flex"

[model_providers.crs]
name = "CRS"

[model_providers.custom]
base_url = "https://example.com/v1"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            models_cache_path.write_text('{"models":[]}', encoding="utf-8")

            with patch("codoxear.server.CODEX_CONFIG_PATH", config_path), patch("codoxear.server.MODELS_CACHE_PATH", models_cache_path), _codex_launch_default_env():
                defaults = _read_codex_launch_defaults()

        self.assertEqual(defaults["model_providers"], ["chatgpt", "openai-api", "crs", "custom"])

    def test_read_codex_launch_defaults_applies_web_env_overrides(self) -> None:
        with TemporaryDirectory() as td:
            config_path = Path(td) / "config.toml"
            models_cache_path = Path(td) / "models.json"
            config_path.write_text(
                """
model = "gpt-5.4"
model_provider = "yesteam"
preferred_auth_method = "apikey"
service_tier = "flex"

[model_providers.yesteam]
base_url = "https://example.invalid/yesteam"

[model_providers.crs]
base_url = "https://example.invalid/crs"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            models_cache_path.write_text(
                '{"models":[{"slug":"gpt-5.4","default_reasoning_level":"medium","priority":1}]}',
                encoding="utf-8",
            )

            with patch("codoxear.server.CODEX_CONFIG_PATH", config_path), patch("codoxear.server.MODELS_CACHE_PATH", models_cache_path), _codex_launch_default_env(
                CODEX_WEB_DEFAULT_MODEL_PROVIDER="crs",
                CODEX_WEB_DEFAULT_MODEL="gpt-5.5",
                CODEX_WEB_DEFAULT_REASONING_EFFORT="high",
                CODEX_WEB_DEFAULT_SERVICE_TIER="fast",
            ):
                defaults = _read_codex_launch_defaults()

        self.assertEqual(defaults["model_provider"], "crs")
        self.assertEqual(defaults["provider_choice"], "crs")
        self.assertEqual(defaults["model"], "gpt-5.5")
        self.assertEqual(defaults["reasoning_effort"], "high")
        self.assertEqual(defaults["service_tier"], "fast")

    def test_read_codex_launch_defaults_rejects_unknown_web_env_provider(self) -> None:
        with TemporaryDirectory() as td:
            config_path = Path(td) / "config.toml"
            models_cache_path = Path(td) / "models.json"
            config_path.write_text(
                """
[model_providers.crs]
base_url = "https://example.invalid/crs"
""".strip()
                + "\n",
                encoding="utf-8",
            )
            models_cache_path.write_text('{"models":[]}', encoding="utf-8")

            with patch("codoxear.server.CODEX_CONFIG_PATH", config_path), patch("codoxear.server.MODELS_CACHE_PATH", models_cache_path), _codex_launch_default_env(
                CODEX_WEB_DEFAULT_MODEL_PROVIDER="missing"
            ):
                with self.assertRaisesRegex(ValueError, "model_provider must be one of crs, openai"):
                    _read_codex_launch_defaults()

    def test_read_pi_launch_defaults_reads_provider_model_and_thinking(self) -> None:
        with TemporaryDirectory() as td:
            settings_path = Path(td) / "settings.json"
            models_path = Path(td) / "models.json"
            auth_path = Path(td) / "missing-auth.json"
            settings_path.write_text(
                """
{
  "defaultProvider": "macaron",
  "defaultModel": "gpt-5.4",
  "defaultThinkingLevel": "medium"
}
""".strip()
                + "\n",
                encoding="utf-8",
            )
            models_path.write_text(
                """
{
  "providers": {
    "macaron": {
      "models": [
        {"id": "gpt-5.4"},
        {"id": "gpt-5.4-mini"}
      ]
    }
  }
}
""".strip()
                + "\n",
                encoding="utf-8",
            )
            with patch("codoxear.server.PI_SETTINGS_PATH", settings_path), patch("codoxear.server.PI_MODELS_PATH", models_path), patch(
                "codoxear.server.PI_AUTH_PATH", auth_path
            ):
                defaults = _read_pi_launch_defaults()

        self.assertEqual(defaults["provider_choice"], "macaron")
        self.assertEqual(defaults["model"], "gpt-5.4")
        self.assertEqual(defaults["reasoning_effort"], "high")
        self.assertEqual(
            defaults["provider_choices"],
            ["macaron", "anthropic", "openai-codex", "github-copilot", "google-gemini-cli", "google-antigravity"],
        )
        self.assertEqual(defaults["models"], ["gpt-5.4", "gpt-5.4-mini"])
        self.assertFalse(defaults["supports_fast"])

    def test_read_new_session_defaults_includes_both_backends(self) -> None:
        with TemporaryDirectory() as td:
            settings_path = Path(td) / "settings.json"
            models_path = Path(td) / "models.json"
            settings_path.write_text('{"defaultProvider":"macaron","defaultModel":"gpt-5.4","defaultThinkingLevel":"medium"}\n', encoding="utf-8")
            models_path.write_text('{"providers":{"macaron":{"models":[{"id":"gpt-5.4"}]}}}\n', encoding="utf-8")
            with patch("codoxear.server.PI_SETTINGS_PATH", settings_path), patch("codoxear.server.PI_MODELS_PATH", models_path):
                defaults = _read_new_session_defaults()

        self.assertEqual(defaults["default_backend"], "codex")
        self.assertIn("codex", defaults["backends"])
        self.assertIn("pi", defaults["backends"])
        self.assertEqual(defaults["backends"]["pi"]["provider_choice"], "macaron")

    def test_read_pi_launch_defaults_includes_logged_in_oauth_providers(self) -> None:
        with TemporaryDirectory() as td:
            settings_path = Path(td) / "settings.json"
            models_path = Path(td) / "models.json"
            auth_path = Path(td) / "auth.json"
            settings_path.write_text('{"defaultProvider":"macaron","defaultModel":"gpt-5.4"}\n', encoding="utf-8")
            models_path.write_text('{"providers":{"macaron":{"models":[{"id":"gpt-5.4"}]}}}\n', encoding="utf-8")
            auth_path.write_text(
                '{"openai-codex":{"type":"oauth","access":"abc","refresh":"def"},"ignore-me":{"type":"apikey"}}\n',
                encoding="utf-8",
            )
            with patch("codoxear.server.PI_SETTINGS_PATH", settings_path), patch("codoxear.server.PI_MODELS_PATH", models_path), patch(
                "codoxear.server.PI_AUTH_PATH", auth_path
            ):
                defaults = _read_pi_launch_defaults()

        self.assertEqual(
            defaults["provider_choices"],
            ["macaron", "anthropic", "openai-codex", "github-copilot", "google-gemini-cli", "google-antigravity"],
        )


if __name__ == "__main__":
    unittest.main()
