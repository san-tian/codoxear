from __future__ import annotations

import base64
import os
import tempfile
import unittest
from pathlib import Path

from codoxear.server import _content_disposition
from codoxear.server import _read_file_for_viewer
from codoxear.server import _resolve_user_file_path


_PNG_1X1 = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/a7sAAAAASUVORK5CYII="
)


class TestServerFiles(unittest.TestCase):
    def test_read_file_for_viewer_text_payload(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "note.txt"
            p.write_text("hello\nworld\n", encoding="utf-8")
            payload = _read_file_for_viewer(p, max_bytes=1024)
        self.assertEqual(payload.get("text"), "hello\nworld\n")
        self.assertEqual(payload.get("mime"), "text/plain; charset=utf-8")
        self.assertFalse(bool(payload.get("is_image")))
        self.assertFalse(bool(payload.get("is_binary")))

    def test_read_file_for_viewer_image_payload(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "pixel.png"
            p.write_bytes(_PNG_1X1)
            payload = _read_file_for_viewer(p, max_bytes=1024)
        self.assertEqual(payload.get("text"), "")
        self.assertEqual(payload.get("mime"), "image/png")
        self.assertTrue(bool(payload.get("is_image")))
        self.assertTrue(bool(payload.get("is_binary")))

    def test_content_disposition_modes(self) -> None:
        p = Path("/tmp/demo image.png")
        inline = _content_disposition(p, download=False)
        attach = _content_disposition(p, download=True)
        self.assertIn("inline;", inline)
        self.assertIn("attachment;", attach)
        self.assertIn('filename="demo_image.png"', inline)
        self.assertIn("filename*=UTF-8''demo%20image.png", inline)

    def test_resolve_user_file_path_relative(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cwd = os.getcwd()
            try:
                os.chdir(td)
                p = _resolve_user_file_path("a/b.txt")
                self.assertEqual(p, (Path(td) / "a" / "b.txt").resolve())
            finally:
                os.chdir(cwd)


if __name__ == "__main__":
    unittest.main()
