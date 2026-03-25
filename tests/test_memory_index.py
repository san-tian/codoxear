from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from codoxear.memory_index import build_index
from codoxear.memory_index import default_config
from codoxear.memory_index import ensure_index_current
from codoxear.memory_index import HashEmbeddingProvider
from codoxear.memory_index import make_provider
from codoxear.memory_index import search_index
from codoxear.memory_index import split_markdown_sections
from codoxear.memory_mcp import MemoryMcpServer


class TestMemoryIndex(unittest.TestCase):
    def test_split_markdown_sections_uses_heading_paths(self) -> None:
        sections = split_markdown_sections(
            "# Top\nintro\n\n## Child\nbody\n\n## Child Two\nbody two\n"
        )
        self.assertEqual(sections[0][0], ["Top"])
        self.assertEqual(sections[1][0], ["Top", "Child"])
        self.assertEqual(sections[2][0], ["Top", "Child Two"])

    def test_build_and_search_index_with_hash_provider(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / ".memory" / "docs" / "features").mkdir(parents=True, exist_ok=True)
            (root / "AGENTS.md").write_text(
                "# Local Agent Guide\n\nUse the busy-state runbook first.\n",
                encoding="utf-8",
            )
            (root / ".memory" / "docs" / "features" / "broker.md").write_text(
                "# Broker Busy\n\n## Codex Turn Markers\nCodex busy should rely on task_started and task_complete.\n",
                encoding="utf-8",
            )
            with patch.dict(os.environ, {"CODOXEAR_MEMORY_EMBED_PROVIDER": "hash"}, clear=False):
                config = default_config(repo_root=root)
                summary = build_index(config)
                self.assertEqual(summary["provider"], "hash")
                self.assertEqual(summary["source_count"], 2)
                matches = search_index(config, query="codex busy task_complete", top_k=3)
            self.assertGreaterEqual(len(matches), 1)
            self.assertEqual(matches[0].note_id, ".memory/docs/features/broker.md")
            self.assertIn("Codex Turn Markers", matches[0].title)

    def test_mcp_server_handles_initialize_and_tool_calls(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / ".memory" / "docs" / "flows").mkdir(parents=True, exist_ok=True)
            (root / "AGENTS.md").write_text("# Agent\n\nRead docs.\n", encoding="utf-8")
            (root / ".memory" / "docs" / "flows" / "testing.md").write_text(
                "# Testing\n\n## Busy\nUse task_complete to mark Codex idle.\n",
                encoding="utf-8",
            )
            with patch.dict(os.environ, {"CODOXEAR_MEMORY_EMBED_PROVIDER": "hash"}, clear=False):
                server = MemoryMcpServer(repo_root=root)
                init_resp = server.handle_request(
                    {
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "initialize",
                        "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}},
                    }
                )
                assert init_resp is not None
                self.assertEqual(init_resp["result"]["protocolVersion"], "2025-03-26")

                list_resp = server.handle_request(
                    {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}
                )
                assert list_resp is not None
                names = [tool["name"] for tool in list_resp["result"]["tools"]]
                self.assertEqual(names, ["memory_search", "memory_read", "memory_refresh"])

                refresh_resp = server.handle_request(
                    {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "memory_refresh", "arguments": {}}}
                )
                assert refresh_resp is not None
                self.assertFalse(refresh_resp["result"]["isError"])
                self.assertTrue(refresh_resp["result"]["structuredContent"]["refreshed"])

                refresh_again_resp = server.handle_request(
                    {"jsonrpc": "2.0", "id": 31, "method": "tools/call", "params": {"name": "memory_refresh", "arguments": {}}}
                )
                assert refresh_again_resp is not None
                self.assertFalse(refresh_again_resp["result"]["structuredContent"]["refreshed"])

                search_resp = server.handle_request(
                    {
                        "jsonrpc": "2.0",
                        "id": 4,
                        "method": "tools/call",
                        "params": {"name": "memory_search", "arguments": {"query": "Codex idle task_complete", "top_k": 2}},
                    }
                )
                assert search_resp is not None
                self.assertFalse(search_resp["result"]["isError"])
                matches = search_resp["result"]["structuredContent"]["matches"]
                self.assertGreaterEqual(len(matches), 1)
                self.assertFalse(search_resp["result"]["structuredContent"]["refreshed"])

                read_resp = server.handle_request(
                    {
                        "jsonrpc": "2.0",
                        "id": 5,
                        "method": "tools/call",
                        "params": {"name": "memory_read", "arguments": {"note_id": ".memory/docs/flows/testing.md"}},
                    }
                )
                assert read_resp is not None
                self.assertIn("Use task_complete", read_resp["result"]["content"][0]["text"])

    def test_memory_openai_envs_override_global_openai_envs(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            with patch.dict(
                os.environ,
                {
                    "CODOXEAR_MEMORY_EMBED_PROVIDER": "openai",
                    "CODOXEAR_MEMORY_OPENAI_API_KEY": "memory-key",
                    "CODOXEAR_MEMORY_OPENAI_BASE_URL": "https://memory.example/v1",
                    "OPENAI_API_KEY": "global-key",
                    "OPENAI_BASE_URL": "https://global.example/v1",
                },
                clear=False,
            ):
                config = default_config(repo_root=root)
                provider = make_provider(config)
            self.assertEqual(config.openai_base_url, "https://memory.example/v1")
            self.assertEqual(provider.api_key, "memory-key")
            self.assertEqual(provider.base_url, "https://memory.example/v1")

    def test_shared_mcp_server_resolves_project_from_cwd_argument(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            project = root / 'proj-a'
            (project / '.memory' / 'docs' / 'features').mkdir(parents=True, exist_ok=True)
            (project / 'AGENTS.md').write_text('# Agent\n', encoding='utf-8')
            (project / '.memory' / 'docs' / 'features' / 'broker.md').write_text('# Broker\n\nTask complete note\n', encoding='utf-8')
            with patch.dict(os.environ, {'CODOXEAR_MEMORY_EMBED_PROVIDER': 'hash'}, clear=False):
                server = MemoryMcpServer(repo_root=None)
                refresh_resp = server.handle_request(
                    {
                        'jsonrpc': '2.0',
                        'id': 1,
                        'method': 'tools/call',
                        'params': {'name': 'memory_refresh', 'arguments': {'cwd': str(project / 'subdir')}} ,
                    }
                )
                assert refresh_resp is not None
                self.assertFalse(refresh_resp['result']['isError'])
                self.assertEqual(refresh_resp['result']['structuredContent']['project_root'], str(project))
                search_resp = server.handle_request(
                    {
                        'jsonrpc': '2.0',
                        'id': 2,
                        'method': 'tools/call',
                        'params': {'name': 'memory_search', 'arguments': {'cwd': str(project), 'query': 'task complete', 'top_k': 1}},
                    }
                )
                assert search_resp is not None
                self.assertFalse(search_resp['result']['isError'])
                self.assertEqual(search_resp['result']['structuredContent']['project_root'], str(project))

    def test_shared_mcp_server_resolves_project_from_project_path_argument(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            project = root / 'proj-b'
            (project / '.memory' / 'docs' / 'features').mkdir(parents=True, exist_ok=True)
            (project / 'AGENTS.md').write_text('# Agent\n', encoding='utf-8')
            (project / '.memory' / 'docs' / 'features' / 'broker.md').write_text('# Broker\n\nRefresh target\n', encoding='utf-8')
            with patch.dict(os.environ, {'CODOXEAR_MEMORY_EMBED_PROVIDER': 'hash'}, clear=False):
                server = MemoryMcpServer(repo_root=None)
                refresh_resp = server.handle_request(
                    {
                        'jsonrpc': '2.0',
                        'id': 1,
                        'method': 'tools/call',
                        'params': {'name': 'memory_refresh', 'arguments': {'project_path': str(project), 'force': False}},
                    }
                )
                assert refresh_resp is not None
                self.assertFalse(refresh_resp['result']['isError'])
                self.assertEqual(refresh_resp['result']['structuredContent']['project_root'], str(project))

    def test_shared_mcp_server_infers_project_root_from_query_path(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            project = root / 'proj-c'
            (project / '.memory' / 'docs' / 'features').mkdir(parents=True, exist_ok=True)
            (project / 'AGENTS.md').write_text('# Agent\n', encoding='utf-8')
            (project / '.memory' / 'docs' / 'features' / 'bootstrap.md').write_text('# Bootstrap\n\nProject init note\n', encoding='utf-8')
            with patch.dict(os.environ, {'CODOXEAR_MEMORY_EMBED_PROVIDER': 'hash'}, clear=False):
                server = MemoryMcpServer(repo_root=None)
                search_resp = server.handle_request(
                    {
                        'jsonrpc': '2.0',
                        'id': 2,
                        'method': 'tools/call',
                        'params': {
                            'name': 'memory_search',
                            'arguments': {
                                'query': f'bootstrap memory workflow for {project}',
                                'top_k': 1,
                            },
                        },
                    }
                )
                assert search_resp is not None
                self.assertFalse(search_resp['result']['isError'])
                self.assertEqual(search_resp['result']['structuredContent']['project_root'], str(project))

    def test_ensure_index_current_skips_when_sources_are_unchanged(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / ".memory" / "docs" / "features").mkdir(parents=True, exist_ok=True)
            (root / "AGENTS.md").write_text("# Agent\n", encoding="utf-8")
            (root / ".memory" / "docs" / "features" / "broker.md").write_text("# Broker\n", encoding="utf-8")
            with patch.dict(os.environ, {"CODOXEAR_MEMORY_EMBED_PROVIDER": "hash"}, clear=False):
                config = default_config(repo_root=root)
                first = ensure_index_current(config, force=False)
                second = ensure_index_current(config, force=False)
            self.assertTrue(first.refreshed)
            self.assertFalse(second.refreshed)
            self.assertEqual(second.reason, "up to date")

    def test_ensure_index_current_rebuilds_after_source_change(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            note = root / ".memory" / "docs" / "features" / "broker.md"
            note.parent.mkdir(parents=True, exist_ok=True)
            (root / "AGENTS.md").write_text("# Agent\n", encoding="utf-8")
            note.write_text("# Broker\n", encoding="utf-8")
            with patch.dict(os.environ, {"CODOXEAR_MEMORY_EMBED_PROVIDER": "hash"}, clear=False):
                config = default_config(repo_root=root)
                first = ensure_index_current(config, force=False)
                note.write_text("# Broker\n\nChanged\n", encoding="utf-8")
                second = ensure_index_current(config, force=False)
            self.assertTrue(first.refreshed)
            self.assertTrue(second.refreshed)
            self.assertEqual(second.reason, "source fingerprint changed")

    def test_incremental_rebuild_only_embeds_changed_file_chunks(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            f1 = root / ".memory" / "docs" / "features" / "one.md"
            f2 = root / ".memory" / "docs" / "features" / "two.md"
            f1.parent.mkdir(parents=True, exist_ok=True)
            (root / "AGENTS.md").write_text("# Agent\n", encoding="utf-8")
            f1.write_text("# One\n\nAlpha\n", encoding="utf-8")
            f2.write_text("# Two\n\nBeta\n", encoding="utf-8")
            with patch.dict(os.environ, {"CODOXEAR_MEMORY_EMBED_PROVIDER": "hash"}, clear=False):
                config = default_config(repo_root=root)
                first = ensure_index_current(config, force=False)
                self.assertTrue(first.refreshed)
                f2.write_text("# Two\n\nBeta changed\n", encoding="utf-8")

                embed_call_sizes: list[int] = []
                original = HashEmbeddingProvider.embed_texts

                def wrapped(provider_self, texts):
                    embed_call_sizes.append(len(texts))
                    return original(provider_self, texts)

                with patch("codoxear.memory_index.HashEmbeddingProvider.embed_texts", new=wrapped):
                    second = ensure_index_current(config, force=False)

            self.assertTrue(second.refreshed)
            self.assertEqual(embed_call_sizes, [1])


if __name__ == "__main__":
    unittest.main()
