from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_floating_todo_renders_all_items_without_more_placeholder():
    app_source = (ROOT / "frontend/src/app.tsx").read_text()
    styles_source = (ROOT / "frontend/src/styles.css").read_text()

    assert "items.map((item, index)" in app_source
    assert "items.slice(0, 4)" not in app_source
    assert "floatingProgressMore" not in app_source
    assert ".floatingProgressMore" not in styles_source
