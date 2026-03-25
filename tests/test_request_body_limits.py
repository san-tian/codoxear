from __future__ import annotations

import io
import unittest

from codoxear.server import MESSAGE_BODY_MAX_BYTES
from codoxear.server import PayloadTooLargeError
from codoxear.server import _api_limits_payload
from codoxear.server import _read_body


class _DummyHandler:
    def __init__(self, body: bytes, *, content_length: int | None = None) -> None:
        self.headers = {}
        if content_length is not None:
            self.headers["Content-Length"] = str(content_length)
        self.rfile = io.BytesIO(body)


class TestRequestBodyLimits(unittest.TestCase):
    def test_read_body_accepts_payload_within_limit(self) -> None:
        handler = _DummyHandler(b"hello", content_length=5)
        self.assertEqual(_read_body(handler, limit=8), b"hello")

    def test_read_body_rejects_payload_over_limit(self) -> None:
        handler = _DummyHandler(b"", content_length=12)
        with self.assertRaises(PayloadTooLargeError) as ctx:
            _read_body(handler, limit=8)
        self.assertEqual(ctx.exception.actual, 12)
        self.assertEqual(ctx.exception.limit, 8)
        self.assertIn("max 8 bytes", str(ctx.exception))

    def test_api_limits_payload_exposes_message_body_limit(self) -> None:
        payload = _api_limits_payload()
        self.assertEqual(payload["message_body_max_bytes"], MESSAGE_BODY_MAX_BYTES)


if __name__ == "__main__":
    unittest.main()
