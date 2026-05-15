from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_unread_badge_is_driven_by_turn_boundaries():
    source = (ROOT / "frontend/src/app.tsx").read_text()
    assert "unreadSessionIds" in source
    assert "previousSessionBusyRef" in source
    assert "function markSessionUnreadAtTurnBoundary" in source
    assert "if (!sessionId || selectedSessionRef.current === sessionId) return;" in source
    assert "if (data.turn_end || data.turn_aborted)" in source
    assert "markSessionUnreadAtTurnBoundary(sessionId);" in source
    assert "hasPreviousBusySnapshot && previousBusyBySession[session.session_id] && !session.busy" in source
    assert "workspaceUnreadMark" in source


def test_unread_badge_has_red_dot_style():
    source = (ROOT / "frontend/src/styles.css").read_text()
    assert ".workspaceUnreadMark" in source
    assert "background: #ef4444;" in source
