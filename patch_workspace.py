with open("codoxear/static/app.css", "r") as f:
    css = f.read()

old_workspace = """      .workspace {
        border: 1px solid var(--border);
        border-radius: 14px;
        padding: 10px;
        margin-bottom: 12px;
        background: rgba(255, 255, 255, 0.96);
        display: flex;
        flex-direction: column;
        gap: 8px;
      }"""

new_workspace = """      .workspace {
        border: 1px solid var(--border);
        border-radius: 14px;
        padding: 10px;
        margin-bottom: 12px;
        background: rgba(255, 255, 255, 0.96);
        display: flex;
        flex-direction: column;
        gap: 8px;
        transition: all 0.2s ease;
      }
      .workspace:hover {
        border-color: rgba(15, 23, 42, 0.12);
        box-shadow: var(--shadow-sm);
      }"""

css = css.replace(old_workspace, new_workspace)

old_ws_file = """      .workspaceFile:hover {
        background: rgba(15, 23, 42, 0.04);
      }"""

new_ws_file = """      .workspaceFile {
        transition: all 0.15s ease;
      }
      .workspaceFile:hover {
        background: rgba(15, 23, 42, 0.04);
        border-color: rgba(15, 23, 42, 0.16);
      }"""

css = css.replace(old_ws_file, new_ws_file)

with open("codoxear/static/app.css", "w") as f:
    f.write(css)

