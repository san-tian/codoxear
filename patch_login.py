with open("codoxear/static/app.css", "r") as f:
    css = f.read()

old_login = """      .login {
        width: min(92vw, 360px);
        margin: 0;
        padding: 20px;
        border: 1px solid var(--border);
        border-radius: 18px;
        background: var(--panel);
        box-shadow: var(--shadow-sm);
      }"""

new_login = """      .login {
        width: min(92vw, 360px);
        margin: 0;
        padding: 24px;
        border: 1px solid rgba(15, 23, 42, 0.08);
        border-radius: 20px;
        background: rgba(255, 255, 255, 0.95);
        backdrop-filter: blur(12px);
        -webkit-backdrop-filter: blur(12px);
        box-shadow: 0 10px 30px rgba(15, 23, 42, 0.08), 0 4px 10px rgba(15, 23, 42, 0.04);
      }"""

css = css.replace(old_login, new_login)
with open("codoxear/static/app.css", "w") as f:
    f.write(css)

