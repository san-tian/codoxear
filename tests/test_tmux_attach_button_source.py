import unittest
from pathlib import Path


APP_JS = Path(__file__).resolve().parents[1] / "codoxear" / "static" / "app.js"


class TestTmuxAttachButtonSource(unittest.TestCase):
    def test_topbar_includes_tmux_attach_button(self) -> None:
        source = APP_JS.read_text(encoding="utf-8")
        self.assertIn('id: "tmuxAttachBtn"', source)
        self.assertIn('title: "Copy tmux attach command"', source)
        self.assertIn("tmuxAttachBtn,", source)

    def test_tmux_attach_command_targets_session_and_window(self) -> None:
        source = APP_JS.read_text(encoding="utf-8")
        self.assertIn("function tmuxAttachCommandForSession(s) {", source)
        self.assertIn("tmux attach-session -t ${shellSingleQuote(tmuxSession)}", source)
        self.assertIn("select-window -t ${shellSingleQuote(`${tmuxSession}:${tmuxWindow}`)}", source)
        self.assertIn('setToast("Copied tmux attach command")', source)


if __name__ == "__main__":
    unittest.main()
