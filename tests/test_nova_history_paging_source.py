from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_older_history_loads_until_user_message_boundary():
    source = (ROOT / "frontend/src/app.tsx").read_text()

    assert "const HISTORY_USER_BOUNDARY_MAX_PAGES = 8;" in source
    assert "function historyBatchReachedUserBoundary(events: UiTranscriptEvent[])" in source
    assert 'events.some((event) => event.kind === "user")' in source
    assert "for (let page = 0; cursor && page < HISTORY_USER_BOUNDARY_MAX_PAGES; page += 1)" in source
    assert "if (historyBatchReachedUserBoundary(older) || !nextData.has_older || !nextCursor || nextCursor === cursor) break;" in source
    assert "olderPages.unshift(older);" in source


def test_shared_older_history_uses_same_user_boundary_paging():
    source = (ROOT / "frontend/src/app.tsx").read_text()

    assert "api.fetchShareHistory(shareId, sessionId, cursor, SHARE_OLDER_LIMIT)" in source
    assert "if (historyBatchReachedUserBoundary(older) || !nextHistory.has_older || !nextCursor || nextCursor === cursor) break;" in source
    assert "const shareSessionRef = useRef(shareTarget?.sessionId || \"\");" in source
    assert "if (shareSessionRef.current !== sessionId) return;" in source
