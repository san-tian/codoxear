import re

with open("codoxear/static/app.css", "r") as f:
    css = f.read()

# Replace :root
old_root = """      :root {
        --bg: #e9eef5;
        --panel: #ffffff;
        --border: rgba(15, 23, 42, 0.12);
        --text: #111827;
        --muted: rgba(17, 24, 39, 0.6);
        --accent: #1d4ed8;
        --accent-weak: rgba(29, 78, 216, 0.1);
        --danger: #b91c1c;
        --shadow-sm: 0 1px 2px rgba(15, 23, 42, 0.08);
        --sidebar-w: 320px;
        --bubble-user: #cfe7ff;
        --bubble-assistant: #ffffff;
      }"""

new_root = """      :root {
        --bg: #f3f4f6;
        --panel: #ffffff;
        --border: rgba(15, 23, 42, 0.08);
        --text: #0f172a;
        --muted: rgba(15, 23, 42, 0.55);
        --accent: #2563eb;
        --accent-weak: rgba(37, 99, 235, 0.06);
        --danger: #ef4444;
        --shadow-sm: 0 2px 4px rgba(15, 23, 42, 0.04), 0 1px 2px rgba(15, 23, 42, 0.02);
        --sidebar-w: 320px;
        --bubble-user: #e0f2fe;
        --bubble-assistant: #ffffff;
      }"""

css = css.replace(old_root, new_root)

# Hover effect for session
old_session = """      .session {
        padding: 10px;
        border: 1px solid var(--border);
        border-radius: 12px;
        margin-bottom: 10px;
        cursor: pointer;
        background: #ffffff;
        display: flex;
        gap: 12px;
        align-items: stretch;
        touch-action: manipulation;
      }"""

new_session = """      .session {
        padding: 10px;
        border: 1px solid var(--border);
        border-radius: 12px;
        margin-bottom: 10px;
        cursor: pointer;
        background: #ffffff;
        display: flex;
        gap: 12px;
        align-items: stretch;
        touch-action: manipulation;
        transition: all 0.2s ease;
      }
      .session:hover {
        border-color: rgba(15, 23, 42, 0.16);
        box-shadow: var(--shadow-sm);
        transform: translateY(-1px);
      }"""

css = css.replace(old_session, new_session)

# Better message bubbles
old_msg = """      .msg {
        max-width: min(760px, 82%);
        min-width: 0;
        padding: 10px 12px 18px 12px;
        border-radius: 18px;
        border: 1px solid rgba(15, 23, 42, 0.1);
        position: relative;
        white-space: normal;
        line-height: 1.35;
        box-shadow: var(--shadow-sm);
        overflow-wrap: anywhere;
      }"""

new_msg = """      .msg {
        max-width: min(760px, 82%);
        min-width: 0;
        padding: 12px 14px 20px 14px;
        border-radius: 20px;
        border: 1px solid rgba(15, 23, 42, 0.05);
        position: relative;
        white-space: normal;
        line-height: 1.5;
        box-shadow: var(--shadow-sm);
        overflow-wrap: anywhere;
      }"""
css = css.replace(old_msg, new_msg)

# Refine topbar blur
old_topbar = """      .topbar {
        padding: calc(10px + env(safe-area-inset-top)) 12px 10px 12px;
        border-bottom: 1px solid var(--border);
        background: rgba(255, 255, 255, 0.86);
        backdrop-filter: blur(10px);
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        flex-wrap: wrap;
        row-gap: 6px;
      }"""

new_topbar = """      .topbar {
        padding: calc(10px + env(safe-area-inset-top)) 12px 10px 12px;
        border-bottom: 1px solid var(--border);
        background: rgba(255, 255, 255, 0.8);
        backdrop-filter: blur(16px);
        -webkit-backdrop-filter: blur(16px);
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        flex-wrap: wrap;
        row-gap: 6px;
      }"""
css = css.replace(old_topbar, new_topbar)

# Refine composer (input area)
old_composer_form = """      .composer form {
        max-width: 900px;
        margin: 0 auto;
        display: flex;
        align-items: center;
        gap: 8px;
        border: 1px solid var(--border);
        background: rgba(255, 255, 255, 0.98);
        border-radius: 999px;
        padding: 6px 8px;
        box-shadow: 0 8px 22px rgba(15, 23, 42, 0.06);
        transition: border-radius 120ms ease-out;
      }"""

new_composer_form = """      .composer form {
        max-width: 900px;
        margin: 0 auto;
        display: flex;
        align-items: center;
        gap: 8px;
        border: 1px solid rgba(15, 23, 42, 0.1);
        background: rgba(255, 255, 255, 0.98);
        border-radius: 999px;
        padding: 6px 8px;
        box-shadow: 0 8px 24px rgba(15, 23, 42, 0.06), 0 2px 8px rgba(15, 23, 42, 0.04);
        transition: all 0.2s ease-out;
      }
      .composer form:focus-within {
        border-color: rgba(37, 99, 235, 0.4);
        box-shadow: 0 8px 24px rgba(37, 99, 235, 0.08), 0 2px 8px rgba(37, 99, 235, 0.04);
      }"""
css = css.replace(old_composer_form, new_composer_form)


with open("codoxear/static/app.css", "w") as f:
    f.write(css)

