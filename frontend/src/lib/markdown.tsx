type MarkdownBlockProps = {
  text: string;
  className?: string;
};

function copyTextViaSelection(text: string) {
  if (typeof document.execCommand !== "function") {
    throw new Error("Selection copy unavailable");
  }
  const value = String(text ?? "");
  const active = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "");
  textarea.setAttribute("aria-hidden", "true");
  textarea.style.position = "fixed";
  textarea.style.top = "0";
  textarea.style.left = "0";
  textarea.style.width = "1px";
  textarea.style.height = "1px";
  textarea.style.padding = "0";
  textarea.style.border = "0";
  textarea.style.opacity = "0";
  textarea.style.pointerEvents = "none";
  document.body.appendChild(textarea);
  textarea.focus({ preventScroll: true });
  textarea.select();
  textarea.setSelectionRange(0, value.length);
  const ok = document.execCommand("copy");
  textarea.remove();
  active?.focus({ preventScroll: true });
  if (!ok) throw new Error("Selection copy failed");
}

async function copyToClipboard(text: string) {
  if (window.isSecureContext && navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
    await navigator.clipboard.writeText(String(text ?? ""));
    return;
  }
  copyTextViaSelection(text);
}

function escapeHtml(value: unknown) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function safeUrl(value: unknown) {
  const raw = String(value ?? "").trim();
  if (!raw) return "";
  try {
    const base = typeof window !== "undefined" && window.location ? window.location.href : "http://localhost/";
    const url = new URL(raw, base);
    if (url.protocol === "http:" || url.protocol === "https:" || url.protocol === "mailto:") return url.href;
  } catch {
    return "";
  }
  return "";
}

function renderInlineMarkdown(value: string): string {
  const raw = String(value ?? "");
  const pattern = /!\[([^\]]*)\]\(([^)]+)\)|`([^`]+)`|\[([^\]]+)\]\(([^)]+)\)|\*\*([^*]+)\*\*/g;
  let out = "";
  let last = 0;
  for (;;) {
    const match = pattern.exec(raw);
    if (!match) break;
    out += escapeHtml(raw.slice(last, match.index));
    if (match[1] !== undefined) {
      const src = safeUrl(match[2]);
      out += src
        ? `<img src="${escapeHtml(src)}" alt="${escapeHtml(match[1])}" loading="lazy" />`
        : escapeHtml(match[0]);
    } else if (match[3] !== undefined) {
      out += `<code>${escapeHtml(match[3])}</code>`;
    } else if (match[4] !== undefined) {
      const href = safeUrl(match[5]);
      out += href
        ? `<a href="${escapeHtml(href)}" target="_blank" rel="noreferrer noopener">${escapeHtml(match[4])}</a>`
        : escapeHtml(match[4]);
    } else if (match[6] !== undefined) {
      out += `<strong>${escapeHtml(match[6])}</strong>`;
    } else {
      out += escapeHtml(match[0]);
    }
    last = match.index + match[0].length;
  }
  out += escapeHtml(raw.slice(last));
  return out;
}

function splitFenceBlocks(value: string) {
  const chunks: Array<{ type: "text" | "code"; lang?: string; value: string }> = [];
  const lines = value.replace(/\r\n/g, "\n").split("\n");
  let textLines: string[] = [];
  let codeLines: string[] = [];
  let codeLang = "";
  let fenceStart = "";

  function flushText() {
    const text = textLines.join("\n");
    textLines = [];
    if (text.trim()) chunks.push({ type: "text", value: text });
  }

  function flushCode() {
    chunks.push({ type: "code", lang: codeLang, value: codeLines.join("\n") });
    codeLines = [];
    codeLang = "";
    fenceStart = "";
  }

  for (const line of lines) {
    if (!fenceStart) {
      const match = line.match(/^\s{0,3}```\s*([a-zA-Z0-9_-]+)?\s*$/);
      if (match) {
        flushText();
        fenceStart = line;
        codeLang = match[1] || "";
        continue;
      }
      textLines.push(line);
      continue;
    }
    if (line.match(/^\s{0,3}```\s*$/)) {
      flushCode();
      continue;
    }
    codeLines.push(line);
  }

  if (fenceStart) {
    textLines.push(fenceStart, ...codeLines);
  }
  flushText();
  return chunks;
}

function listItem(line: string) {
  const trimmed = line.trim();
  if (trimmed.startsWith("- ") || trimmed.startsWith("* ") || trimmed.startsWith("\u2022 ")) {
    return { type: "ul" as const, text: trimmed.slice(2).trimStart(), start: 0 };
  }
  const ordered = trimmed.match(/^(\d+)\.\s+(.*)$/);
  if (ordered) return { type: "ol" as const, text: (ordered[2] || "").trimStart(), start: Number(ordered[1]) };
  return null;
}

function blockquoteLine(line: string) {
  const match = line.match(/^\s{0,3}>\s?(.*)$/);
  return match ? match[1] || "" : null;
}

function splitTableCells(line: string) {
  let text = line.trim();
  if (!text.includes("|")) return [];
  if (text.startsWith("|")) text = text.slice(1);
  if (text.endsWith("|")) text = text.slice(0, -1);
  const cells: string[] = [];
  let cell = "";
  let escaped = false;
  for (const char of text) {
    if (escaped) {
      cell += char;
      escaped = false;
      continue;
    }
    if (char === "\\") {
      escaped = true;
      continue;
    }
    if (char === "|") {
      cells.push(cell.trim());
      cell = "";
      continue;
    }
    cell += char;
  }
  cells.push(cell.trim());
  return cells;
}

function tableAlignment(line: string) {
  const cells = splitTableCells(line);
  if (!cells.length) return null;
  const alignments: string[] = [];
  for (const cell of cells) {
    const compact = cell.replace(/\s+/g, "");
    if (!/^:?-{3,}:?$/.test(compact)) return null;
    if (compact.startsWith(":") && compact.endsWith(":")) alignments.push("center");
    else if (compact.endsWith(":")) alignments.push("right");
    else if (compact.startsWith(":")) alignments.push("left");
    else alignments.push("");
  }
  return alignments;
}

function renderTable(lines: string[], start: number) {
  if (start + 1 >= lines.length) return null;
  const headers = splitTableCells(lines[start] || "");
  const alignments = tableAlignment(lines[start + 1] || "");
  if (!headers.length || !alignments || headers.length !== alignments.length) return null;
  const rows: string[][] = [];
  let index = start + 2;
  while (index < lines.length) {
    const line = lines[index] || "";
    if (!line.trim() || !line.includes("|") || tableAlignment(line)) break;
    const cells = splitTableCells(line);
    if (cells.length !== headers.length) break;
    rows.push(cells);
    index += 1;
  }
  const alignAttr = (align: string) => (align ? ` style="text-align:${align}"` : "");
  const out = ['<div class="md-table-wrap"><table><thead><tr>'];
  headers.forEach((header, cellIndex) => {
    out.push(`<th${alignAttr(alignments[cellIndex])}>${renderInlineMarkdown(header)}</th>`);
  });
  out.push("</tr></thead><tbody>");
  rows.forEach((row) => {
    out.push("<tr>");
    row.forEach((cell, cellIndex) => {
      out.push(`<td${alignAttr(alignments[cellIndex])}>${renderInlineMarkdown(cell)}</td>`);
    });
    out.push("</tr>");
  });
  out.push("</tbody></table></div>");
  return { html: out.join(""), next: index };
}

function renderMarkdownBlocks(value: string) {
  const out: string[] = [];
  const blocks = value.split(/\n{2,}/);
  for (const block of blocks) {
    const lines = block.split("\n").map((line) => line.trimEnd());
    let paragraph: string[] = [];

    function flushParagraph() {
      const text = paragraph.join("\n").trim();
      paragraph = [];
      if (text) out.push(`<p>${renderInlineMarkdown(text).replaceAll("\n", "<br />")}</p>`);
    }

    for (let index = 0; index < lines.length; index += 1) {
      const line = lines[index] || "";
      const trimmed = line.trim();
      if (!trimmed) {
        flushParagraph();
        continue;
      }
      const heading = trimmed.match(/^(#{1,6})\s+(.*)$/);
      if (heading) {
        flushParagraph();
        const level = heading[1].length;
        out.push(`<h${level}>${renderInlineMarkdown(heading[2] || "")}</h${level}>`);
        continue;
      }
      if (/^(?:-{3,}|\*{3,}|_{3,})$/.test(trimmed.replace(/\s+/g, ""))) {
        flushParagraph();
        out.push("<hr />");
        continue;
      }
      const quote = blockquoteLine(line);
      if (quote !== null) {
        const quoteLines: string[] = [];
        flushParagraph();
        while (index < lines.length) {
          const current = blockquoteLine(lines[index] || "");
          if (current === null) break;
          quoteLines.push(current);
          index += 1;
        }
        out.push(`<blockquote>${renderMarkdownBlocks(quoteLines.join("\n"))}</blockquote>`);
        index -= 1;
        continue;
      }
      const table = renderTable(lines, index);
      if (table) {
        flushParagraph();
        out.push(table.html);
        index = table.next - 1;
        continue;
      }
      const item = listItem(line);
      if (item) {
        flushParagraph();
        const listType = item.type;
        const startAttr = listType === "ol" && item.start > 1 ? ` start="${item.start}"` : "";
        out.push(`<${listType}${startAttr}>`);
        while (index < lines.length) {
          const current = listItem(lines[index] || "");
          if (!current || current.type !== listType) break;
          out.push(`<li>${renderInlineMarkdown(current.text)}</li>`);
          index += 1;
        }
        out.push(`</${listType}>`);
        index -= 1;
        continue;
      }
      paragraph.push(line);
    }
    flushParagraph();
  }
  return out.join("");
}

export function markdownToHtml(text: string) {
  const chunks = splitFenceBlocks(String(text ?? ""));
  return chunks
    .map((chunk) => {
      if (chunk.type === "code") {
        const lang = String(chunk.lang || "").trim();
        const langAttr = lang ? ` data-lang="${escapeHtml(lang)}"` : "";
        const label = lang ? escapeHtml(lang) : "code";
        return `<div class="md-code-block"><div class="md-code-bar"><span class="md-code-lang">${label}</span><button class="md-code-copy-btn" type="button" data-md-code-copy title="Copy code" aria-label="Copy code">Copy</button></div><pre><code${langAttr}>${escapeHtml(chunk.value)}</code></pre></div>`;
      }
      return renderMarkdownBlocks(chunk.value);
    })
    .join("");
}

export function MarkdownBlock({ text, className }: MarkdownBlockProps) {
  async function onClick(event: MouseEvent) {
    const target = event.target instanceof Element ? event.target : null;
    const button = target?.closest("[data-md-code-copy]");
    if (!(button instanceof HTMLButtonElement)) return;
    event.preventDefault();
    event.stopPropagation();
    const code = button.closest(".md-code-block")?.querySelector("code");
    const value = code?.textContent || "";
    if (!value) return;
    const previousText = button.textContent || "Copy";
    try {
      await copyToClipboard(value);
      button.textContent = "Copied";
      button.classList.add("is-copied");
      window.setTimeout(() => {
        button.textContent = previousText;
        button.classList.remove("is-copied");
      }, 1200);
    } catch {
      button.textContent = "Failed";
      button.classList.add("is-failed");
      window.setTimeout(() => {
        button.textContent = previousText;
        button.classList.remove("is-failed");
      }, 1400);
    }
  }

  return <div className={className} onClick={onClick} dangerouslySetInnerHTML={{ __html: markdownToHtml(text) }} />;
}
