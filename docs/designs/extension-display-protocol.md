# Extension Display Protocol

## Purpose

`codoxear_display` lets Codex/Pi tools and local plugins publish structured transcript display events without adding one-off Web UI code for each plugin.

## Version 1

Tools attach a `codoxear_display` object to their tool-call arguments. Codoxear parses that object from rollout logs and returns a normalized transcript event with `type: "extension"`.

```json
{
  "codoxear_display": {
    "version": 1,
    "kind": "progress",
    "source": "ralph-loop",
    "title": "Ralph loop",
    "status": "running",
    "summary": "Iteration 2",
    "progress": {
      "current": 2,
      "total": 5,
      "label": "iterations"
    },
    "items": [
      { "label": "Patch parser", "status": "completed" },
      { "label": "Run checks", "status": "pending" }
    ],
    "text": "Optional Markdown-compatible detail."
  }
}
```

The same fields can be passed directly when the tool name is `codoxear_display` or `codoxear.display`.

## Fields

- `version` — protocol version. Current version is `1`.
- `kind` — display kind. Current Web UI has first-class rendering for `progress`.
- `source` — stable plugin/tool identifier, such as `ralph-loop`.
- `title` — short display title.
- `status` — status label, commonly `pending`, `running`, `in_progress`, `completed`, `failed`, or `error`.
- `summary` — one-line status summary.
- `progress.current` and `progress.total` — numeric progress values.
- `progress.label` — unit label for the progress values.
- `items` — ordered progress/checklist rows. Each item supports `label`, `status`, and `detail`.
- `text` — optional Markdown-compatible body text.

## Built-In Mapping

Codex `update_plan` tool calls are normalized into the same `extension` event shape with:

- `source: "codex"`
- `title: "Todo"`
- `kind: "progress"`
- `summary: "<completed>/<total> completed"`
- `items` copied from the plan steps

## Web Behavior

Nova preview renders `extension` events as structured transcript rows with a title, status pill, progress bar, checklist, and optional body text. These rows remain visible when raw tool-call display is hidden, because they are already UI-ready plugin display events rather than raw tool noise.
