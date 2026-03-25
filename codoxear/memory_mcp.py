from __future__ import annotations

import argparse
import json
import os
import re
import sys
import traceback
from pathlib import Path
from typing import Any

from .memory_index import default_config
from .memory_index import ensure_index_current
from .memory_index import read_note
from .memory_index import resolve_project_root
from .memory_index import search_index


PROTOCOL_VERSION = "2025-03-26"
SERVER_NAME = "workspace-memory"
SERVER_VERSION = "0.1.0"
PATH_HINT_RE = re.compile(r"(?P<path>(?:~|/|\.\.?/)[^\s\"'`:,;]+)")


def _tool_definitions() -> list[dict[str, Any]]:
    return [
        {
            "name": "memory_search",
            "description": "Semantic search over local project .memory/docs and AGENTS guidance indexed under .memory/. If the query mentions a target project path, the server will try to infer the project root automatically.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "search query"},
                    "top_k": {"type": "integer", "minimum": 1, "maximum": 20, "description": "maximum matches to return"},
                    "kind": {"type": "string", "description": "optional note kind filter such as feature, flow, record, guide"},
                    "path_prefix": {"type": "string", "description": "optional source file path prefix filter"},
                    "root": {"type": "string", "description": "optional explicit project root"},
                    "cwd": {"type": "string", "description": "optional working directory used to auto-detect project root"},
                    "project_path": {"type": "string", "description": "optional target project path when searching memory for a different project than the current working directory"},
                },
                "required": ["query"],
                "additionalProperties": False,
            },
        },
        {
            "name": "memory_read",
            "description": "Read a full note by note_id (usually the repo-relative markdown path). Use `project_path`, `cwd`, or `root` when reading memory for a different project than the current working directory.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "note_id": {"type": "string", "description": "repo-relative markdown path from kb_search results"},
                    "root": {"type": "string", "description": "optional explicit project root"},
                    "cwd": {"type": "string", "description": "optional working directory used to auto-detect project root"},
                    "project_path": {"type": "string", "description": "optional target project path when reading memory for a different project than the current working directory"},
                },
                "required": ["note_id"],
                "additionalProperties": False,
            },
        },
        {
            "name": "memory_refresh",
            "description": "Rebuild the local .memory index from .memory/docs/ and AGENTS.md. Use `project_path`, `cwd`, or `root` when refreshing memory for a different project than the current working directory.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "root": {"type": "string", "description": "optional explicit project root"},
                    "cwd": {"type": "string", "description": "optional working directory used to auto-detect project root"},
                    "project_path": {"type": "string", "description": "optional target project path when refreshing memory for a different project than the current working directory"},
                    "force": {"type": "boolean", "description": "force a full rebuild even if the index appears current"},
                },
                "additionalProperties": False,
            },
        },
    ]


class MemoryMcpServer:
    def __init__(self, *, repo_root: Path | None) -> None:
        self.repo_root = repo_root.resolve() if isinstance(repo_root, Path) else None

    def handle_request(self, msg: dict[str, Any]) -> dict[str, Any] | None:
        method = msg.get("method")
        req_id = msg.get("id")
        if not isinstance(method, str):
            return self._error(req_id, -32600, "missing method")

        if method == "initialize":
            return {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {
                        "tools": {
                            "listChanged": True,
                        }
                    },
                    "serverInfo": {
                        "name": SERVER_NAME,
                        "title": "Codoxear Memory",
                        "version": SERVER_VERSION,
                    },
                },
            }
        if method in ("notifications/initialized", "initialized"):
            return None
        if method == "ping":
            return {"jsonrpc": "2.0", "id": req_id, "result": {}}
        if method == "tools/list":
            return {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "tools": _tool_definitions(),
                },
            }
        if method == "tools/call":
            params = msg.get("params")
            if not isinstance(params, dict):
                return self._error(req_id, -32602, "tools/call requires params")
            name = params.get("name")
            args = params.get("arguments") or {}
            if not isinstance(name, str):
                return self._error(req_id, -32602, "tools/call requires tool name")
            if not isinstance(args, dict):
                return self._error(req_id, -32602, "tool arguments must be an object")
            try:
                result = self._call_tool(name, args)
            except Exception as exc:
                return {
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "content": [
                            {"type": "text", "text": f"{type(exc).__name__}: {exc}"},
                        ],
                        "structuredContent": {
                            "error": {
                                "type": type(exc).__name__,
                                "message": str(exc),
                            }
                        },
                        "isError": True,
                    },
                }
            return {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "content": [
                        {"type": "text", "text": result["text"]},
                    ],
                    "structuredContent": result["structured"],
                    "isError": False,
                },
            }
        return self._error(req_id, -32601, f"method not found: {method}")

    def _call_tool(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        config = self._config_for_arguments(name, arguments)
        if name in ("memory_refresh", "kb_refresh"):
            refresh = ensure_index_current(config, force=bool(arguments.get("force")))
            return {
                "text": (
                    f"Rebuilt .memory index from {refresh.summary['source_count']} source files into {refresh.summary['chunk_count']} chunks using provider={refresh.summary['provider']}"
                    if refresh.refreshed
                    else f".memory index is already up to date ({refresh.summary['source_count']} source files, {refresh.summary['chunk_count']} chunks)"
                ),
                "structured": {
                    "project_root": str(config.repo_root),
                    "refreshed": refresh.refreshed,
                    "reason": refresh.reason,
                    **refresh.summary,
                },
            }
        if name in ("memory_search", "kb_search"):
            refresh = ensure_index_current(config, force=False)
            query = str(arguments.get("query") or "").strip()
            if not query:
                raise ValueError("query is required")
            top_k_raw = arguments.get("top_k", config.top_k_default)
            top_k = max(1, min(int(top_k_raw), 20))
            matches = search_index(
                config,
                query=query,
                top_k=top_k,
                kind=_str_or_none(arguments.get("kind")),
                path_prefix=_str_or_none(arguments.get("path_prefix")),
            )
            text_lines = [f"Found {len(matches)} matches for: {query}"]
            for idx, match in enumerate(matches, 1):
                text_lines.append(
                    f"{idx}. [{match.kind}] {match.note_id} :: {match.title} (score={match.score:.3f})\n   {match.snippet}"
                )
            return {
                "text": "\n".join(text_lines),
                "structured": {
                    "project_root": str(config.repo_root),
                    "refreshed": refresh.refreshed,
                    "refresh_reason": refresh.reason,
                    "query": query,
                    "matches": [match.__dict__ for match in matches],
                },
            }
        if name in ("memory_read", "kb_read"):
            note_id = str(arguments.get("note_id") or "").strip()
            if not note_id:
                raise ValueError("note_id is required")
            note = read_note(config, note_id)
            return {
                "text": note["content"],
                "structured": {"project_root": str(config.repo_root), **note},
            }
        raise ValueError(f"unknown tool: {name}")

    def _config_for_arguments(self, tool_name: str, arguments: dict[str, Any]):
        root_raw = _str_or_none(arguments.get("root"))
        cwd_raw = _str_or_none(arguments.get("cwd"))
        project_path_raw = _str_or_none(arguments.get("project_path"))
        if root_raw:
            return default_config(repo_root=Path(root_raw))
        if cwd_raw:
            return default_config(repo_root=resolve_project_root(start=Path(cwd_raw)))
        if project_path_raw:
            return default_config(repo_root=resolve_project_root(start=self._resolve_path_hint(project_path_raw)))
        if tool_name in ("memory_search", "kb_search"):
            inferred = self._infer_project_root_from_text(_str_or_none(arguments.get("query")) or "")
            if inferred is not None:
                return default_config(repo_root=inferred)
        if tool_name in ("memory_read", "kb_read"):
            note_hint = _str_or_none(arguments.get("note_id"))
            if note_hint and note_hint.startswith("/"):
                return default_config(repo_root=resolve_project_root(start=Path(note_hint)))
        if self.repo_root is not None:
            return default_config(repo_root=self.repo_root)
        return default_config(repo_root=resolve_project_root())

    def _infer_project_root_from_text(self, text: str) -> Path | None:
        if not text:
            return None
        for match in PATH_HINT_RE.finditer(text):
            candidate = match.group("path")
            try:
                resolved = self._resolve_path_hint(candidate)
            except Exception:
                continue
            if resolved.exists():
                return resolve_project_root(start=resolved)
        return None

    def _resolve_path_hint(self, raw: str) -> Path:
        text = raw.strip()
        path = Path(text).expanduser()
        if path.is_absolute():
            return path.resolve()
        base = Path(os.environ.get("CODOXEAR_MEMORY_CALLER_CWD", "") or os.getcwd()).resolve()
        return (base / path).resolve()

    def _error(self, req_id: Any, code: int, message: str) -> dict[str, Any]:
        return {
            "jsonrpc": "2.0",
            "id": req_id,
            "error": {
                "code": code,
                "message": message,
            },
        }


def _str_or_none(raw: Any) -> str | None:
    if raw is None:
        return None
    text = str(raw).strip()
    return text or None


def _write_message(obj: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Expose Codoxear local .memory/docs + AGENTS as an MCP searchable memory index")
    parser.add_argument("--root", default="", help="optional fixed repo root; omit for shared auto-detection mode")
    args = parser.parse_args(argv)

    server = MemoryMcpServer(repo_root=(Path(args.root).resolve() if str(args.root).strip() else None))
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except Exception:
            _write_message({
                "jsonrpc": "2.0",
                "id": None,
                "error": {"code": -32700, "message": "parse error"},
            })
            continue
        try:
            response = server.handle_request(msg)
        except Exception as exc:
            traceback.print_exc(file=sys.stderr)
            _write_message({
                "jsonrpc": "2.0",
                "id": msg.get("id"),
                "error": {"code": -32603, "message": f"internal error: {exc}"},
            })
            continue
        if response is not None:
            _write_message(response)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
