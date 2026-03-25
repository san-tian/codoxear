from __future__ import annotations

import subprocess
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

from codoxear.server import Session
from codoxear.server import SessionManager
from codoxear.server import _git_branch_for_cwd


def _make_session(*, sid: str, cwd: str) -> Session:
    p = Path("/tmp") / f"{sid}.jsonl"
    return Session(
        session_id=sid,
        thread_id=f"thread-{sid}",
        broker_pid=1,
        codex_pid=1,
        cli="codex",
        owned=False,
        start_ts=0.0,
        cwd=cwd,
        log_path=None,
        sock_path=p.with_suffix(".sock"),
    )


def _make_manager(*sessions: Session) -> SessionManager:
    mgr = SessionManager.__new__(SessionManager)
    mgr._lock = threading.Lock()
    mgr._sessions = {s.session_id: s for s in sessions}
    mgr._harness = {}
    mgr._aliases = {}
    mgr._files = {}
    mgr._stop = None
    mgr._last_discover_ts = 0.0
    mgr._harness_last_injected = {}
    mgr._harness_last_injected_scope = {}
    mgr._discover_existing_if_stale = lambda: None  # type: ignore[assignment]
    mgr._prune_dead_sessions = lambda: None  # type: ignore[assignment]
    mgr._update_meta_counters = lambda: None  # type: ignore[assignment]
    mgr._save_files = lambda: None  # type: ignore[assignment]
    return mgr


class TestGitBranchDisplay(unittest.TestCase):
    def test_git_branch_for_cwd_reads_branch_name(self) -> None:
        completed = subprocess.CompletedProcess(
            args=["git", "rev-parse", "--abbrev-ref", "HEAD"],
            returncode=0,
            stdout="feature/nav-branch\n",
            stderr="",
        )
        with patch("codoxear.server.subprocess.run", return_value=completed):
            self.assertEqual(_git_branch_for_cwd("/tmp/project"), "feature/nav-branch")

    def test_git_branch_for_cwd_returns_empty_outside_repo(self) -> None:
        completed = subprocess.CompletedProcess(
            args=["git", "rev-parse", "--abbrev-ref", "HEAD"],
            returncode=128,
            stdout="",
            stderr="fatal: not a git repository",
        )
        with patch("codoxear.server.subprocess.run", return_value=completed):
            self.assertEqual(_git_branch_for_cwd("/tmp/not-a-repo"), "")

    def test_list_sessions_includes_git_branch_once_per_workspace(self) -> None:
        mgr = _make_manager(
            _make_session(sid="a", cwd="/tmp/work"),
            _make_session(sid="b", cwd="/tmp/work"),
            _make_session(sid="c", cwd="/tmp/other"),
        )
        with patch("codoxear.server._git_branch_for_cwd", side_effect=["dev", "main"]) as branch_mock:
            sessions = mgr.list_sessions()
        by_id = {s["session_id"]: s for s in sessions}
        self.assertEqual(by_id["a"]["git_branch"], "dev")
        self.assertEqual(by_id["b"]["git_branch"], "dev")
        self.assertEqual(by_id["c"]["git_branch"], "main")
        self.assertEqual(branch_mock.call_count, 2)


if __name__ == "__main__":
    unittest.main()
