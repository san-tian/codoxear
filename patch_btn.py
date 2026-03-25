with open("codoxear/static/app.css", "r") as f:
    css = f.read()

old_btn = """      button {
        border: 1px solid var(--border);
        background: #ffffff;
        color: var(--text);
        border-radius: 10px;
        padding: 10px 12px;
        cursor: pointer;
      }"""

new_btn = """      button {
        border: 1px solid var(--border);
        background: #ffffff;
        color: var(--text);
        border-radius: 10px;
        padding: 10px 12px;
        cursor: pointer;
        transition: all 0.15s ease;
        box-shadow: 0 1px 2px rgba(15, 23, 42, 0.02);
      }
      button:hover:not(:disabled) {
        background: rgba(15, 23, 42, 0.02);
        border-color: rgba(15, 23, 42, 0.15);
      }
      button:active:not(:disabled) {
        background: rgba(15, 23, 42, 0.05);
        transform: scale(0.98);
      }"""

css = css.replace(old_btn, new_btn)

old_primary_btn = """      button.primary {
        border-color: rgba(29, 78, 216, 0.35);
        background: rgba(29, 78, 216, 0.08);
        color: #1d4ed8;
      }"""

new_primary_btn = """      button.primary {
        border-color: rgba(37, 99, 235, 0.4);
        background: var(--accent);
        color: #ffffff;
        box-shadow: 0 1px 3px rgba(37, 99, 235, 0.3);
      }
      button.primary:hover:not(:disabled) {
        background: #1d4ed8;
        border-color: #1d4ed8;
        box-shadow: 0 2px 4px rgba(37, 99, 235, 0.4);
      }"""

css = css.replace(old_primary_btn, new_primary_btn)

with open("codoxear/static/app.css", "w") as f:
    f.write(css)

