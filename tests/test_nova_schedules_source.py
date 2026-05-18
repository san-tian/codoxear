from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_nova_owner_and_share_expose_schedules_ui():
    app_source = (ROOT / "frontend/src/app.tsx").read_text()
    share_source = (ROOT / "frontend/src/share.tsx").read_text()
    schedule_source = (ROOT / "frontend/src/schedules.tsx").read_text()
    api_source = (ROOT / "frontend/src/lib/api.ts").read_text()

    assert 'icon("schedule")' in app_source
    assert "onOpenSchedules" in share_source
    assert "ScheduleModal" in app_source
    assert "new_session_each_run" in schedule_source
    assert "create_once_reuse" in schedule_source
    assert "Mark as done" in schedule_source
    assert "/api/schedules" in api_source
    assert "/schedules/${encodeURIComponent(scheduleId)}/runs/" in api_source


def test_schedule_routes_include_share_scope_and_lifecycle_actions():
    routes_source = (ROOT / "backend-rs/src/routes.rs").read_text()

    assert '"/share/:share_id/schedules"' in routes_source
    assert '"/api/v1/schedules/:schedule_id/run_now"' in routes_source
    assert '"/api/v1/schedules/:schedule_id/enable"' in routes_source
    assert '"/api/v1/schedules/:schedule_id/runs/:run_id/mark_done"' in routes_source
    assert "authorized_share_session_ids" in routes_source
