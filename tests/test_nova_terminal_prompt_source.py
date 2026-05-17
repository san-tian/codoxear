from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_goal_replace_terminal_prompt_has_owner_ui_and_api():
    app_source = (ROOT / "frontend/src/app.tsx").read_text()
    api_source = (ROOT / "frontend/src/lib/api.ts").read_text()
    types_source = (ROOT / "frontend/src/lib/types.ts").read_text()

    assert "TerminalPromptState" in app_source
    assert "terminalPrompt" in app_source
    assert "handleTerminalPromptResponse" in app_source
    assert "terminalPromptPanel" in app_source
    assert "Goal replacement confirmed" in app_source
    assert "Goal replacement declined" in app_source
    assert "sendTerminalResponse" in api_source
    assert "/terminal_response" in api_source
    assert "export type TerminalPrompt" in types_source


def test_goal_replace_terminal_prompt_has_share_ui_and_api():
    app_source = (ROOT / "frontend/src/app.tsx").read_text()
    share_source = (ROOT / "frontend/src/share.tsx").read_text()
    api_source = (ROOT / "frontend/src/lib/api.ts").read_text()
    routes_source = (ROOT / "backend-rs/src/routes.rs").read_text()

    assert "shareTerminalPrompt" in app_source
    assert "handleShareTerminalPromptResponse" in app_source
    assert "sendShareTerminalResponse" in api_source
    assert "terminalPromptPanel" in share_source
    assert "onTerminalPromptResponse" in share_source
    assert "/share/:share_id/sessions/:session_id/terminal_response" in routes_source
    assert "share_session_terminal_response" in routes_source


def test_goal_replace_terminal_prompt_is_backend_driven_from_broker_tail():
    runtime_source = (ROOT / "backend-rs/src/runtime.rs").read_text()
    broker_source = (ROOT / "backend-rs/src/broker.rs").read_text()
    routes_source = (ROOT / "backend-rs/src/routes.rs").read_text()

    assert "TERMINAL_PROMPT_REPLACE_GOAL" in runtime_source
    assert "terminal_prompt_from_tail" in runtime_source
    assert 'choice.key_seq' in runtime_source
    assert '"tail": state.output_tail' in broker_source
    assert "session_terminal_response" in routes_source
