from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_bundled_codoxear_session_skill_is_distributable():
    skill_dir = ROOT / "skills/codoxear-session"
    skill_source = (skill_dir / "SKILL.md").read_text()
    common_source = (skill_dir / "scripts/common.py").read_text()

    assert "name: codoxear-session" in skill_source
    assert "scripts/create_session.py" in skill_source
    assert "scripts/chat.py" in skill_source
    assert "scripts/schedule.py" in skill_source
    assert "DEFAULT_BASE_URL = \"http://127.0.0.1:8743\"" in common_source
    assert "/vePFS-Mindverse" not in skill_source
    assert "/vePFS-Mindverse" not in common_source


def test_bundled_codoxear_session_skill_exposes_schedules():
    skill_dir = ROOT / "skills/codoxear-session"
    schedule_source = (skill_dir / "scripts/schedule.py").read_text()

    assert "/api/schedules" in schedule_source
    assert "run-now" in schedule_source
    assert "mark-done" in schedule_source
    assert "new_session_each_run" in schedule_source
    assert "create_once_reuse" in schedule_source


def test_readme_installs_bundled_codoxear_session_skill():
    readme = (ROOT / "README.md").read_text()

    assert "skills/codoxear-session" in readme
    assert 'cp -R skills/codoxear-session "$CODEX_HOME/skills/codoxear-session"' in readme
    assert "通过 Codoxear 创建、发送和管理会话" in readme
