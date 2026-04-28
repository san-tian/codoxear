import unittest
from pathlib import Path


SERVER_PY = Path(__file__).resolve().parents[1] / "codoxear" / "server.py"
RUNTIME_RS = Path(__file__).resolve().parents[1] / "backend-rs" / "src" / "runtime.rs"


class TestMessageRouteSource(unittest.TestCase):
    def test_server_has_no_legacy_messages_route(self) -> None:
        source = SERVER_PY.read_text(encoding="utf-8")
        self.assertNotIn('path.endswith("/messages")', source)
        self.assertNotIn('"offset": int(new_off)', source)
        self.assertNotIn('"next_before": int(next_before)', source)

    def test_tail_live_history_routes_emit_cursor_fields_from_rust(self) -> None:
        source = RUNTIME_RS.read_text(encoding="utf-8")
        self.assertIn("live_cursor: Some(live_cursor)", source)
        self.assertIn("history_cursor: selected.first().map(|record| record.start.to_string())", source)
        self.assertIn("load_messages_history", source)
        self.assertIn("load_messages_live", source)


if __name__ == "__main__":
    unittest.main()
