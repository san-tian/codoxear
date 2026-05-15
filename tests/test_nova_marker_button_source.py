from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_sidebar_marker_button_toggles_only_normal_and_star():
    source = (ROOT / "frontend/src/app.tsx").read_text()
    helper_start = source.index("function nextSessionMarkerButtonState")
    helper_end = source.index("function sessionMarkerLabel", helper_start)
    helper = source[helper_start:helper_end]

    assert 'return state === "star" ? "normal" : "star";' in helper
    assert '"pending"' not in helper
    assert '"snooze"' not in helper
    assert "nextSessionMarkerButtonState(markerState)" in source
