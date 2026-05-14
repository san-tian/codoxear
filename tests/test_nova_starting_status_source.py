from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_starting_status_takes_priority_over_queue_and_busy():
    source = (ROOT / "frontend/src/app.tsx").read_text()

    assert source.index('if (sessionIsStarting(session)) return "starting";') < source.index('if (session.queue_len) return `queue ${session.queue_len}`;')
    assert "const selectedSessionStarting = sessionIsStarting(selectedSession);" in source
    assert "const selectedSessionBusy = Boolean(!selectedSessionStarting && (busy || selectedSession?.busy));" in source
    assert "const composerSessionBusy = Boolean(selectedSessionStarting || effectiveAwaitingAssistantReply || effectiveSelectedSessionBusy);" in source

    top_status_body = source[source.index("const topSessionStatus = useMemo(() => {") : source.index("const topSessionStatusClass =")]
    assert top_status_body.index("if (selectedSessionStarting) return \"starting\";") < top_status_body.index('if (sending) return "sending";')
    assert top_status_body.index("if (selectedSessionStarting) return \"starting\";") < top_status_body.index('return "working";')
