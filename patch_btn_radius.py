with open("codoxear/static/app.css", "r") as f:
    css = f.read()

old_btn_base = """      button {
        border: 1px solid var(--border);
        background: #ffffff;
        color: var(--text);
        border-radius: 10px;
        padding: 10px 12px;
        cursor: pointer;
        transition: all 0.15s ease;
        box-shadow: 0 1px 2px rgba(15, 23, 42, 0.02);
      }"""

new_btn_base = """      button {
        border: 1px solid rgba(15, 23, 42, 0.08);
        background: #ffffff;
        color: var(--text);
        border-radius: 12px;
        padding: 10px 12px;
        cursor: pointer;
        transition: all 0.2s ease;
        box-shadow: 0 1px 2px rgba(15, 23, 42, 0.03);
      }"""

css = css.replace(old_btn_base, new_btn_base)

old_icon_btn = """              .icon-btn {
                width: 38px;
                height: 38px;
                padding: 0;
                display: inline-flex;
                align-items: center;
                justify-content: center;
                border-radius: 10px;
                position: relative;
              }"""

new_icon_btn = """              .icon-btn {
                width: 38px;
                height: 38px;
                padding: 0;
                display: inline-flex;
                align-items: center;
                justify-content: center;
                border-radius: 12px;
                position: relative;
              }"""

css = css.replace(old_icon_btn, new_icon_btn)

old_mobile_icon_btn = """        .icon-btn {
          width: 34px;
          height: 34px;
          border-radius: 10px;
        }"""
new_mobile_icon_btn = """        .icon-btn {
          width: 34px;
          height: 34px;
          border-radius: 12px;
        }"""
css = css.replace(old_mobile_icon_btn, new_mobile_icon_btn)

old_icon_active = """      .icon-btn.active {
        border-color: rgba(37, 99, 235, 0.5);
        background: rgba(37, 99, 235, 0.08);
        color: var(--accent);
      }"""

new_icon_active = """      .icon-btn.active {
        border-color: rgba(37, 99, 235, 0.4);
        background: rgba(37, 99, 235, 0.08);
        color: var(--accent);
        box-shadow: 0 2px 6px rgba(37, 99, 235, 0.12);
      }"""

css = css.replace(old_icon_active, new_icon_active)

with open("codoxear/static/app.css", "w") as f:
    f.write(css)

