with open("codoxear/static/app.css", "r") as f:
    css = f.read()

old_ll = """      .lastLine {
        font-size: 12px;
        line-height: 1.2;
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
      }"""

new_ll = """      .lastLine {
        font-size: 13px;
        color: var(--muted);
        line-height: 1.3;
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
      }"""

css = css.replace(old_ll, new_ll)

with open("codoxear/static/app.css", "w") as f:
    f.write(css)

