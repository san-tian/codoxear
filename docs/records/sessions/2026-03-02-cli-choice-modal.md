# CLI Choice Modal

## Request
用户反馈切换 CLI 的方式太繁琐（需要多次点击），希望在创建/复制会话时直接提供三选一的 CLI 选择界面。

## Changes

### UI 改进
1. 添加 CLI 选择模态框（`cliChoice`）：
   - 三个 CLI 按钮（Codex、Claude、Gemini）
   - 显示会话 cwd 信息
   - 取消按钮和背景点击关闭

2. 修改创建/复制会话流程：
   - 新建会话（`#newBtn`）：输入 cwd 后弹出 CLI 选择框
   - 顶部栏复制按钮（`duplicateBtn`）：直接弹出 CLI 选择框
   - 侧边栏复制按钮（`dupBtn`）：直接弹出 CLI 选择框

3. 移除旧的自动继承逻辑：
   - 不再自动使用 `preferredSpawnCli`
   - 不再自动继承原会话的 CLI
   - 每次创建/复制都显式选择

### Implementation

#### JavaScript (`codoxear/static/app.js`)
- 添加 `cliChoice` 模态框 DOM 结构（1483-1545 行）
- 添加 `showCliChoice()` 和 `hideCliChoice()` 函数
- 修改 `$("#newBtn").onclick`：添加 CLI 选择步骤
- 修改 `duplicateBtn.onclick`：添加 CLI 选择步骤
- 修改侧边栏 `dupBtn.onclick`：添加 CLI 选择步骤

#### CSS (`codoxear/static/app.css`)
- `.cliChoice`：模态框容器样式
- `.cliChoiceButtons`：按钮组布局
- `.cliChoiceBtn`：CLI 按钮样式（hover 效果、图标布局）
- `.cliChoiceLogo`：Logo 容器
- `.cliChoiceCancel`：取消按钮样式

### User Experience
- 创建会话：输入 cwd → 选择 CLI → 启动
- 复制会话：点击复制 → 选择 CLI → 启动
- 一次点击完成 CLI 选择，无需多次切换
- 视觉清晰：三个 CLI 并排显示，带 logo 和名称

## Testing
手动测试：
1. 点击"新建会话"按钮，输入 cwd，验证 CLI 选择框弹出
2. 点击顶部栏复制按钮，验证 CLI 选择框弹出
3. 点击侧边栏会话卡片的复制按钮，验证 CLI 选择框弹出
4. 选择不同 CLI，验证会话正确启动
5. 点击取消或背景，验证模态框关闭且不创建会话

## Files Changed
- `codoxear/static/app.js`: 添加 CLI 选择模态框和修改创建/复制逻辑
- `codoxear/static/app.css`: 添加模态框样式

## Notes
- `spawnCliBtn` 的循环切换功能保留，用于快速切换默认 CLI（虽然现在不再自动使用）
- 模态框使用 Promise 模式，与现有的 `sendChoice` 模态框保持一致
- 取消操作返回 `null`，调用方检查后直接返回，不执行创建
