from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_mobile_queue_preview_is_bounded_for_long_messages():
    source = (ROOT / "frontend/src/styles.css").read_text()
    assert "@media (max-width: 860px)" in source
    assert ".queuePreview" in source
    assert "max-height: min(128px, 24dvh);" in source
    assert "overflow: auto;" in source
    assert "overscroll-behavior: contain;" in source
    assert "-webkit-line-clamp: 2;" in source
    assert "-webkit-box-orient: vertical;" in source


def test_small_mobile_queue_preview_uses_tighter_height_limit():
    source = (ROOT / "frontend/src/styles.css").read_text()
    assert "@media (max-width: 520px)" in source
    assert "max-height: min(104px, 22dvh);" in source
