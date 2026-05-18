from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_sidebar_uses_selected_session_runtime_state_from_live_polling():
    source = (ROOT / "frontend/src/app.tsx").read_text()

    assert "function sessionWithRuntimeState(session: SessionSummary" in source
    assert "const cached = sessionViewCacheRef.current[session.session_id];" in source
    assert "busy: cached.busy" in source
    assert "queue_len: cached.queueLen" in source
    assert "setSessions((current) =>" in source
    assert "session.session_id === sessionId ? { ...session, busy: nextBusy, queue_len: nextQueueLen } : session" in source
    assert "const runtimeSession = sessionWithRuntimeState(session);" in source
    assert "group.sessions.push(runtimeSession);" in source
    assert "sessionIsRunning(runtimeSession" in source


def test_working_status_takes_priority_over_queue_text():
    source = (ROOT / "frontend/src/app.tsx").read_text()
    body = source[source.index("function sessionStatusText") : source.index("function sessionMarkerState")]

    assert body.index("if (sessionIsStarting(session)) return \"starting\";") < body.index("if (sessionIsRunning(session")
    assert body.index("if (sessionIsRunning(session") < body.index("if (session.queue_len)")
