from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_markdown_code_blocks_have_copy_button_with_clipboard_fallback():
    source = (ROOT / "frontend/src/lib/markdown.tsx").read_text()
    styles = (ROOT / "frontend/src/styles.css").read_text()

    assert 'data-md-code-copy title="Copy code" aria-label="Copy code"' in source
    assert 'button.closest(".md-code-block")?.querySelector("code")' in source
    assert "copyTextViaSelection(text);" in source
    assert ".markdown-body .md-code-copy-btn" in styles
