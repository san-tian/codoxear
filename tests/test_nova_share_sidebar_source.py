from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_share_sidebar_prefers_workspace_cwd_over_real_cwd():
    source = (ROOT / "frontend/src/share.tsx").read_text()
    assert "{item.workspace_cwd || item.cwd || item.session_id}" in source
    assert "{item.cwd || item.workspace_cwd || item.session_id}" not in source
