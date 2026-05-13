from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[1]


def test_awaiting_reply_closes_on_error_and_abort_events():
    source = (ROOT / "frontend/src/lib/session-status.ts").read_text()
    assert "eventClosesAssistantWait" in source
    assert 'event.kind === "tool_result" && event.toolResultIsError' in source
    assert 'event.kind === "tool" && event.toolResultIsError' in source
    assert '"aborted"' in source
    assert '"cancelled"' in source
    assert "latestCloseTs" in source
    assert "closedTurnTs = 0" in source
    assert "latestUserTs > latestCloseTs" in source


def test_awaiting_reply_helper_behavior():
    subprocess.run(
        ["node", "--experimental-strip-types", "tests/fixtures/session_status_regression.mjs"],
        cwd=ROOT,
        check=True,
    )


def test_app_uses_centralized_awaiting_reply_helper():
    source = (ROOT / "frontend/src/app.tsx").read_text()
    assert 'import { isAwaitingAssistantReply } from "./lib/session-status";' in source
    assert "closedTurnTsBySession" in source
    assert "data.turn_end || data.turn_aborted" in source
    assert "return isAwaitingAssistantReply(transcript, queueLen, closedTurnTsBySession[selectedSessionId] || 0);" in source
    assert "latestAssistantTs" not in source
