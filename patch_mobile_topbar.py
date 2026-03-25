with open("codoxear/static/app.css", "r") as f:
    css = f.read()

old_mob_topbar = """        .topbar {
          padding: calc(8px + env(safe-area-inset-top)) 10px 8px 10px;
          gap: 10px;
          align-items: stretch;
          flex-direction: column;
        }"""

new_mob_topbar = """        .topbar {
          padding: calc(10px + env(safe-area-inset-top)) 12px 10px 12px;
          gap: 12px;
          align-items: stretch;
          flex-direction: column;
        }"""

css = css.replace(old_mob_topbar, new_mob_topbar)

with open("codoxear/static/app.css", "w") as f:
    f.write(css)

