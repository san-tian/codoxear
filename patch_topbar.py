with open("codoxear/static/app.css", "r") as f:
    css = f.read()

old_topbar = """      .topbar {
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

new_topbar = """      .topbar {
        padding: calc(14px + env(safe-area-inset-top)) 16px 14px 16px;
        border-bottom: 1px solid rgba(15, 23, 42, 0.05);
        background: rgba(255, 255, 255, 0.85);
        backdrop-filter: blur(24px);
        -webkit-backdrop-filter: blur(24px);
        box-shadow: 0 1px 3px rgba(15, 23, 42, 0.02);
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 16px;
        flex-wrap: wrap;
        row-gap: 8px;
        z-index: 10;
      }"""

css = css.replace(old_topbar, new_topbar)

old_title = """      #threadTitle {
        font-weight: 650;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        max-width: 56vw;
      }"""

new_title = """      #threadTitle {
        font-weight: 600;
        font-size: 15px;
        letter-spacing: -0.01em;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        max-width: 56vw;
        color: var(--text);
      }"""

css = css.replace(old_title, new_title)

# Smooth status-chip in topbar
old_chip = """      .status-chip {
        display: inline-flex;
        align-items: center;
        padding: 4px 10px;
        border-radius: 999px;
        border: 1px solid var(--border);
        background: rgba(255, 255, 255, 0.9);
        color: var(--muted);
        font-size: 12px;
        line-height: 1.2;
        white-space: nowrap;
      }"""

new_chip = """      .status-chip {
        display: inline-flex;
        align-items: center;
        padding: 4px 10px;
        border-radius: 999px;
        border: 1px solid rgba(15, 23, 42, 0.08);
        background: rgba(255, 255, 255, 0.9);
        color: var(--muted);
        font-size: 12px;
        line-height: 1.2;
        white-space: nowrap;
        box-shadow: 0 1px 2px rgba(15, 23, 42, 0.02);
        transition: all 0.2s ease;
      }"""

css = css.replace(old_chip, new_chip)

with open("codoxear/static/app.css", "w") as f:
    f.write(css)

