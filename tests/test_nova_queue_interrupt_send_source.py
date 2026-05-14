from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_queue_modal_has_interrupt_send_action():
    source = (ROOT / "frontend/src/app.tsx").read_text()
    assert "queueInterruptSendingItemId" in source
    assert "async function interruptSendQueueItem" in source
    assert "await api.interrupt(sessionId);" in source
    assert "await api.sendMessage(sessionId, text);" in source
    assert "await api.deleteQueueItem(sessionId, itemId);" in source
    assert "queueInterruptSendBtn" in source
    assert "Interrupt current work and send this queued message" in source
