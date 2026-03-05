import unittest

from codoxear.server import _sanitize_claude_tail_text


class TestTailSanitize(unittest.TestCase):
    def test_claude_tail_filters_spinner_noise(self) -> None:
        text = "\n".join(
            [
                "Flowing…2",
                "40",
                "*1",
                "esctointerrupt",
                "? for shortcuts",
                "● useful line",
                "❯ ",
                "/root/code/codoxear",
            ]
        )
        out = _sanitize_claude_tail_text(text)
        self.assertIn("● useful line", out)
        self.assertIn("/root/code/codoxear", out)
        self.assertIn("❯", out)
        self.assertNotIn("Flowing", out)
        self.assertNotIn("40", out)
        self.assertNotIn("*1", out)
        self.assertNotIn("esctointerrupt", out)
        self.assertNotIn("? for shortcuts", out)

    def test_claude_tail_keeps_cjk_content_and_drops_short_shards(self) -> None:
        text = "\n".join(
            [
                "F3",
                "li4",
                "●找到了关键代码",
                "1. 定义了三个选项",
                "✶9",
            ]
        )
        out = _sanitize_claude_tail_text(text)
        self.assertIn("●找到了关键代码", out)
        self.assertIn("1. 定义了三个选项", out)
        self.assertNotIn("F3", out)
        self.assertNotIn("li4", out)
        self.assertNotIn("✶9", out)


if __name__ == "__main__":
    unittest.main()
