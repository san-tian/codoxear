import pathlib
import unittest


APP_JS = pathlib.Path(__file__).resolve().parents[1] / "codoxear" / "static" / "app.js"


class DuplicateButtonSourceTest(unittest.TestCase):
    def test_no_undefined_duplicate_button_binding(self):
        source = APP_JS.read_text(encoding="utf-8")
        self.assertNotIn("duplicateBtn.onclick =", source)
        self.assertIn('"Duplicate session"', source)


if __name__ == "__main__":
    unittest.main()
