# Remove Sidebar CLI Toggle

## Request
用户希望调整导航栏左上角，去掉显示 `Codex/Claude/Gemini` 的切换按钮。

## Changes
- 从侧边栏头部 actions 区域移除 `spawnCliBtn`，不再在 UI 中显示常驻 CLI 切换按钮。
- 删除 `spawnCliBtn` 的点击轮换逻辑（`codex -> claude -> gemini`）。
- 保留创建/复制会话时的 CLI 选择模态框流程，不影响新建和复制会话的 CLI 选择能力。
- 继续在会话创建成功后记录最后一次选择的 CLI 到 `localStorage`（`codexweb.spawnCli`），用于内部默认值追踪。

## Testing
- 手动静态检查：确认侧边栏左上角不再出现 `Codex/Claude/Gemini` 文本按钮。
- 手动流程检查：
  1. 点击新建会话，确认仍会弹出 CLI 选择框并可正常创建。
  2. 点击复制会话，确认仍会弹出 CLI 选择框并可正常创建。

## Files Changed
- `codoxear/static/app.js`
- `docs/features/ui.md`
- `docs/records/WORK_RECORDS.md`
- `docs/records/sessions/2026-03-02-remove-sidebar-cli-toggle.md`

## Notes
- 此次修改仅移除常驻切换入口，未改变后端 CLI 启动参数行为。
