      const $ = (q) => document.querySelector(q);
      function updateAppHeightVar() {
        const vv = window.visualViewport;
        const h = vv ? vv.height : window.innerHeight;
        const top = vv ? vv.offsetTop : 0;
        const hh = Math.round(h);
        const tt = Math.round(top);
        if (updateAppHeightVar._h === hh && updateAppHeightVar._t === tt) return;
        updateAppHeightVar._h = hh;
        updateAppHeightVar._t = tt;
        document.documentElement.style.setProperty("--appH", `${hh}px`);
        document.documentElement.style.setProperty("--vvTop", `${tt}px`);
      }
      updateAppHeightVar();
      window.addEventListener("resize", updateAppHeightVar);
      if (window.visualViewport) {
        window.visualViewport.addEventListener("resize", updateAppHeightVar);
        window.visualViewport.addEventListener("scroll", updateAppHeightVar);
      }
      // Best-effort zoom disable (iOS Safari still has edge cases).
      document.addEventListener(
        "gesturestart",
        (e) => {
          e.preventDefault();
        },
        { passive: false }
      );
      document.addEventListener(
        "gesturechange",
        (e) => {
          e.preventDefault();
        },
        { passive: false }
      );
      document.addEventListener(
        "gestureend",
        (e) => {
          e.preventDefault();
        },
        { passive: false }
      );
      document.addEventListener(
        "touchstart",
        (e) => {
          if (e.touches && e.touches.length > 1) e.preventDefault();
        },
        { passive: false }
      );
      document.addEventListener(
        "touchmove",
        (e) => {
          if (e.touches && e.touches.length > 1) e.preventDefault();
        },
        { passive: false }
      );
      const el = (tag, attrs = {}, children = []) => {
        const n = document.createElement(tag);
        for (const [k, v] of Object.entries(attrs)) {
          if (k === "class") n.className = v;
          else if (k === "text") n.textContent = v;
          else if (k === "html") n.innerHTML = v;
          else n.setAttribute(k, v);
        }
        for (const c of children) n.appendChild(c);
        return n;
      };
      let lastTapAt = 0;
      function bindTap(el, handler) {
        if (!el) return;
        const onTap = (e) => {
          if (e && e.preventDefault) e.preventDefault();
          if (e && e.stopPropagation) e.stopPropagation();
          handler(e);
        };
        el.addEventListener("touchend", (e) => {
          lastTapAt = Date.now();
          onTap(e);
        });
        el.addEventListener("click", (e) => {
          if (Date.now() - lastTapAt < 500) return;
          onTap(e);
        });
      }

      const perfWindow = 200;
      const perfSamples = new Map();

      function pushPerfSample(name, valueMs) {
        if (!(valueMs >= 0)) return;
        const arr = perfSamples.get(name) || [];
        arr.push(valueMs);
        if (arr.length > perfWindow) arr.splice(0, arr.length - perfWindow);
        perfSamples.set(name, arr);
      }

      function perfPercentile(sorted, p) {
        if (!sorted.length) return 0;
        if (sorted.length === 1) return sorted[0];
        const pos = Math.max(0, Math.min(1, p)) * (sorted.length - 1);
        const lo = Math.floor(pos);
        const hi = Math.min(lo + 1, sorted.length - 1);
        const frac = pos - lo;
        return sorted[lo] * (1 - frac) + sorted[hi] * frac;
      }

      function summarizePerf() {
        const out = {};
        for (const [k, arr] of perfSamples.entries()) {
          if (!arr.length) continue;
          const s = arr.slice().sort((a, b) => a - b);
          out[k] = {
            count: s.length,
            p50_ms: Math.round(perfPercentile(s, 0.5) * 100) / 100,
            p95_ms: Math.round(perfPercentile(s, 0.95) * 100) / 100,
            max_ms: Math.round(s[s.length - 1] * 100) / 100,
            last_ms: Math.round(arr[arr.length - 1] * 100) / 100,
          };
        }
        return out;
      }

      window.codoxearPerf = summarizePerf;

      const appBaseUrl = new URL(".", window.location.href);
      function resolveAppUrl(path) {
        const s = String(path ?? "");
        const rel = s.startsWith("/") ? s.slice(1) : s;
        return new URL(rel, appBaseUrl).toString();
      }

      function normalizeFileModeValue(raw) {
        const v = String(raw || "").trim().toLowerCase();
        if (v === "view" || v === "edit" || v === "preview") return v;
        return "";
      }

      function parseFileLaunchParams() {
        const params = new URLSearchParams(window.location.search || "");
        const path = params.get("file") || params.get("path") || "";
        const sessionId = params.get("session_id") || params.get("sid") || "";
        const mode = normalizeFileModeValue(params.get("mode") || params.get("file_mode"));
        const fullscreenRaw = params.get("fullscreen") || params.get("full") || "";
        const fullscreen = fullscreenRaw === "1" || fullscreenRaw.toLowerCase() === "true";
        const wrapRaw = params.get("wrap");
        const wrap = wrapRaw === null ? null : wrapRaw === "1" || wrapRaw.toLowerCase() === "true";
        return {
          path: String(path || "").trim(),
          sessionId: String(sessionId || "").trim(),
          mode,
          fullscreen,
          wrap,
        };
      }

      const fileLaunchParams = parseFileLaunchParams();
      if (fileLaunchParams.fullscreen && document.body) document.body.classList.add("file-fullscreen");

      async function api(path, { method = "GET", body } = {}) {
        const t0 = performance.now();
        const opts = { method, headers: {}, cache: "no-store" };
        if (body !== undefined) {
          opts.headers["Content-Type"] = "application/json";
          opts.body = JSON.stringify(body);
        }
        const url = resolveAppUrl(path);
        const res = await fetch(url, opts);
        const txt = await res.text();
        let obj;
        try {
          obj = JSON.parse(txt);
        } catch (e) {
          console.error("api: invalid json response", { path, url, method, txt });
          throw e;
        }
        const dt = performance.now() - t0;
        const rawPath = String(path ?? "");
        if (rawPath === "/api/sessions" && method === "GET") pushPerfSample("api_sessions_ms", dt);
        else if (rawPath.includes("/messages") && method === "GET") {
          if (rawPath.includes("init=1")) pushPerfSample("api_messages_init_ms", dt);
          else pushPerfSample("api_messages_poll_ms", dt);
        }
        if (!res.ok) throw Object.assign(new Error(obj.error || "request failed"), { status: res.status, obj });
        return obj;
      }

      function fmtTs(ts) {
        try {
          const d = new Date(ts * 1000);
          const y = String(d.getFullYear()).padStart(4, "0");
          const m = String(d.getMonth() + 1).padStart(2, "0");
          const day = String(d.getDate()).padStart(2, "0");
          const hh = String(d.getHours()).padStart(2, "0");
          const mm = String(d.getMinutes()).padStart(2, "0");
          return `${y}-${m}-${day} ${hh}:${mm}`;
        } catch {
          return String(ts);
        }
      }

      const FILE_AUTO_REFRESH_MS = 4000;

      function fmtBytes(n) {
        const v = Number(n);
        if (!Number.isFinite(v)) return String(n ?? "");
        if (v < 1024) return `${v} B`;
        const units = ["KB", "MB", "GB", "TB"];
        let val = v;
        let u = 0;
        while (val >= 1024 && u < units.length - 1) {
          val /= 1024;
          u += 1;
        }
        const dec = val >= 100 ? 0 : val >= 10 ? 1 : 2;
        return `${val.toFixed(dec)} ${units[u]}`;
      }

      function listFromFilesField(val) {
        if (!Array.isArray(val)) return [];
        const out = [];
        for (const v of val) {
          if (typeof v !== "string") continue;
          const p = v.trim();
          if (!p || out.includes(p)) continue;
          out.push(p);
        }
        return out;
      }

      function baseName(p) {
        if (!p) return "";
        const s = String(p);
        const parts = s.split("/").filter(Boolean);
        return parts.length ? parts[parts.length - 1] : s;
      }

      function shortSessionId(sid) {
        const s = sid == null ? "" : String(sid);
        const m = s.match(/^([0-9a-f]{8})[0-9a-f-]{28}-(\d+)$/i);
        if (m) return `${m[1]}-${m[2]}`;
        return s.slice(0, 8);
      }

      function sessionDisplayName(s) {
        if (!s || typeof s !== "object") return "";
        const alias = typeof s.alias === "string" ? s.alias.trim() : "";
        if (alias) return alias;
        const cwdName = baseName(s.cwd);
        if (cwdName) return cwdName;
        const ts = typeof s.updated_ts === "number" && Number.isFinite(s.updated_ts)
          ? s.updated_ts
          : typeof s.start_ts === "number" && Number.isFinite(s.start_ts)
            ? s.start_ts
            : 0;
        return ts ? `Session ${fmtTs(ts)}` : "Session";
      }

      function sessionTitleWithId(s) {
        if (!s || typeof s !== "object") return "No session selected";
        const name = sessionDisplayName(s);
        return name || "No session selected";
      }

      function sessionSortName(s) {
        const raw = sessionDisplayName(s);
        return raw ? raw.toLocaleLowerCase() : "";
      }

      function isUploadPathLine(line, sid) {
        if (!line || line[0] !== "/") return false;
        if (!line.includes("/.local/share/codoxear/uploads/")) return false;
        if (sid && !line.includes(`/uploads/${sid}/`)) return false;
        return /\.(png|jpe?g|webp|gif|bmp|heic|heif)$/i.test(line);
      }

      function stripUploadPathLines(text, sid) {
        const raw = String(text ?? "");
        if (!raw) return { text: raw, removed: false };
        const lines = raw.split(/\r?\n/);
        const out = [];
        let removed = false;
        for (const line of lines) {
          const trimmed = line.trim();
          if (trimmed && isUploadPathLine(trimmed, sid)) {
            removed = true;
            continue;
          }
          out.push(line);
        }
        if (!removed) return { text: raw, removed: false };
        let joined = out.join("\n");
        joined = joined.replace(/\n{3,}/g, "\n\n");
        return { text: joined, removed: true };
      }

      function sanitizeUserText(text, sid) {
        const raw = typeof text === "string" ? text : "";
        if (!raw) return { text: raw, dropped: false };
        const { text: stripped, removed } = stripUploadPathLines(raw, sid);
        if (!removed) return { text: raw, dropped: false };
        if (!stripped.trim()) return { text: "", dropped: true };
        return { text: stripped, dropped: false };
      }

      function sanitizeUserEvent(ev, sid) {
        if (!ev || ev.role !== "user" || typeof ev.text !== "string") return ev;
        const res = sanitizeUserText(ev.text, sid);
        if (res.dropped) return null;
        if (res.text !== ev.text) return { ...ev, text: res.text };
        return ev;
      }

      const UNKNOWN_WORKSPACE_KEY = "__unknown_cwd__";

      function normalizeCwd(value) {
        if (typeof value !== "string") return "";
        const trimmed = value.trim();
        if (!trimmed || trimmed === "?") return "";
        if (trimmed.length > 1) return trimmed.replace(/\/+$/, "");
        return trimmed;
      }

      function workspaceKeyFromCwd(cwd) {
        return cwd ? `cwd:${cwd}` : UNKNOWN_WORKSPACE_KEY;
      }

      function workspaceTitleParts(cwd) {
        if (!cwd) return { title: "Unknown cwd", subtitle: "" };
        const base = baseName(cwd);
        if (base && base !== cwd) return { title: base, subtitle: cwd };
        return { title: cwd, subtitle: "" };
      }

      function buildWorkspaces(sessions) {
        const map = new Map();
        for (const s of sessions) {
          const cwd = normalizeCwd(s && s.cwd);
          const key = workspaceKeyFromCwd(cwd);
          let ws = map.get(key);
          if (!ws) {
            ws = { key, cwd, sessions: [], updated_ts: 0 };
            map.set(key, ws);
          }
          ws.sessions.push(s);
          const ts = Number(s && (s.updated_ts || s.start_ts || 0));
          if (Number.isFinite(ts) && ts > ws.updated_ts) ws.updated_ts = ts;
        }
        for (const ws of map.values()) {
          ws.sessions.sort((a, b) => {
            const na = sessionSortName(a);
            const nb = sessionSortName(b);
            const cmp = na.localeCompare(nb, undefined, { numeric: true, sensitivity: "base" });
            if (cmp) return cmp;
            const at = Number(a && (a.updated_ts || a.start_ts || 0));
            const bt = Number(b && (b.updated_ts || b.start_ts || 0));
            if (Number.isFinite(at) && Number.isFinite(bt) && at !== bt) return bt - at;
            const aid = a && a.session_id ? String(a.session_id) : "";
            const bid = b && b.session_id ? String(b.session_id) : "";
            return aid.localeCompare(bid, undefined, { numeric: true, sensitivity: "base" });
          });
        }
        return Array.from(map.values()).sort((a, b) => (b.updated_ts || 0) - (a.updated_ts || 0));
      }

      function escapeHtml(s) {
        return String(s)
          .replaceAll("&", "&amp;")
          .replaceAll("<", "&lt;")
          .replaceAll(">", "&gt;")
          .replaceAll('"', "&quot;")
          .replaceAll("'", "&#39;");
      }

      function safeUrl(u) {
        try {
          const url = new URL(String(u), location.origin);
          if (url.protocol === "http:" || url.protocol === "https:" || url.protocol === "mailto:") return url.href;
        } catch (e) {
          console.error("safeUrl: invalid url", { u, e });
        }
        return null;
      }

      function renderInlineMd(s) {
        const raw = String(s ?? "");
        const re = /`([^`]+)`|\[([^\]]+)\]\(([^)]+)\)|\*\*([^*]+)\*\*/g;
        let out = "";
        let last = 0;
        for (;;) {
          const m = re.exec(raw);
          if (!m) break;
          out += escapeHtml(raw.slice(last, m.index));
          if (m[1] !== undefined) {
            out += `<code>${escapeHtml(m[1])}</code>`;
          } else if (m[2] !== undefined) {
            const href = safeUrl(m[3]);
            if (!href) out += `${escapeHtml(m[2])} (${escapeHtml(m[3])})`;
            else out += `<a href="${escapeHtml(href)}" target="_blank" rel="noreferrer noopener">${escapeHtml(m[2])}</a>`;
          } else if (m[4] !== undefined) {
            out += `<strong>${escapeHtml(m[4])}</strong>`;
          } else {
            out += escapeHtml(m[0]);
          }
          last = m.index + m[0].length;
        }
        out += escapeHtml(raw.slice(last));
        return out;
      }

      function mdToHtml(src) {
        const s = String(src ?? "").replaceAll("\r\n", "\n");
        const splitByFences = (input) => {
          const chunks = [];
          const lines = String(input ?? "").split("\n");
          let textLines = [];
          let inFence = false;
          let fenceLang = "";
          let fenceLines = [];
          let fenceStart = "";

          const flushText = () => {
            const v = textLines.join("\n");
            textLines = [];
            if (v.trim()) chunks.push({ type: "text", value: v });
          };
          const flushFence = () => {
            const v = fenceLines.join("\n");
            fenceLines = [];
            chunks.push({ type: "code", lang: fenceLang, value: v });
            fenceLang = "";
            fenceStart = "";
          };

          for (const line of lines) {
            if (!inFence) {
              const m = line.match(/^\s{0,3}```\s*([a-zA-Z0-9_-]+)?\s*$/);
              if (m) {
                flushText();
                inFence = true;
                fenceLang = m[1] || "";
                fenceStart = line;
                fenceLines = [];
                continue;
              }
              textLines.push(line);
              continue;
            }
            if (line.match(/^\s{0,3}```\s*$/)) {
              inFence = false;
              flushFence();
              continue;
            }
            fenceLines.push(line);
          }

          if (inFence) {
            // Preserve prior behavior: an unclosed fence is not treated as code.
            textLines.push(fenceStart);
            for (const x of fenceLines) textLines.push(x);
          }
          flushText();
          return chunks;
        };

        const listItemInfo = (line) => {
          const l = String(line ?? "");
          let indent = 0;
          while (indent < l.length && l[indent] === " ") indent += 1;
          const t = l.trim();
          if (t.startsWith("- ") || t.startsWith("* ") || t.startsWith("\u2022 ")) {
            return { type: "ul", indent, text: t.slice(2).trimStart() };
          }
          const mOl = t.match(/^(\d+)\.\s+(.*)$/);
          if (mOl) return { type: "ol", indent, text: (mOl[2] || "").trimStart(), num: Number(mOl[1]) };
          return null;
        };

        const parseList = (lines, start) => {
          const head = listItemInfo(lines[start]);
          if (!head) throw new Error("parseList called on non-list line");
          const baseIndent = head.indent;
          const listType = head.type;
          const listStart = listType === "ol" && Number.isFinite(head.num) ? head.num : null;
          const items = [];

          let i = start;
          while (i < lines.length) {
            const info = listItemInfo(lines[i]);
            if (!info) break;
            if (info.indent < baseIndent) break;
            if (info.indent > baseIndent) {
              if (!items.length) break;
              const child = parseList(lines, i);
              items[items.length - 1].child = child.node;
              i = child.next;
              continue;
            }
            if (info.type !== listType) break;
            items.push({ text: info.text, child: null });
            i += 1;
          }
          return { node: { type: listType, items, start: listStart }, next: i };
        };

        const renderList = (node) => {
          const out = [];
          if (node.type === "ol") {
            const startAttr = Number.isFinite(node.start) && node.start > 1 ? ` start="${node.start}"` : "";
            out.push(`<ol${startAttr}>`);
          } else {
            out.push("<ul>");
          }
          for (const it of node.items) {
            out.push("<li>");
            out.push(renderInlineMd(it.text || ""));
            if (it.child) out.push(renderList(it.child));
            out.push("</li>");
          }
          out.push(node.type === "ol" ? "</ol>" : "</ul>");
          return out.join("");
        };

        const chunks = splitByFences(s);

        const out = [];
        for (const c of chunks) {
          if (c.type === "code") {
            const lang = String(c.lang || "").trim().toLowerCase();
            if (lang === "mermaid" || lang === "mmd") {
              out.push(`<div class="mermaid">${escapeHtml(c.value)}</div>`);
            } else {
              const langAttr = c.lang ? ` data-lang="${escapeHtml(c.lang)}"` : "";
              out.push(`<pre><code${langAttr}>${escapeHtml(c.value)}</code></pre>`);
            }
            continue;
          }
          const blocks = c.value.split(/\n{2,}/);
          for (const block of blocks) {
            const lines = block.split("\n").map((x) => x.trimEnd());
            if (!lines.length) continue;

            const head = lines[0] || "";
            const mHeading = head.match(/^(#{1,6})\s+(.*)$/);
            let startIdx = 0;
            if (mHeading) {
              const level = mHeading[1].length;
              out.push(`<h${level}>${renderInlineMd(mHeading[2])}</h${level}>`);
              startIdx = 1;
            }

            let paraLines = [];
            const flushPara = () => {
              const para = paraLines.join("\n").trim();
              paraLines = [];
              if (!para) return;
              out.push(`<p>${renderInlineMd(para).replaceAll("\n", "<br />")}</p>`);
            };

            for (let i = startIdx; i < lines.length; i++) {
              const l = lines[i] || "";
              const t = l.trim();
              if (!t) {
                flushPara();
                continue;
              }
              const info = listItemInfo(l);
              if (info) {
                flushPara();
                const parsed = parseList(lines, i);
                out.push(renderList(parsed.node));
                i = parsed.next - 1;
                continue;
              }
              paraLines.push(l);
            }
            flushPara();
          }
        }
        return out.join("");
      }

      const mdCache = new Map();
      function mdToHtmlCached(src) {
        const key = String(src ?? "");
        const hit = mdCache.get(key);
        if (hit !== undefined) return hit;
        const html = mdToHtml(key);
        mdCache.set(key, html);
        if (mdCache.size > 1200) {
          // Prevent unbounded growth; chat history is expected to be small.
          mdCache.clear();
        }
        return html;
      }

      let mermaidConfigured = false;
      function ensureMermaidReady() {
        const m = window.mermaid;
        if (!m) return null;
        if (!mermaidConfigured && typeof m.initialize === "function") {
          try {
            m.initialize({ startOnLoad: false, securityLevel: "strict" });
          } catch (e) {
            console.warn("mermaid initialize failed", e);
          }
          mermaidConfigured = true;
        }
        return m;
      }

      function renderMermaidIn(root) {
        const m = ensureMermaidReady();
        if (!m || !root || typeof root.querySelectorAll !== "function" || typeof m.run !== "function") return;
        const nodes = root.querySelectorAll(".mermaid");
        if (!nodes.length) return;
        Promise.resolve(m.run({ nodes, suppressErrors: true })).catch((e) => {
          console.warn("mermaid render failed", e);
        });
      }

      function iconSvg(name) {
        if (name === "menu")
          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 6h16M4 12h16M4 18h16"/></svg>`;
        if (name === "refresh")
          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20 12a8 8 0 1 1-2.34-5.66"/><path d="M20 4v6h-6"/></svg>`;
	        if (name === "harness")
	          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 12h3l2-4 3 8 2-4h6"/><path d="M12 21a9 9 0 1 0-9-9"/></svg>`;
	        if (name === "stop")
	          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="7" y="7" width="10" height="10" rx="2"/></svg>`;
	        if (name === "plus")
	          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>`;
	        if (name === "logout")
          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M10 17l5-5-5-5"/><path d="M15 12H3"/><path d="M21 3v18"/></svg>`;
        if (name === "send")
          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 2L11 13"/><path d="M22 2l-7 20-4-9-9-4 20-7z"/></svg>`;
        if (name === "paperclip")
          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21.44 11.05l-8.49 8.49a5 5 0 0 1-7.07-7.07l9.19-9.19a3.5 3.5 0 0 1 4.95 4.95l-9.19 9.19a2 2 0 0 1-2.83-2.83l8.49-8.49"/></svg>`;
        if (name === "down")
          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14"/><path d="M19 12l-7 7-7-7"/></svg>`;
        if (name === "trash")
          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M6 6l1 16h10l1-16"/><path d="M10 11v6"/><path d="M14 11v6"/></svg>`;
        if (name === "edit")
          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/></svg>`;
        if (name === "file")
          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z"/><path d="M14 2v6h6"/></svg>`;
        if (name === "x")
          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 6 18"/><path d="M6 6l12 12"/></svg>`;
        if (name === "queue")
          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 7h16"/><path d="M4 12h16"/><path d="M4 17h10"/></svg>`;
        if (name === "duplicate")
          return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="8" y="8" width="11" height="11" rx="2"/><rect x="5" y="5" width="11" height="11" rx="2"/></svg>`;
        return "";
      }

      function renderLogin(onAuthed) {
        const root = $("#root");
        root.innerHTML = "";
        const err = el("div", { class: "err" });
        const wrap = el("div", { class: "loginWrap" });
        const box = el("div", { class: "login" }, [
          el("h1", { text: "Codoxear login" }),
          el("div", { class: "row2" }, [
            el("input", { type: "password", id: "pw", placeholder: "Password" }),
            el("button", { class: "primary", id: "loginBtn", text: "Login" }),
            err,
          ]),
        ]);
        wrap.appendChild(box);
        root.appendChild(wrap);
        $("#loginBtn").onclick = async () => {
          err.textContent = "";
          const pw = $("#pw").value;
          try {
            await api("/api/login", { method: "POST", body: { password: pw } });
            onAuthed();
          } catch (e) {
            err.textContent = e.obj?.error || e.message;
          }
        };
      }

	      function renderApp() {
	        const root = $("#root");
	        root.innerHTML = "";

	        const backdrop = el("div", { class: "backdrop", id: "backdrop" });
	        const app = el("div", { class: "app" });
        const sidebar = el("div", { class: "sidebar" });
        const sessionsWrap = el("div", { class: "sessions" });
        const sidebarFooter = el("footer", {}, [
          el("button", { id: "logoutBtnSide", type: "button", title: "Log out", "aria-label": "Log out", text: "Log out" }),
        ]);
        const main = el("div", { class: "main" });
        const chatWrap = el("div", { class: "chatWrap", id: "chatWrap" });
        const chat = el("div", { class: "chat", id: "chat" });
        const chatInner = el("div", { class: "chatInner", id: "chatInner" });
        const olderWrap = el("div", { class: "olderWrap", id: "olderWrap" });
        const olderBtn = el("button", {
          class: "olderBtn",
          id: "olderBtn",
          type: "button",
          text: "Load older messages",
        });
        olderWrap.appendChild(olderBtn);
        const bottomSentinel = el("div", { id: "bottomSentinel" });
        const jumpBtn = el("button", {
          class: "jumpBtn",
          id: "jumpBtn",
          title: "Jump to latest",
          "aria-label": "Jump to latest",
          html: iconSvg("down"),
        });
        chatInner.appendChild(olderWrap);
        chatInner.appendChild(bottomSentinel);
        chat.appendChild(chatInner);
        chatWrap.appendChild(chat);
        chatWrap.appendChild(jumpBtn);
        const composer = el("div", { class: "composer" });

	        let selected = null;
        let offset = 0;
        const INIT_PAGE_LIMIT_DESKTOP = 60;
        const INIT_PAGE_LIMIT_MOBILE = 24;
        const OLDER_PAGE_LIMIT = 60;
        const CACHE_LIMIT = 40;
        const CHAT_DOM_WINDOW = 260;
        const DEFER_IDLE_RELEASE_MS = 2000;
        const DEFER_GATE_MIN_MS = 8000;
        let activeLogPath = null;
        let activeThreadId = null;
        let olderBefore = 0;
        let hasOlder = false;
        let loadingOlder = false;
        let olderAutoTriggerAt = 0;
        const OLDER_AUTO_COOLDOWN_MS = 450;
        let pollTimer = null;
        let pollGen = 0;
        let pollLoopBusy = false;
        let pollKickPending = false;
	        let pollFastUntilMs = 0;
	        let turnOpen = false;
	        let sessionsTimer = null;
	        let currentRunning = false;
        let deferInFlight = false;
        const deferredBySession = new Map();
        const deferredLoaded = new Set();
        const deferGateBySession = new Map();
        const deferGateSinceBySession = new Map();
        const deferIdleSinceBySession = new Map();
        const lastActivityBySession = new Map();
        let queueSaveTimer = null;
        const draftBySession = new Map();
        const draftSaveTimers = new Map();
        const seenAssistantBySession = new Map();
        const lastLineBySession = new Map();
        const lastLineSaveTimers = new Map();
        const userSummaryBySession = new Map();
        const userSummarySaveTimers = new Map();
        const cacheBySession = new Map();
        const cacheLoaded = new Set();
        const cacheSaveTimers = new Map();
	        let sessionIndex = new Map(); // session_id -> session info
        function normalizeSessionName(name) {
          return String(name || "")
            .trim()
            .replace(/\s+/g, " ")
            .toLocaleLowerCase();
        }
        function collectSessionNameSet({ excludeId } = {}) {
          const out = new Set();
          for (const s of sessionIndex.values()) {
            if (!s || (excludeId && s.session_id === excludeId)) continue;
            const name = sessionDisplayName(s);
            if (!name) continue;
            out.add(normalizeSessionName(name));
          }
          return out;
        }
        function buildDuplicateAlias(baseName) {
          const base = String(baseName || "").trim() || "Session";
          const existing = collectSessionNameSet();
          let candidate = `${base} duplicate`;
          if (!existing.has(normalizeSessionName(candidate))) return candidate;
          for (let i = 2; i < 200; i += 1) {
            candidate = `${base} duplicate ${i}`;
            if (!existing.has(normalizeSessionName(candidate))) return candidate;
          }
          return `${base} duplicate ${Date.now()}`;
        }
	        let sending = false;
	        let localEchoSeq = 0;
	        const pendingUser = [];
	        let attachedImages = 0;
		        let autoScroll = true;
			        let backfillToken = 0;
        let backfillState = null;
			    let lastScrollTop = 0;
				    let lastToken = null;
				    let typingRow = null;
        let attachBadgeEl = null;
        let queueBadgeEl = null;
        const recentEventKeys = [];
        const recentEventKeySet = new Set();
        const recentEventSigTs = new Map();
        const recentEventSigOrder = [];
        const RECENT_EVENT_KEYS_MAX = 320;
        const RECENT_EVENT_SIG_MAX = 360;
        const RECENT_EVENT_SIG_WINDOW_MS = 1600;
                let clickLoadT0 = 0;
                let clickMetricPending = false;
					        let harnessMenuOpen = false;
					        let harnessCfg = { enabled: false, request: "" };
					        let harnessSaveTimer = null;

				        const titleLabel = el("div", { id: "threadTitle", text: "No session selected" });
				        const statusChip = el("span", { class: "status-chip", id: "statusChip", text: "Idle" });
				        const ctxChip = el("span", { class: "status-chip", id: "ctxChip", text: "" });
		        ctxChip.style.display = "none";
        const interruptBtn = el("button", {
          id: "interruptBtn",
          class: "icon-btn",
          title: "Interrupt (Esc)",
          "aria-label": "Interrupt (Esc)",
          type: "button",
          html: iconSvg("stop"),
        });
        interruptBtn.style.display = "none";
        const toast = el("div", { class: "muted toast", id: "toast" });
			        const toggleSidebarBtn = el("button", {
	          id: "toggleSidebarBtn",
	          class: "icon-btn",
	          title: "Toggle sidebar",
	          "aria-label": "Toggle sidebar",
	          html: iconSvg("menu"),
	        });
        const harnessBtn = el("button", {
          id: "harnessBtn",
          class: "icon-btn",
          title: "Harness mode",
          "aria-label": "Harness mode",
            type: "button",
            html: iconSvg("harness"),
          });
          harnessBtn.disabled = true;
          harnessBtn.classList.toggle("active", false);
        const renameBtn = el("button", {
          id: "renameBtn",
          class: "icon-btn",
          title: "Rename session",
          "aria-label": "Rename session",
          type: "button",
          html: iconSvg("edit"),
        });
        renameBtn.disabled = true;
        const duplicateBtn = el("button", {
          id: "dupBtn",
          class: "icon-btn",
          title: "Duplicate session",
          "aria-label": "Duplicate session",
          type: "button",
          html: iconSvg("duplicate"),
        });
        duplicateBtn.disabled = true;
        const fileBtn = el("button", {
          id: "fileBtn",
          class: "icon-btn",
          title: "View file",
          "aria-label": "View file",
          type: "button",
          html: iconSvg("file"),
        });
        const harnessMenu = el("div", { id: "harnessMenu", class: "harnessMenu", role: "dialog", "aria-label": "Harness mode settings" }, [
          el("div", { class: "row" }, [
            el("label", {}, [
              el("input", { type: "checkbox", id: "harnessEnabled" }),
              el("span", { text: "Harness mode" }),
			            ]),
			          ]),
			          el("div", { class: "label", text: "Additional request to append (optional; per session)" }),
			          el("textarea", { id: "harnessRequest", "aria-label": "Additional request for harness prompt" }),
			        ]);

        const topMeta = el("div", { class: "topMeta" }, [statusChip, ctxChip]);
        const titleRow = el("div", { class: "titleRow" }, [titleLabel, topMeta]);
        const lastLine = el("div", { class: "lastLine muted", id: "lastLine", text: "" });
        const titleWrap = el("div", { class: "titleWrap" }, [titleRow, lastLine, toast]);
        const topbar = el("div", { class: "topbar" }, [
          el("div", { class: "pill" }, [toggleSidebarBtn, titleWrap]),
          el("div", { class: "actions topActions" }, [
            duplicateBtn,
            renameBtn,
            fileBtn,
            interruptBtn,
            harnessBtn,
          ]),
        ]);

        const form = el("form", {}, [
          el("button", {
            class: "icon-btn",
            id: "attachBtn",
            type: "button",
            title: "Attach image",
            "aria-label": "Attach image",
            html: iconSvg("paperclip"),
          }),
          el("div", { class: "inputWrap" }, [
            el("textarea", { id: "msg", placeholder: "", "aria-label": "Enter your instructions here" }),
            el("div", { class: "ph", id: "msgPh", text: "Enter your instructions here" }),
          ]),
          el("input", { id: "imgInput", type: "file", accept: "image/*", style: "display:none" }),
          el("button", { class: "icon-btn primary", id: "sendBtn", type: "submit", title: "Send", "aria-label": "Send", html: iconSvg("send") }),
          el("button", { class: "icon-btn", id: "queueBtn", type: "button", title: "Queued messages", "aria-label": "Queued messages", html: iconSvg("queue") }),
        ]);
        composer.appendChild(form);

        sidebar.appendChild(
          el("header", {}, [
            el("div", { class: "title", html: `<img class="sidebarLogo" src="/static/codoxear-icon.png" alt="" />Codoxear` }),
            el("div", { class: "actions" }, [
              el("button", { id: "newBtn", class: "icon-btn", title: "New session", "aria-label": "New session", html: iconSvg("plus") }),
              el("button", { id: "refreshBtn", class: "icon-btn", title: "Refresh", "aria-label": "Refresh", html: iconSvg("refresh") }),
            ]),
          ])
        );
        sidebar.appendChild(sessionsWrap);
        sidebar.appendChild(sidebarFooter);
        main.appendChild(topbar);
        main.appendChild(chatWrap);
        main.appendChild(composer);
        app.appendChild(sidebar);
        app.appendChild(main);
        app.appendChild(backdrop);
        root.appendChild(app);
        root.appendChild(harnessMenu);

        const fileBackdrop = el("div", { class: "modalBackdrop", id: "fileBackdrop" });
        const fileCloseBtn = el("button", {
          id: "fileCloseBtn",
          class: "icon-btn",
          title: "Close",
          "aria-label": "Close",
          type: "button",
          html: iconSvg("x"),
        });
        const fileWrapBtn = el("button", {
          id: "fileWrapBtn",
          class: "icon-btn text-btn",
          title: "Toggle line wrap",
          "aria-label": "Toggle line wrap",
          type: "button",
          text: "Wrap",
        });
        const fileViewBtn = el("button", {
          id: "fileViewBtn",
          class: "icon-btn text-btn",
          title: "View raw",
          "aria-label": "View raw",
          type: "button",
          text: "View",
        });
        const fileEditBtn = el("button", {
          id: "fileEditBtn",
          class: "icon-btn text-btn",
          title: "Edit file",
          "aria-label": "Edit file",
          type: "button",
          text: "Edit",
        });
        const filePreviewBtn = el("button", {
          id: "filePreviewBtn",
          class: "icon-btn text-btn",
          title: "Preview markdown",
          "aria-label": "Preview markdown",
          type: "button",
          text: "Preview",
        });
        const fileOpenInlineBtn = el("button", {
          id: "fileOpenInlineBtn",
          class: "icon-btn text-btn",
          title: "Open file path",
          "aria-label": "Open file path",
          type: "button",
          text: "Open",
        });
        const fileSaveBtn = el("button", {
          id: "fileSaveBtn",
          class: "icon-btn primary text-btn",
          title: "Save file",
          "aria-label": "Save file",
          type: "button",
          text: "Save",
        });
        const filePopoutBtn = el("button", {
          id: "filePopoutBtn",
          class: "icon-btn text-btn",
          title: "Open in new tab",
          "aria-label": "Open in new tab",
          type: "button",
          text: "Pop out",
        });
        const filePathInput = el("input", { id: "filePathInput", type: "text", placeholder: "/path/to/file" });
        const fileOpenBtn = el("button", { id: "fileOpenBtn", class: "primary", type: "button", text: "Open" });
        const fileStatus = el("div", { class: "muted", id: "fileStatus", text: "" });
        const fileContent = el("pre", { class: "fileContent", id: "fileContent" });
        const fileEditor = el("textarea", { class: "fileEditor", id: "fileEditor", spellcheck: "false" });
        const filePreview = el("div", { class: "filePreview md", id: "filePreview" });
        const filePathRow = el("div", { class: "filePathRow" }, [filePathInput, fileOpenBtn]);
        const fileTitle = el("div", { class: "title", text: "View file" });
        const fileViewer = el("div", { class: "fileViewer", id: "fileViewer", role: "dialog", "aria-label": "File viewer" }, [
          el("div", { class: "fileViewerHeader" }, [
            fileTitle,
            el("div", { class: "actions" }, [
              fileViewBtn,
              fileEditBtn,
              filePreviewBtn,
              fileSaveBtn,
              filePopoutBtn,
              fileOpenInlineBtn,
              fileWrapBtn,
              fileCloseBtn,
            ]),
          ]),
          filePathRow,
          fileStatus,
          fileContent,
          fileEditor,
          filePreview,
        ]);
        root.appendChild(fileBackdrop);
        root.appendChild(fileViewer);

        const fileLaunch = fileLaunchParams;
        const fileSessionOverride = fileLaunch.sessionId;
        const fileFullscreenMode = Boolean(fileLaunch.fullscreen);
        const fileLaunchActive = Boolean(fileLaunch.fullscreen || fileLaunch.path);
        let fileLaunchHandled = false;

        const sendChoiceBackdrop = el("div", { class: "modalBackdrop", id: "sendChoiceBackdrop" });
        const sendChoice = el("div", { class: "sendChoice", id: "sendChoice", role: "dialog", "aria-label": "Send options" }, [
          el("div", { class: "title", text: "Current response is running" }),
          el("div", { class: "muted", text: "Choose how to handle your next message." }),
          el("div", { class: "sendChoiceActions" }, [
            el("button", { class: "primary", id: "sendChoiceNow", type: "button", text: "Send now" }),
            el("button", { id: "sendChoiceLater", type: "button", text: "Send after current" }),
            el("button", { id: "sendChoiceCancel", type: "button", text: "Cancel" }),
          ]),
        ]);
        root.appendChild(sendChoiceBackdrop);
        root.appendChild(sendChoice);

        const queueBackdrop = el("div", { class: "modalBackdrop", id: "queueBackdrop" });
        const queueCloseBtn = el("button", {
          id: "queueCloseBtn",
          class: "icon-btn",
          title: "Close",
          "aria-label": "Close",
          type: "button",
          html: iconSvg("x"),
        });
        const queueList = el("div", { class: "queueList", id: "queueList" });
        const queueEmpty = el("div", { class: "muted", id: "queueEmpty", text: "No queued messages." });
        const queueViewer = el("div", { class: "queueViewer", id: "queueViewer", role: "dialog", "aria-label": "Queued messages" }, [
          el("div", { class: "queueHeader" }, [
            el("div", { class: "title", text: "Queued messages" }),
            el("div", { class: "actions" }, [queueCloseBtn]),
          ]),
          queueEmpty,
          queueList,
        ]);
        root.appendChild(queueBackdrop);
        root.appendChild(queueViewer);

        function setToast(text) {
          toast.textContent = text || "";
          if (!text) return;
          setTimeout(() => {
            if (toast.textContent === text) toast.textContent = "";
          }, 2200);
        }

        function setStatus({ running, queueLen }) {
          const q = Number(queueLen || 0);
          const mobile = isMobile();
          const wasRunning = currentRunning;
          currentRunning = Boolean(running);
          if (selected && currentRunning) {
            const dq = getDeferredQueue(selected);
            if (dq.length) {
              markDeferGate(selected);
            }
          }
          if (running) {
            statusChip.style.display = "none";
            statusChip.classList.remove("running");
          } else {
            statusChip.style.display = "inline-flex";
			            if (q) statusChip.textContent = mobile ? `Q ${q}` : `Queue ${q}`;
			            else statusChip.textContent = "Idle";
          }
          interruptBtn.style.display = running && selected ? "inline-flex" : "none";
          interruptBtn.disabled = !(running && selected);
          if (wasRunning && !currentRunning) {
            deferInFlight = false;
          }
          if (!currentRunning) {
            maybeSendDeferred();
          }
        }

	        function setContext(tok) {
	          if (!tok || typeof tok !== "object") {
	            lastToken = null;
	            ctxChip.style.display = "none";
	            ctxChip.textContent = "";
	            ctxChip.title = "";
	            return;
	          }
	          const ctx = Number(tok.context_window);
	          const used = Number(tok.tokens_in_context);
	          const pct = Number(tok.percent_remaining);
	          if (!Number.isFinite(ctx) || !Number.isFinite(used) || ctx <= 0 || used < 0) {
	            lastToken = null;
	            ctxChip.style.display = "none";
	            return;
	          }
	          const p = Number.isFinite(pct) ? Math.max(0, Math.min(100, Math.round(pct))) : null;
	          lastToken = { ctx, used, pct: p, remaining: Math.max(ctx - used, 0), baseline: Number(tok.baseline_tokens) || 0, asOf: tok.as_of || "" };
	          ctxChip.style.display = "inline-flex";
	          ctxChip.textContent = p === null ? "Ctx" : `Ctx ${p}%`;
	          ctxChip.title = `Context: ${used}/${ctx} tokens (baseline ${lastToken.baseline}).`;
	        }
	        ctxChip.onclick = () => {
	          if (!lastToken) return;
	          setToast(`ctx ${lastToken.used}/${lastToken.ctx} (${lastToken.pct ?? "?"}% left)`);
	        };

        function resetChatRenderState() {
          autoScroll = true;
          pendingUser.length = 0;
          sending = false;
          localEchoSeq = 0;
          recentEventKeys.length = 0;
          recentEventKeySet.clear();
          recentEventSigTs.clear();
          recentEventSigOrder.length = 0;
          olderBefore = 0;
          hasOlder = false;
          loadingOlder = false;
          olderAutoTriggerAt = 0;
              clickMetricPending = false;
		          chatInner.innerHTML = "";
	          chatInner.appendChild(olderWrap);
	          chatInner.appendChild(bottomSentinel);
              setOlderState({ hasMore: false, isLoading: false });
	          typingRow = null;
	          jumpBtn.style.display = "none";
	          backfillState = null;
	          backfillToken += 1;
	          lastScrollTop = 0;
	          chat.scrollTop = 0;
	        }

        function setOlderState({ hasMore, isLoading }) {
          hasOlder = Boolean(hasMore);
          loadingOlder = Boolean(isLoading);
          olderWrap.style.display = hasOlder ? "flex" : "none";
          olderBtn.disabled = loadingOlder;
          olderBtn.textContent = loadingOlder ? "Loading..." : "Load older messages";
        }

        function initPageLimit() {
          return isMobile() ? INIT_PAGE_LIMIT_MOBILE : INIT_PAGE_LIMIT_DESKTOP;
        }

        function olderPageLimit() {
          return OLDER_PAGE_LIMIT;
        }

        function deferredStorageKey(sid) {
          return `codexweb.deferred.${sid}`;
        }

        function loadDeferredFromStorage(sid) {
          if (!sid || deferredLoaded.has(sid)) return;
          deferredLoaded.add(sid);
          try {
            const raw = localStorage.getItem(deferredStorageKey(sid));
            if (!raw) return;
            const arr = JSON.parse(raw);
            if (!Array.isArray(arr)) return;
            const out = [];
            for (const v of arr) {
              if (typeof v !== "string") continue;
              const t = v.trim();
              if (!t) continue;
              out.push(t);
            }
            if (out.length) deferredBySession.set(sid, out);
          } catch {
            // ignore corrupted storage
          }
        }

        function saveDeferredToStorage(sid) {
          if (!sid) return;
          const q = deferredBySession.get(sid) || [];
          try {
            localStorage.setItem(deferredStorageKey(sid), JSON.stringify(q));
          } catch {
            // ignore quota or serialization issues
          }
        }

        function scheduleQueueSave() {
          if (!selected) return;
          if (queueSaveTimer) clearTimeout(queueSaveTimer);
          queueSaveTimer = setTimeout(() => {
            if (!selected) return;
            saveDeferredToStorage(selected);
          }, 350);
        }

        function getDeferredQueue(sid) {
          if (!sid) return [];
          loadDeferredFromStorage(sid);
          let q = deferredBySession.get(sid);
          if (!q) {
            q = [];
            deferredBySession.set(sid, q);
          }
          return q;
        }

        function draftStorageKey(sid) {
          return `codexweb.draft.${sid}`;
        }

        function loadDraftFromStorage(sid) {
          if (!sid) return "";
          if (draftBySession.has(sid)) return draftBySession.get(sid) || "";
          let raw = "";
          try {
            raw = localStorage.getItem(draftStorageKey(sid)) || "";
          } catch {
            raw = "";
          }
          draftBySession.set(sid, raw);
          return raw;
        }

        function saveDraftToStorage(sid, text) {
          if (!sid) return;
          const val = String(text || "");
          draftBySession.set(sid, val);
          try {
            if (val) localStorage.setItem(draftStorageKey(sid), val);
            else localStorage.removeItem(draftStorageKey(sid));
          } catch {
            // ignore quota or storage errors
          }
        }

        function scheduleDraftSave(sid, text) {
          if (!sid) return;
          const prev = draftSaveTimers.get(sid);
          if (prev) clearTimeout(prev);
          const t = setTimeout(() => {
            draftSaveTimers.delete(sid);
            saveDraftToStorage(sid, text);
          }, 250);
          draftSaveTimers.set(sid, t);
        }

        function clearDraftForSession(sid) {
          if (!sid) return;
          const prev = draftSaveTimers.get(sid);
          if (prev) clearTimeout(prev);
          draftSaveTimers.delete(sid);
          draftBySession.delete(sid);
          try {
            localStorage.removeItem(draftStorageKey(sid));
          } catch {
            // ignore
          }
        }

        function seenAssistantStorageKey(sid) {
          return `codexweb.seen.assistant.${sid}`;
        }

        function loadSeenAssistantTs(sid) {
          if (!sid) return 0;
          if (seenAssistantBySession.has(sid)) return seenAssistantBySession.get(sid) || 0;
          let raw = "";
          try {
            raw = localStorage.getItem(seenAssistantStorageKey(sid)) || "";
          } catch {
            raw = "";
          }
          const val = Number(raw);
          const out = Number.isFinite(val) ? val : 0;
          seenAssistantBySession.set(sid, out);
          return out;
        }

        function saveSeenAssistantTs(sid, ts) {
          if (!sid) return;
          const val = Number(ts);
          if (!Number.isFinite(val) || val <= 0) return;
          seenAssistantBySession.set(sid, val);
          try {
            localStorage.setItem(seenAssistantStorageKey(sid), String(val));
          } catch {
            // ignore storage issues
          }
        }

        function markAssistantSeen(sid, ts) {
          if (!sid) return;
          const val = Number(ts);
          if (!Number.isFinite(val) || val <= 0) return;
          const cur = loadSeenAssistantTs(sid);
          if (val > cur) saveSeenAssistantTs(sid, val);
        }

        function clearSeenAssistantForSession(sid) {
          if (!sid) return;
          seenAssistantBySession.delete(sid);
          try {
            localStorage.removeItem(seenAssistantStorageKey(sid));
          } catch {
            // ignore
          }
        }

        function lastLineStorageKey(sid) {
          return `codexweb.lastline.${sid}`;
        }

        function loadLastLineFromStorage(sid) {
          if (!sid) return "";
          if (lastLineBySession.has(sid)) return lastLineBySession.get(sid) || "";
          let raw = "";
          try {
            raw = localStorage.getItem(lastLineStorageKey(sid)) || "";
          } catch {
            raw = "";
          }
          lastLineBySession.set(sid, raw);
          return raw;
        }

        function saveLastLineToStorage(sid, text) {
          if (!sid) return;
          const val = String(text || "");
          lastLineBySession.set(sid, val);
          try {
            if (val) localStorage.setItem(lastLineStorageKey(sid), val);
            else localStorage.removeItem(lastLineStorageKey(sid));
          } catch {
            // ignore storage issues
          }
        }

        function scheduleLastLineSave(sid, text) {
          if (!sid) return;
          const prev = lastLineSaveTimers.get(sid);
          if (prev) clearTimeout(prev);
          const t = setTimeout(() => {
            lastLineSaveTimers.delete(sid);
            saveLastLineToStorage(sid, text);
          }, 200);
          lastLineSaveTimers.set(sid, t);
        }

        function clearLastLineForSession(sid) {
          if (!sid) return;
          const prev = lastLineSaveTimers.get(sid);
          if (prev) clearTimeout(prev);
          lastLineSaveTimers.delete(sid);
          lastLineBySession.delete(sid);
          try {
            localStorage.removeItem(lastLineStorageKey(sid));
          } catch {
            // ignore
          }
        }

        function userSummaryStorageKey(sid) {
          return `codexweb.summary.user.${sid}`;
        }

        function loadUserSummaryFromStorage(sid) {
          if (!sid) return "";
          if (userSummaryBySession.has(sid)) return userSummaryBySession.get(sid) || "";
          let raw = "";
          try {
            raw = localStorage.getItem(userSummaryStorageKey(sid)) || "";
          } catch {
            raw = "";
          }
          userSummaryBySession.set(sid, raw);
          return raw;
        }

        function saveUserSummaryToStorage(sid, text) {
          if (!sid) return;
          const val = String(text || "");
          userSummaryBySession.set(sid, val);
          try {
            if (val) localStorage.setItem(userSummaryStorageKey(sid), val);
            else localStorage.removeItem(userSummaryStorageKey(sid));
          } catch {
            // ignore storage issues
          }
        }

        function scheduleUserSummarySave(sid, text) {
          if (!sid) return;
          const prev = userSummarySaveTimers.get(sid);
          if (prev) clearTimeout(prev);
          const t = setTimeout(() => {
            userSummarySaveTimers.delete(sid);
            saveUserSummaryToStorage(sid, text);
          }, 200);
          userSummarySaveTimers.set(sid, t);
        }

        function clearUserSummaryForSession(sid) {
          if (!sid) return;
          const prev = userSummarySaveTimers.get(sid);
          if (prev) clearTimeout(prev);
          userSummarySaveTimers.delete(sid);
          userSummaryBySession.delete(sid);
          try {
            localStorage.removeItem(userSummaryStorageKey(sid));
          } catch {
            // ignore
          }
        }

        function formatUserSummaryText(text, sid) {
          const res = sanitizeUserText(text, sid);
          if (res.dropped) return "";
          let t = res.text.replace(/\s+/g, " ").trim();
          if (!t) return "";
          const maxLen = 220;
          if (t.length > maxLen) {
            t = t.slice(0, Math.max(0, maxLen - 3)) + "...";
          }
          return t;
        }

        function updateUserSummaryFromEvent(ev, sid) {
          if (!sid || !ev || ev.pending || ev.role !== "user") return;
          const line = formatUserSummaryText(ev.text, sid);
          if (!line) return;
          scheduleUserSummarySave(sid, line);
        }

        function updateUserSummaryFromText(sid, text) {
          if (!sid) return;
          const line = formatUserSummaryText(text, sid);
          if (!line) return;
          scheduleUserSummarySave(sid, line);
        }

        function updateUserSummaryFromEvents(events, sid) {
          if (!sid || !Array.isArray(events) || !events.length) return;
          let best = null;
          let bestTs = -1;
          for (const ev of events) {
            if (!ev || ev.role !== "user") continue;
            if (ev.pending) continue;
            const ts = typeof ev.ts === "number" && Number.isFinite(ev.ts) ? ev.ts : null;
            if (ts !== null) {
              if (ts > bestTs) {
                bestTs = ts;
                best = ev;
              }
            } else {
              best = ev;
            }
          }
          if (!best) return;
          const line = formatUserSummaryText(best.text, sid);
          if (!line) return;
          saveUserSummaryToStorage(sid, line);
        }

        function formatLastLine(ev, sid) {
          if (!ev || typeof ev.text !== "string") return "";
          let text = ev.text;
          if (ev.role === "user") {
            const res = sanitizeUserText(text, sid);
            if (res.dropped) return "";
            text = res.text;
          }
          text = text.replace(/\s+/g, " ").trim();
          if (!text) return "";
          const prefix = ev.role === "user" ? "You: " : "Assistant: ";
          const maxLen = 280;
          if (text.length > maxLen) {
            text = text.slice(0, Math.max(0, maxLen - 3)) + "...";
          }
          return prefix + text;
        }

        function updateLastLineForEvent(ev, sid) {
          if (!sid || !ev || ev.pending) return;
          if (ev.role !== "user" && ev.role !== "assistant") return;
          const line = formatLastLine(ev, sid);
          if (!line) return;
          scheduleLastLineSave(sid, line);
          if (sid === selected) setLastLine(line);
        }

        function updateLastLineFromEvents(events, sid) {
          if (!sid || !Array.isArray(events) || !events.length) return;
          let best = null;
          let bestTs = -1;
          for (const ev of events) {
            if (!ev || ev.role !== "user" && ev.role !== "assistant") continue;
            if (ev.pending) continue;
            const ts = typeof ev.ts === "number" && Number.isFinite(ev.ts) ? ev.ts : null;
            if (ts !== null) {
              if (ts > bestTs) {
                bestTs = ts;
                best = ev;
              }
            } else if (best === null) {
              best = ev;
            } else {
              best = ev;
            }
          }
          if (!best) return;
          const line = formatLastLine(best, sid);
          if (!line) return;
          saveLastLineToStorage(sid, line);
          if (sid === selected) setLastLine(line);
        }

        function cacheStorageKey(sid) {
          return `codexweb.cache.v4.${sid}`;
        }

        function normalizeCacheEvent(ev) {
          if (!ev || (ev.role !== "user" && ev.role !== "assistant")) return null;
          if (typeof ev.text !== "string" || !ev.text.trim()) return null;
          const out = { role: ev.role, text: ev.text };
          if (typeof ev.ts === "number" && Number.isFinite(ev.ts)) out.ts = ev.ts;
          return out;
        }

        function loadCacheFromStorage(sid) {
          if (!sid || cacheLoaded.has(sid)) return;
          cacheLoaded.add(sid);
          try {
            const raw = localStorage.getItem(cacheStorageKey(sid));
            if (!raw) return;
            const obj = JSON.parse(raw);
            if (!obj || typeof obj !== "object") return;
            const eventsIn = Array.isArray(obj.events) ? obj.events : [];
            const events = [];
            for (const ev of eventsIn) {
              const norm = normalizeCacheEvent(ev);
              if (norm) events.push(norm);
            }
            if (!events.length) return;
            if (events.length > CACHE_LIMIT) events.splice(0, events.length - CACHE_LIMIT);
            const cache = {
              log_path: typeof obj.log_path === "string" ? obj.log_path : null,
              offset: Number(obj.offset) || 0,
              older_before: Number(obj.older_before) || 0,
              has_older: Boolean(obj.has_older),
              events,
            };
            cacheBySession.set(sid, cache);
          } catch {
            // ignore corrupted cache
          }
        }

        function getCache(sid) {
          if (!sid) return null;
          loadCacheFromStorage(sid);
          return cacheBySession.get(sid) || null;
        }

        function saveCacheNow(sid) {
          if (!sid) return;
          const cache = cacheBySession.get(sid);
          if (!cache) {
            localStorage.removeItem(cacheStorageKey(sid));
            return;
          }
          const payload = {
            log_path: cache.log_path || null,
            offset: Number(cache.offset) || 0,
            older_before: Number(cache.older_before) || 0,
            has_older: Boolean(cache.has_older),
            events: Array.isArray(cache.events) ? cache.events : [],
          };
          try {
            localStorage.setItem(cacheStorageKey(sid), JSON.stringify(payload));
          } catch {
            // ignore quota issues
          }
        }

        function scheduleCacheSave(sid) {
          if (!sid) return;
          const existing = cacheSaveTimers.get(sid);
          if (existing) clearTimeout(existing);
          const t = setTimeout(() => {
            cacheSaveTimers.delete(sid);
            saveCacheNow(sid);
          }, 400);
          cacheSaveTimers.set(sid, t);
        }

        function setCacheMeta(sid, { logPath, offset: off, olderBefore, hasOlder } = {}) {
          if (!sid) return;
          const cache =
            getCache(sid) || { log_path: null, offset: 0, older_before: 0, has_older: false, events: [] };
          if (logPath !== undefined) cache.log_path = logPath || null;
          if (typeof off === "number" && Number.isFinite(off)) cache.offset = off;
          if (typeof olderBefore === "number" && Number.isFinite(olderBefore)) cache.older_before = olderBefore;
          if (typeof hasOlder === "boolean") cache.has_older = hasOlder;
          cacheBySession.set(sid, cache);
          scheduleCacheSave(sid);
        }

        function replaceCacheEvents(sid, events) {
          if (!sid) return;
          const cache =
            getCache(sid) || { log_path: null, offset: 0, older_before: 0, has_older: false, events: [] };
          const out = [];
          for (const ev of events || []) {
            const norm = normalizeCacheEvent(ev);
            if (norm) out.push(norm);
          }
          if (out.length > CACHE_LIMIT) out.splice(0, out.length - CACHE_LIMIT);
          cache.events = out;
          cacheBySession.set(sid, cache);
          scheduleCacheSave(sid);
        }

        function appendCacheEvents(sid, events) {
          if (!sid || !events || !events.length) return;
          const cache =
            getCache(sid) || { log_path: null, offset: 0, older_before: 0, has_older: false, events: [] };
          const list = Array.isArray(cache.events) ? cache.events : [];
          for (const ev of events) {
            const norm = normalizeCacheEvent(ev);
            if (norm) list.push(norm);
          }
          if (list.length > CACHE_LIMIT) list.splice(0, list.length - CACHE_LIMIT);
          cache.events = list;
          cacheBySession.set(sid, cache);
          scheduleCacheSave(sid);
        }

        function clearCache(sid) {
          if (!sid) return;
          cacheBySession.delete(sid);
          cacheLoaded.delete(sid);
          cacheSaveTimers.delete(sid);
          localStorage.removeItem(cacheStorageKey(sid));
        }

        function queueEditorActive() {
          if (queueViewer.style.display !== "flex") return false;
          const active = document.activeElement;
          return Boolean(active && active.classList && active.classList.contains("queueText"));
        }

        function updateQueueBadge() {
          if (!queueBadgeEl) return;
          if (!selected) {
            queueBadgeEl.textContent = "";
            queueBadgeEl.style.display = "none";
            return;
          }
          const q = getDeferredQueue(selected);
          const n = q.length;
          if (n > 0) {
            queueBadgeEl.textContent = String(n);
            queueBadgeEl.style.display = "inline-flex";
          } else {
            queueBadgeEl.textContent = "";
            queueBadgeEl.style.display = "none";
          }
          // Avoid tearing down the queue editor while the user is typing (IME-safe).
          if (queueViewer.style.display === "flex" && !queueEditorActive()) {
            renderQueueList();
          }
        }

        function queueLocalMessage(raw, { front = false } = {}) {
          if (!selected) return;
          markDeferGate(selected);
          const q = getDeferredQueue(selected);
          if (front) q.unshift(raw);
          else q.push(raw);
          saveDeferredToStorage(selected);
          updateQueueBadge();
          setToast(`queued locally (${q.length})`);
        }

        function maybeSendDeferred({ force = false } = {}) {
          if (!selected) return;
          if (sending || deferInFlight) return;
          if (!force && currentRunning) return;
          if (deferGateBySession.get(selected)) return;
          const q = getDeferredQueue(selected);
          if (!q.length) {
            updateQueueBadge();
            return;
          }
          const raw = q.shift();
          saveDeferredToStorage(selected);
          deferInFlight = true;
          updateQueueBadge();
          void sendText(raw, { deferred: true });
        }

        function trackDeferredIdle(sid, { idle }) {
          if (!sid) return;
          if (!deferGateBySession.get(sid)) {
            deferIdleSinceBySession.delete(sid);
            return;
          }
          const gateSince = deferGateSinceBySession.get(sid);
          if (typeof gateSince === "number" && Number.isFinite(gateSince)) {
            if (performance.now() - gateSince < DEFER_GATE_MIN_MS) {
              deferIdleSinceBySession.delete(sid);
              return;
            }
          }
          if (!idle) {
            deferIdleSinceBySession.delete(sid);
            return;
          }
          const lastAt = lastActivityBySession.get(sid);
          if (typeof lastAt === "number" && Number.isFinite(lastAt)) {
            if (performance.now() - lastAt < DEFER_IDLE_RELEASE_MS) {
              deferIdleSinceBySession.delete(sid);
              return;
            }
          }
          const now = performance.now();
          const t0 = deferIdleSinceBySession.get(sid);
          if (t0 == null) {
            deferIdleSinceBySession.set(sid, now);
            return;
          }
          if (now - t0 < DEFER_IDLE_RELEASE_MS) return;
          deferIdleSinceBySession.delete(sid);
          clearDeferGate(sid);
          if (sid === selected) {
            maybeSendDeferred({ force: true });
          }
        }

        function markDeferGate(sid) {
          if (!sid) return;
          deferGateBySession.set(sid, true);
          if (!deferGateSinceBySession.has(sid)) {
            deferGateSinceBySession.set(sid, performance.now());
          }
          deferIdleSinceBySession.delete(sid);
        }

        function clearDeferGate(sid) {
          if (!sid) return;
          deferGateBySession.delete(sid);
          deferIdleSinceBySession.delete(sid);
          deferGateSinceBySession.delete(sid);
        }

          function markClickFirstPaint() {
            if (!clickMetricPending) return;
            clickMetricPending = false;
            const dt = performance.now() - clickLoadT0;
            pushPerfSample("click_to_first_message_ms", dt);
          }

	        function ensureTypingRow() {
	          if (typingRow && typingRow.isConnected) return typingRow;
	          const row = el("div", { class: "msg-row assistant typing-row" });
	          row.dataset.role = "assistant";
	          const bubble = el("div", { class: "msg assistant typing" });
	          const dots = el("div", { class: "typingDots", "aria-label": "Running", title: "Running" }, [
	            el("span", { class: "typingDot" }),
	            el("span", { class: "typingDot" }),
	            el("span", { class: "typingDot" }),
	          ]);
	          bubble.appendChild(dots);
	          row.appendChild(bubble);
	          typingRow = row;
	          return row;
	        }

	        function setTyping(show) {
	          if (!show) {
	            if (typingRow && typingRow.isConnected) typingRow.remove();
	            return;
	          }
	          const row = ensureTypingRow();
	          if (!row.isConnected) {
	            chatInner.insertBefore(row, bottomSentinel);
	          } else if (row.nextSibling !== bottomSentinel) {
	            chatInner.insertBefore(row, bottomSentinel);
	          }
	          if (autoScroll) requestAnimationFrame(() => scrollToBottom());
	        }

        function isNearBottom() {
          const thresholdPx = 80;
          return chat.scrollHeight - (chat.scrollTop + chat.clientHeight) <= thresholdPx;
        }

        function scrollToBottom() {
          bottomSentinel.scrollIntoView({ block: "end" });
          lastScrollTop = chat.scrollTop;
        }

        function ymd(d) {
          const y = String(d.getFullYear()).padStart(4, "0");
          const m = String(d.getMonth() + 1).padStart(2, "0");
          const day = String(d.getDate()).padStart(2, "0");
          return `${y}-${m}-${day}`;
        }

        function dayLabel(d) {
          const today = new Date();
          const a = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
          const b = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
          const diffDays = Math.round((a - b) / 86400000);
          const date = ymd(d);
          if (diffDays === 0) return `Today (${date})`;
          if (diffDays === 1) return `Yesterday (${date})`;
          return date;
        }

        function time24(d) {
          const hh = String(d.getHours()).padStart(2, "0");
          const mm = String(d.getMinutes()).padStart(2, "0");
          return `${hh}:${mm}`;
        }

        function rebuildDecorations({ preserveScroll }) {
          const oldTop = chat.scrollTop;
          const oldH = chat.scrollHeight;

          for (const n of Array.from(chatInner.querySelectorAll(".day-sep"))) n.remove();

          const rows = Array.from(chatInner.querySelectorAll(".msg-row"));
          let prevRole = null;
          let prevDay = null;
          let lastDay = null;

          for (const row of rows) {
            const role = row.classList.contains("user") ? "user" : "assistant";
            const ts = Number(row.dataset.ts || "0");
            const day = ts ? ymd(new Date(ts * 1000)) : null;

            row.classList.remove("grouped");
            if (prevRole === role && prevDay && day && prevDay === day) row.classList.add("grouped");
            prevRole = role;
            prevDay = day;

            if (day && day !== lastDay) {
              const d = new Date(ts * 1000);
              const sep = el("div", { class: "day-sep", text: dayLabel(d) });
              sep.dataset.day = day;
              chatInner.insertBefore(sep, row);
              lastDay = day;
            }
          }

          if (preserveScroll) {
            chat.scrollTop = oldTop + (chat.scrollHeight - oldH);
          }
          if (autoScroll) {
            requestAnimationFrame(() => scrollToBottom());
            jumpBtn.style.display = "none";
          } else {
            jumpBtn.style.display = "inline-flex";
          }
        }

        function trimRenderedRows({ fromTop }) {
          const rows = Array.from(chatInner.querySelectorAll(".msg-row")).filter((x) => !x.classList.contains("typing-row"));
          if (rows.length <= CHAT_DOM_WINDOW) return;
          const extra = rows.length - CHAT_DOM_WINDOW;
          if (fromTop) {
            for (const row of rows.slice(0, extra)) row.remove();
          } else {
            for (const row of rows.slice(rows.length - extra)) row.remove();
          }
        }

        function makeRow(ev, { ts, pending }) {
          const role = ev.role === "user" ? "user" : "assistant";
          const row = el("div", { class: `msg-row ${role}` });
          row.dataset.role = role;
          if (typeof ts === "number" && Number.isFinite(ts)) row.dataset.ts = String(ts);

          const bubble = el("div", { class: role === "user" ? "msg user" : "msg assistant" });
          const mdEl = el("div", { class: "md", html: mdToHtmlCached(ev.text) });
          bubble.appendChild(mdEl);
          renderMermaidIn(mdEl);
          if (typeof ts === "number" && Number.isFinite(ts)) bubble.appendChild(el("div", { class: "ts", text: time24(new Date(ts * 1000)) }));

          if (pending) {
            bubble.style.opacity = "0.72";
            bubble.setAttribute("data-pending", "1");
            if (ev.localId) bubble.setAttribute("data-local-id", String(ev.localId));
          }

          row.appendChild(bubble);
          return { row, bubble };
        }

        function normalizeTextForPendingMatch(s) {
          // Normalize common platform newline differences to improve pending->ack reconciliation.
          return String(s || "").replace(/\r\n/g, "\n").replace(/\r/g, "\n");
        }

        function eventKey(ev) {
          if (!ev || (ev.role !== "user" && ev.role !== "assistant")) return "";
          const text = typeof ev.text === "string" ? ev.text : "";
          const ts = typeof ev.ts === "number" && Number.isFinite(ev.ts) ? ev.ts : 0;
          return `${ev.role}|${ts}|${text}`;
        }

        function eventSig(ev) {
          if (!ev || (ev.role !== "user" && ev.role !== "assistant")) return "";
          const raw = typeof ev.text === "string" ? ev.text : "";
          const text = normalizeTextForPendingMatch(raw);
          return `${ev.role}|${text}`;
        }

        function eventTs(ev) {
          return typeof ev.ts === "number" && Number.isFinite(ev.ts) ? ev.ts : null;
        }

        function markEventSeen(ev) {
          const key = eventKey(ev);
          if (!key) return;
          if (recentEventKeySet.has(key)) return;
          recentEventKeySet.add(key);
          recentEventKeys.push(key);
          if (recentEventKeys.length > RECENT_EVENT_KEYS_MAX) {
            const drop = recentEventKeys.splice(0, recentEventKeys.length - RECENT_EVENT_KEYS_MAX);
            for (const k of drop) recentEventKeySet.delete(k);
          }
          const sig = eventSig(ev);
          const ts = eventTs(ev);
          if (sig && typeof ts === "number") {
            recentEventSigTs.set(sig, ts);
            recentEventSigOrder.push({ sig, ts });
            if (recentEventSigOrder.length > RECENT_EVENT_SIG_MAX) {
              const drop = recentEventSigOrder.splice(0, recentEventSigOrder.length - RECENT_EVENT_SIG_MAX);
              for (const item of drop) {
                if (recentEventSigTs.get(item.sig) === item.ts) {
                  recentEventSigTs.delete(item.sig);
                }
              }
            }
          }
        }

        function isDuplicateEvent(ev) {
          const key = eventKey(ev);
          if (!key) return false;
          if (recentEventKeySet.has(key)) return true;
          const sig = eventSig(ev);
          const ts = eventTs(ev);
          if (!sig || typeof ts !== "number") return false;
          const last = recentEventSigTs.get(sig);
          if (typeof last !== "number") return false;
          return Math.abs(ts - last) * 1000 <= RECENT_EVENT_SIG_WINDOW_MS;
        }

        function pendingMatchKey(s) {
          // Codex log serialization can trim trailing whitespace/newlines; match on a slightly
          // normalized key to avoid duplicating the optimistic local echo bubble.
          const t = normalizeTextForPendingMatch(s);
          return t.replace(/[ \t]+$/gm, "").replace(/\s+$/, "");
        }

        function consumePendingUserIfMatches(ev) {
          if (ev.role !== "user" || ev.pending) return false;
          const key = pendingMatchKey(ev.text);
          const loose = normalizeTextForPendingMatch(ev.text);
          const evTs = typeof ev.ts === "number" && Number.isFinite(ev.ts) ? ev.ts : null;
          const candidates = [];
          for (let i = 0; i < pendingUser.length; i++) {
            const x = pendingUser[i];
            if (x.key === key || x.loose === loose) candidates.push({ i, x });
          }
          if (!candidates.length) return false;
          let best = candidates[0];
          if (evTs !== null) {
            let bestD = Math.abs(evTs - (best.x.t0 || evTs));
            for (const c of candidates.slice(1)) {
              const d = Math.abs(evTs - (c.x.t0 || evTs));
              if (d < bestD) {
                best = c;
                bestD = d;
              }
            }
          }
          const idx = best.i;
          if (idx < 0) return false;
          const { id } = pendingUser[idx];
          pendingUser.splice(idx, 1);
          const pendingEl = chatInner.querySelector(`.msg.user[data-local-id="${id}"]`);
          if (!pendingEl) return true;

          pendingEl.style.opacity = "1";
          pendingEl.removeAttribute("data-local-id");
          pendingEl.removeAttribute("data-pending");

          const mdEl = pendingEl.querySelector(".md");
          if (mdEl && typeof ev.text === "string") {
            mdEl.innerHTML = mdToHtmlCached(ev.text);
            renderMermaidIn(mdEl);
          }

          const row = pendingEl.closest(".msg-row");
          if (row && typeof ev.ts === "number" && Number.isFinite(ev.ts)) row.dataset.ts = String(ev.ts);
          const tsEl = pendingEl.querySelector(".ts");
          if (tsEl && typeof ev.ts === "number" && Number.isFinite(ev.ts)) tsEl.textContent = time24(new Date(ev.ts * 1000));
          rebuildDecorations({ preserveScroll: true });
          return true;
        }

        function isMobile() {
          return window.matchMedia && window.matchMedia("(max-width: 880px)").matches;
        }

        function setSidebarOpen(open) {
          if (open) {
            document.body.classList.add("sidebar-open");
            localStorage.setItem("codexweb.sidebarOpen", "1");
          } else {
            document.body.classList.remove("sidebar-open");
            localStorage.removeItem("codexweb.sidebarOpen");
          }
        }

        function setSidebarCollapsed(collapsed) {
          if (collapsed) {
            document.body.classList.add("sidebar-collapsed");
            localStorage.setItem("codexweb.sidebarCollapsed", "1");
          } else {
            document.body.classList.remove("sidebar-collapsed");
            localStorage.removeItem("codexweb.sidebarCollapsed");
          }
        }

        function getLastAssistantTs(s) {
          const v = Number(s && s.last_assistant_ts);
          return Number.isFinite(v) ? v : 0;
        }

        function isSessionUnread(s) {
          if (!s || !s.session_id) return false;
          if (selected === s.session_id) return false;
          const last = getLastAssistantTs(s);
          if (!last) return false;
          const seen = loadSeenAssistantTs(s.session_id);
          return last > seen + 0.0001;
        }

        function workspaceHasSelected(ws) {
          if (!selected) return false;
          return ws.sessions.some((s) => s && s.session_id === selected);
        }

        function workspacePreferredSessionId(ws) {
          if (workspaceHasSelected(ws)) return selected;
          const first = ws.sessions[0];
          return first ? first.session_id : null;
        }

        function collectWorkspaceFiles(ws) {
          const out = [];
          const seen = new Set();
          for (const s of ws.sessions) {
            const files = listFromFilesField(s && s.files);
            for (const p of files) {
              if (seen.has(p)) continue;
              seen.add(p);
              out.push(p);
            }
          }
          return out;
        }

        function buildWorkspaceFiles(ws, files) {
          if (!files.length) return null;
          const maxShow = 5;
          const show = files.slice(0, maxShow);
          const fileRows = show.map((p) => {
            const btn = el("button", { class: "workspaceFile", title: p, "aria-label": `Open ${p}`, text: baseName(p) || p });
            bindTap(btn, () => {
              const sid = workspacePreferredSessionId(ws);
              if (sid && sid !== selected) void selectSession(sid);
              showFileViewer();
              filePathInput.value = p;
              openFilePath();
            });
            return btn;
          });
          const more = files.length > maxShow ? el("div", { class: "workspaceFilesMore muted", text: `+${files.length - maxShow} more` }) : null;
          return el("div", { class: "workspaceFiles" }, [
            el("div", { class: "workspaceFilesLabel", text: "Files" }),
            ...fileRows,
            ...(more ? [more] : []),
          ]);
        }

        function buildSessionCard(s) {
          const badge = el("span", { class: "badge" + (s.busy ? " busy" : ""), text: s.busy ? "busy" : "idle" });
          const q = s.queue_len ? el("span", { class: "badge queue", text: `queue ${s.queue_len}` }) : null;
          const card = el("div", { class: "session" + (selected === s.session_id ? " active" : "") });

          const title = sessionDisplayName(s);
          const badges = [];
          if (s.harness_enabled) badges.push(el("span", { class: "badge harness", text: "harness", title: "Harness mode enabled" }));
          badges.push(badge);
          if (q) badges.push(q);
          if (isSessionUnread(s)) badges.push(el("span", { class: "unreadDot", title: "Unread response" }));
          let delBtn = null;
          const renameCardBtn = el("button", {
            class: "icon-btn",
            title: "Rename session",
            "aria-label": "Rename session",
            type: "button",
            html: iconSvg("edit"),
          });
          renameCardBtn.onclick = (e) => {
            e.preventDefault();
            e.stopPropagation();
            void renameSessionId(s.session_id);
          };
          if (s.owned) {
            delBtn = el("button", {
              class: "icon-btn danger sessionDel",
              title: "Delete session",
              "aria-label": "Delete session",
              type: "button",
              html: iconSvg("trash"),
            });
            delBtn.onclick = async (e) => {
              e.preventDefault();
              e.stopPropagation();
              if (!confirm("Delete this session?")) return;
              try {
                await api(`/api/sessions/${s.session_id}/delete`, { method: "POST", body: {} });
                clearCache(s.session_id);
                clearDraftForSession(s.session_id);
                clearSeenAssistantForSession(s.session_id);
                clearLastLineForSession(s.session_id);
                clearUserSummaryForSession(s.session_id);
                if (selected === s.session_id) {
                  selected = null;
                  offset = 0;
                  activeLogPath = null;
                  activeThreadId = null;
                  turnOpen = false;
                  localStorage.removeItem("codexweb.selected");
                  titleLabel.textContent = "No session selected";
                  setStatus({ running: false, queueLen: 0 });
                  setContext(null);
                  setTyping(false);
                  setAttachCount(0);
                  resetChatRenderState();
                  updateQueueBadge();
                  if (harnessMenuOpen) hideHarnessMenu();
                  updateHarnessBtnState();
                }
                await refreshSessions();
              } catch (err) {
                setToast(`delete error: ${err.message}`);
              }
            };
          }
          const top = el("div", { class: "row" }, [
            el("div", { class: "titleLine", text: title, title: s.cwd || "" }),
            el("div", { class: "sessionBadges" }, badges),
          ]);
          const updatedTs = typeof s.updated_ts === "number" && Number.isFinite(s.updated_ts) ? s.updated_ts : s.start_ts;
          const meta = el("div", { class: "muted subLine", text: updatedTs ? `last ${fmtTs(updatedTs)}` : "" });
          const summary = loadUserSummaryFromStorage(s.session_id);
          const summaryEl = summary
            ? el("div", { class: "sessionSummary muted", text: summary, title: summary })
            : null;
          const mainCol = el("div", { class: "sessionMain" }, [top, meta, ...(summaryEl ? [summaryEl] : [])]);
          // Session file lists are shown at the workspace level instead.
          card.appendChild(mainCol);
          const actionButtons = [renameCardBtn];
          const dupBtn = el("button", {
            class: "icon-btn",
            title: "Duplicate session",
            "aria-label": "Duplicate session",
            type: "button",
            html: iconSvg("duplicate"),
          });
          dupBtn.onclick = async (e) => {
            e.preventDefault();
            e.stopPropagation();
            const cwd = s && s.cwd && s.cwd !== "?" ? s.cwd : "";
            if (!cwd) {
              setToast("cwd unavailable");
              return;
            }
            const base = sessionDisplayName(s) || baseName(cwd) || "Session";
            const alias = buildDuplicateAlias(base);
            await spawnSessionWithCwd(cwd, { alias });
          };
          actionButtons.unshift(dupBtn);
          if (delBtn) actionButtons.push(delBtn);
          card.appendChild(el("div", { class: "sessionAction" }, actionButtons));
          card.onclick = () => {
            if (isMobile()) setSidebarOpen(false);
            selectSession(s.session_id);
          };
          return card;
        }

	        async function refreshSessions() {
	          const data = await api("/api/sessions");
	          sessionsWrap.innerHTML = "";
	          sessionIndex = new Map();
	          const sessions = (data.sessions || [])
              .slice()
              .sort((a, b) => (b.updated_ts || b.start_ts || 0) - (a.updated_ts || a.start_ts || 0));
            for (const s of sessions) {
              sessionIndex.set(s.session_id, s);
            }
            const workspaces = buildWorkspaces(sessions);
            for (const ws of workspaces) {
              const title = workspaceTitleParts(ws.cwd);
              const countLabel = `${ws.sessions.length} session${ws.sessions.length === 1 ? "" : "s"}`;
              const header = el("div", { class: "workspaceHeader" }, [
                el("div", { class: "workspaceTitleRow" }, [
                  el("div", { class: "workspaceTitle", text: title.title, title: ws.cwd || "Unknown cwd" }),
                  el("div", { class: "workspaceMeta muted", text: countLabel }),
                ]),
                title.subtitle ? el("div", { class: "workspacePath muted", text: title.subtitle, title: title.subtitle }) : null,
              ].filter(Boolean));
              const files = collectWorkspaceFiles(ws);
              const filesWrap = buildWorkspaceFiles(ws, files);
              const sessionWrap = el("div", { class: "workspaceSessions" });
              for (const s of ws.sessions) {
                sessionWrap.appendChild(buildSessionCard(s));
              }
              const wsEl = el("div", { class: "workspace" + (workspaceHasSelected(ws) ? " active" : "") });
              wsEl.appendChild(header);
              if (filesWrap) wsEl.appendChild(filesWrap);
              wsEl.appendChild(sessionWrap);
              sessionsWrap.appendChild(wsEl);
            }
          if (selected && !sessionIndex.has(selected)) {
            selected = null;
            offset = 0;
            activeLogPath = null;
            activeThreadId = null;
            pollGen += 1;
            if (pollTimer) clearTimeout(pollTimer);
            pollTimer = null;
            pollKickPending = false;
            localStorage.removeItem("codexweb.selected");
            titleLabel.textContent = "No session selected";
            setStatus({ running: false, queueLen: 0 });
            setTyping(false);
            resetChatRenderState();
            turnOpen = false;
            setLastLine("");
            if (harnessMenuOpen) hideHarnessMenu();
            updateHarnessBtnState();
            updateQueueBadge();
          } else if (selected) {
            const s = sessionIndex.get(selected);
            if (s) titleLabel.textContent = sessionTitleWithId(s);
          }
          updateHarnessBtnState();
          updateQueueBadge();
          return sessions;
        }

        function appendEvent(ev) {
          const clean = sanitizeUserEvent(ev, selected);
          if (!clean) return;
          ev = clean;
          if (consumePendingUserIfMatches(ev)) return;
          if (!ev.pending && isDuplicateEvent(ev)) return;

          if (!ev.pending && selected) {
            lastActivityBySession.set(selected, performance.now());
          }
          const stick = autoScroll || isNearBottom();
          const ts = typeof ev.ts === "number" && Number.isFinite(ev.ts) ? ev.ts : ev.pending ? Date.now() / 1000 : null;
	          const { row } = makeRow(ev, { ts, pending: Boolean(ev.pending) });
	          const anchor = typingRow && typingRow.isConnected ? typingRow : bottomSentinel;
	          chatInner.insertBefore(row, anchor);
            trimRenderedRows({ fromTop: true });
          rebuildDecorations({ preserveScroll: false });
            if (!ev.pending) markClickFirstPaint();
            if (!ev.pending && selected) {
              appendCacheEvents(selected, [ev]);
            }
          if (!ev.pending) markEventSeen(ev);
          if (!ev.pending && selected && ev.role === "assistant" && typeof ev.ts === "number" && Number.isFinite(ev.ts)) {
            markAssistantSeen(selected, ev.ts);
          }
          if (selected) updateLastLineForEvent(ev, selected);
          if (selected) updateUserSummaryFromEvent(ev, selected);

          if (stick) {
            requestAnimationFrame(() => scrollToBottom());
            jumpBtn.style.display = "none";
          } else {
            jumpBtn.style.display = "inline-flex";
          }
        }

        function prependOlderEvents(allEvents) {
          const msgs = [];
          for (const ev of allEvents) {
            if (!ev || (ev.role !== "user" && ev.role !== "assistant")) continue;
            const clean = sanitizeUserEvent(ev, selected);
            if (!clean) continue;
            msgs.push(clean);
          }
          if (!msgs.length) return;
          const oldTop = chat.scrollTop;
          const oldH = chat.scrollHeight;
          const frag = document.createDocumentFragment();
          for (const ev of msgs) {
            const ts = typeof ev.ts === "number" && Number.isFinite(ev.ts) ? ev.ts : null;
            frag.appendChild(makeRow(ev, { ts, pending: false }).row);
          }
          const firstMsg = chatInner.querySelector(".msg-row:not(.typing-row)");
          const anchor = firstMsg || (typingRow && typingRow.isConnected ? typingRow : bottomSentinel);
          chatInner.insertBefore(frag, anchor);
          trimRenderedRows({ fromTop: false });
          rebuildDecorations({ preserveScroll: true });
          chat.scrollTop = oldTop + (chat.scrollHeight - oldH);
        }

        async function loadOlderMessages({ auto = false } = {}) {
          if (!selected || !hasOlder || loadingOlder) return;
          if (auto) {
            const now = performance.now();
            if (now - olderAutoTriggerAt < OLDER_AUTO_COOLDOWN_MS) return;
            olderAutoTriggerAt = now;
          }
          const sid = selected;
          const gen = pollGen;
          setOlderState({ hasMore: hasOlder, isLoading: true });
          try {
            const reqBefore = Math.max(0, Number(olderBefore) || 0);
            const data = await api(`/api/sessions/${sid}/messages?offset=0&init=1&limit=${olderPageLimit()}&before=${reqBefore}`);
            if (selected !== sid || pollGen !== gen) return;
            const evs = Array.isArray(data.events) ? data.events : [];
            if (evs.length) prependOlderEvents(evs);
            olderBefore = Number.isFinite(Number(data.next_before)) ? Number(data.next_before) : reqBefore;
            setOlderState({ hasMore: Boolean(data.has_older), isLoading: false });
            setCacheMeta(sid, { olderBefore, hasOlder: Boolean(data.has_older) });
          } catch {
            if (selected !== sid || pollGen !== gen) return;
            setOlderState({ hasMore: hasOlder, isLoading: false });
          }
        }

        function maybeAutoLoadOlder() {
          if (chat.scrollTop > 1) return;
          void loadOlderMessages({ auto: true });
        }

        function startInitialRender(allEvents) {
          backfillToken += 1;
          const myToken = backfillToken;
          if (selected) lastActivityBySession.delete(selected);

          const msgs = [];
          let latestAssistantTs = null;
          recentEventKeys.length = 0;
          recentEventKeySet.clear();
          recentEventSigTs.clear();
          recentEventSigOrder.length = 0;
          for (const ev of allEvents) {
		            if (!ev || (ev.role !== "user" && ev.role !== "assistant")) continue;
		            const clean = sanitizeUserEvent(ev, selected);
		            if (!clean) continue;
		            if (consumePendingUserIfMatches(clean)) continue;
                if (isDuplicateEvent(clean)) continue;
                markEventSeen(clean);
		            msgs.push(clean);
                if (clean.role === "assistant" && typeof clean.ts === "number" && Number.isFinite(clean.ts)) {
                  latestAssistantTs =
                    latestAssistantTs === null ? clean.ts : Math.max(latestAssistantTs, clean.ts);
                }
          }
          if (!msgs.length) return;
          if (selected) replaceCacheEvents(selected, msgs);
          if (selected && latestAssistantTs !== null) {
            markAssistantSeen(selected, latestAssistantTs);
          }
          if (selected) {
            updateLastLineFromEvents(msgs, selected);
            updateUserSummaryFromEvents(msgs, selected);
          }
	          const frag = document.createDocumentFragment();
          for (const ev of msgs) {
            const ts = typeof ev.ts === "number" && Number.isFinite(ev.ts) ? ev.ts : null;
            frag.appendChild(makeRow(ev, { ts, pending: false }).row);
	          }
	          const anchor = typingRow && typingRow.isConnected ? typingRow : bottomSentinel;
	          chatInner.insertBefore(frag, anchor);
            trimRenderedRows({ fromTop: true });
	          rebuildDecorations({ preserveScroll: false });
            markClickFirstPaint();
	          // Ensure scroll-to-bottom happens after layout.
	          requestAnimationFrame(() => {
	            if (myToken !== backfillToken) return;
	            scrollToBottom();
	            requestAnimationFrame(() => {
	              if (myToken !== backfillToken) return;
	              scrollToBottom();
	            });
	          });
	          backfillState = null;
	        }

			        async function pollMessages(sid = selected, gen = pollGen) {
			          if (!sid) return;
			          try {
	            const prevOffset = offset;
	            const reqOffset = offset;
		            const data = await api(`/api/sessions/${sid}/messages?offset=${reqOffset}`);
	            if (gen !== pollGen || sid !== selected) return;
	            const lp = data && typeof data.log_path === "string" ? data.log_path : null;
	            const tid = data && typeof data.thread_id === "string" ? data.thread_id : null;
	            const nowBusy = Boolean(data.busy);
	            if (!activeLogPath && lp) activeLogPath = lp;
	            if (activeLogPath && !lp) {
	              activeLogPath = null;
	              activeThreadId = tid;
	              offset = 0;
	              resetChatRenderState();
	              setAttachCount(0);
	              setTyping(false);
	              turnOpen = false;
	              setOlderState({ hasMore: false, isLoading: false });
	              olderBefore = 0;
              setStatus({ running: Boolean(nowBusy), queueLen: data.queue_len });
              setContext(data.token);
              setTyping(Boolean(nowBusy));
              trackDeferredIdle(sid, { idle: !nowBusy });
              return;
            }
	            if (activeLogPath && lp && lp !== activeLogPath) {
	              activeLogPath = lp;
	              activeThreadId = tid;
	              offset = 0;
	              resetChatRenderState();
              setAttachCount(0);
              setTyping(false);
              turnOpen = false;
              setStatus({ running: false, queueLen: 0 });
              try {
                const d2 = await api(`/api/sessions/${sid}/messages?offset=0&init=1&limit=${initPageLimit()}&before=0`);
                if (gen !== pollGen || sid !== selected) return;
                if (d2 && typeof d2.log_path === "string") activeLogPath = d2.log_path;
                if (d2 && typeof d2.thread_id === "string") activeThreadId = d2.thread_id;
                offset = d2.offset;
                const evs2 = Array.isArray(d2.events) ? d2.events : [];
                if (evs2.length) startInitialRender(evs2);
                olderBefore = Number.isFinite(Number(d2.next_before)) ? Number(d2.next_before) : 0;
                setOlderState({ hasMore: Boolean(d2.has_older), isLoading: false });
                setCacheMeta(sid, {
                  logPath: activeLogPath,
                  offset,
                  olderBefore,
                  hasOlder: Boolean(d2.has_older),
                });
                const nowBusy2 = Boolean(d2.busy);
                const turnStart2 = Boolean(d2.turn_start);
                const turnEnd2 = Boolean(d2.turn_end);
                const turnAborted2 = Boolean(d2.turn_aborted);
                if (turnStart2 || nowBusy2) turnOpen = true;
                if (turnEnd2 || turnAborted2 || !nowBusy2) turnOpen = false;
                setStatus({ running: Boolean(turnOpen || nowBusy2), queueLen: d2.queue_len });
                setContext(d2.token);
                setTyping(Boolean(turnOpen || nowBusy2));
                if ((turnEnd2 || turnAborted2) && selected) {
                  clearDeferGate(selected);
                  if (getDeferredQueue(selected).length) {
                    maybeSendDeferred({ force: true });
                  }
                }
                trackDeferredIdle(sid, { idle: !(turnOpen || nowBusy2) });
                } catch (e2) {
                  console.error("poll init reload failed", e2);
                  throw e2;
                }
                return;
             }
	
		            offset = data.offset;
              setCacheMeta(sid, { logPath: activeLogPath || lp || null, offset });
	            const evs = Array.isArray(data.events) ? data.events : [];
	            if (prevOffset === 0 && !chatInner.querySelector(".msg-row:not(.typing-row)") && evs.length) {
	              startInitialRender(evs);
            } else {
              for (const ev of evs) appendEvent(ev);
            }

            const turnStart = Boolean(data.turn_start);
            const turnEnd = Boolean(data.turn_end);
            const turnAborted = Boolean(data.turn_aborted);
            if (turnStart) {
              turnOpen = true;
            }
            if (!turnOpen && nowBusy) {
              turnOpen = true;
            }

            if ((turnEnd || turnAborted) && turnOpen) {
              turnOpen = false;
            }
		            if (turnOpen && !nowBusy) {
		              turnOpen = false;
		            }

				            setStatus({ running: Boolean(turnOpen || nowBusy), queueLen: data.queue_len });
				            setContext(data.token);
				            setTyping(Boolean(turnOpen || nowBusy));
            if ((turnEnd || turnAborted) && selected) {
              clearDeferGate(selected);
              if (getDeferredQueue(selected).length) {
                maybeSendDeferred({ force: true });
              }
            }
            trackDeferredIdle(sid, { idle: !(turnOpen || nowBusy) });
            const s = sessionIndex.get(sid);
            if (s) titleLabel.textContent = sessionTitleWithId(s);
		          } catch (e) {
            if (gen !== pollGen || sid !== selected) return;
            if (e && e.status === 404) {
              selected = null;
              offset = 0;
              activeLogPath = null;
              activeThreadId = null;
              pollGen += 1;
              if (pollTimer) clearTimeout(pollTimer);
              pollTimer = null;
              pollKickPending = false;
              turnOpen = false;
              localStorage.removeItem("codexweb.selected");
              titleLabel.textContent = "No session selected";
              setStatus({ running: false, queueLen: 0 });
              setTyping(false);
              resetChatRenderState();
              updateQueueBadge();
              try {
                await refreshSessions();
              } catch (e2) {
                  console.error("refreshSessions failed after session disappeared", e2);
                  toast.textContent = `refresh error: ${e2 && e2.message ? e2.message : "unknown error"}`;
                }
                return;
            }
            toast.textContent = `error: ${e.message}`;
          }
        }

        async function pollLoop() {
          if (!selected) return;
          if (pollLoopBusy) {
            pollKickPending = true;
            return;
          }
          pollLoopBusy = true;
          const mySid = selected;
          const myGen = pollGen;
          try {
            await pollMessages(mySid, myGen);
          } finally {
            pollLoopBusy = false;
          }
          if (pollKickPending) {
            pollKickPending = false;
            kickPoll(0);
            return;
          }
          if (selected !== mySid || pollGen !== myGen) return;
          const now = Date.now();
          let nextMs = 900;
          if (now < pollFastUntilMs) nextMs = 200;
          else if (turnOpen) nextMs = 250;
          pollTimer = setTimeout(pollLoop, nextMs);
        }

        function kickPoll(ms = 0) {
          if (pollTimer) {
            clearTimeout(pollTimer);
            pollTimer = null;
          }
          if (pollLoopBusy) {
            pollKickPending = true;
            return;
          }
          pollTimer = setTimeout(pollLoop, ms);
        }

		        async function selectSession(id) {
	          pollGen += 1;
	          const myGen = pollGen;
	          if (pollTimer) {
	            clearTimeout(pollTimer);
	            pollTimer = null;
	          }
		          pollKickPending = false;
            const prevSid = selected;
            if (prevSid) {
              const prevTa = $("#msg");
              if (prevTa) saveDraftToStorage(prevSid, prevTa.value);
            }
            const sid = id;
            selected = sid;
            offset = 0;
            activeLogPath = null;
            activeThreadId = null;
            resetChatRenderState();
            setAttachCount(0);
            localStorage.setItem("codexweb.selected", sid);
            updateQueueBadge();
            setStatus({ running: false, queueLen: 0 });
            setContext(null);
            setTyping(false);
            turnOpen = false;
		          {
		            const s = sessionIndex.get(sid);
            if (s) titleLabel.textContent = sessionTitleWithId(s);
            else titleLabel.textContent = sid ? String(sid) : "No session selected";
                const lastSeenTs = getLastAssistantTs(s);
                if (lastSeenTs) saveSeenAssistantTs(sid, lastSeenTs);
                const storedLine = loadLastLineFromStorage(sid);
                setLastLine(storedLine);
		          }
                const draft = loadDraftFromStorage(sid);
                const ta = $("#msg");
                if (ta) {
                  ta.value = draft;
                  const ph = $("#msgPh");
                  if (ph) ph.style.display = ta.value ? "none" : "flex";
                  autoGrow();
                }
                clickLoadT0 = performance.now();
                clickMetricPending = true;
          if (pollGen !== myGen || selected !== sid) return;
			          const s0 = sessionIndex.get(sid);
			          if (s0 && s0.token) setContext(s0.token);
                let cached = getCache(sid);
                if (cached && Array.isArray(cached.events) && cached.events.length) {
                  const hasUser = cached.events.some((ev) => ev && ev.role === "user");
                  const hasAssistant = cached.events.some((ev) => ev && ev.role === "assistant");
                  if (!hasUser && hasAssistant) {
                    clearCache(sid);
                    cached = null;
                  }
                }
                const hasCached = Boolean(
                  cached &&
                    Array.isArray(cached.events) &&
                    cached.events.length &&
                    Number(cached.offset) > 0
                );
                try {
			            const data = await api(`/api/sessions/${sid}/messages?offset=0&init=1&limit=${initPageLimit()}&before=0`);
		            if (pollGen !== myGen || selected !== sid) return;
                  if (data && typeof data.log_path === "string") activeLogPath = data.log_path;
                  if (data && typeof data.thread_id === "string") activeThreadId = data.thread_id;
			            offset = data.offset;
			            const evs = Array.isArray(data.events) ? data.events : [];
			            if (evs.length) startInitialRender(evs);
                  olderBefore = Number.isFinite(Number(data.next_before)) ? Number(data.next_before) : 0;
                  setOlderState({ hasMore: Boolean(data.has_older), isLoading: false });
                  setCacheMeta(sid, {
                    logPath: activeLogPath,
                    offset,
                    olderBefore,
                    hasOlder: Boolean(data.has_older),
                  });
			            const nowBusy = Boolean(data.busy);
	          const turnStart = Boolean(data.turn_start);
	          const turnEnd = Boolean(data.turn_end);
	          const turnAborted = Boolean(data.turn_aborted);
			            if (turnStart || nowBusy) turnOpen = true;
			            if (turnEnd || turnAborted || !nowBusy) turnOpen = false;
			            setStatus({ running: Boolean(turnOpen || nowBusy), queueLen: data.queue_len });
			            setContext(data.token);
			            setTyping(Boolean(turnOpen || nowBusy));
		          } catch {
                if (hasCached) {
                  activeLogPath = typeof cached.log_path === "string" ? cached.log_path : null;
                  offset = Number(cached.offset) || 0;
                  olderBefore = Number(cached.older_before) || 0;
                  setOlderState({ hasMore: Boolean(cached.has_older), isLoading: false });
                  startInitialRender(cached.events);
                  try {
                    await pollMessages(sid, myGen);
                  } catch {
                    // ignore and rely on next poll
                  }
                  if (pollGen !== myGen || selected !== sid) return;
                } else {
                  await pollMessages(sid, myGen);
                  if (pollGen !== myGen || selected !== sid) return;
                }
		          }
            refreshSessions().catch((e) => console.error("refreshSessions failed", e));
           kickPoll(900);
           if (isMobile()) setSidebarOpen(false);
           updateHarnessBtnState();
         }

			        $("#refreshBtn").onclick = async () => {
                const sid = selected;
                if (!sid) {
                  await refreshSessions();
                  setToast("refreshed");
                  return;
                }
                clearCache(sid);
                try {
                  await refreshSessions();
                } catch (e) {
                  console.error("refreshSessions failed", e);
                  setToast(`refresh error: ${e && e.message ? e.message : "unknown error"}`);
                  return;
                }
                if (selected !== sid) return;
                if (sessionIndex.has(sid)) {
                  await selectSession(sid);
                }
                setToast("refreshed");
              };
        function updateHarnessBtnState() {
          const s = selected ? sessionIndex.get(selected) : null;
          const on = Boolean(s && s.harness_enabled);
          harnessBtn.disabled = !selected;
          harnessBtn.classList.toggle("active", Boolean(selected && on));
          renameBtn.disabled = !selected;
          duplicateBtn.disabled = !selected;
        }
           async function loadHarnessCfgForSelected() {
             if (!selected) return;
             const sid = selected;
              const d = await api(`/api/sessions/${sid}/harness`);
              if (selected !== sid) return;
              if (!d || typeof d !== "object") throw new Error("invalid harness response");
              if (typeof d.enabled !== "boolean") throw new Error("invalid harness.enabled");
              if (typeof d.request !== "string") throw new Error("invalid harness.request");
              harnessCfg = { enabled: d.enabled, request: d.request };
             const enabledEl = $("#harnessEnabled");
             const requestEl = $("#harnessRequest");
             if (enabledEl) enabledEl.checked = harnessCfg.enabled;
             if (requestEl) requestEl.value = harnessCfg.request;
           }
			        function scheduleHarnessSave() {
			          if (!selected) return;
			          const sid = selected;
			          if (harnessSaveTimer) clearTimeout(harnessSaveTimer);
			          harnessSaveTimer = setTimeout(async () => {
			            if (selected !== sid) return;
               try {
                 await api(`/api/sessions/${sid}/harness`, { method: "POST", body: { enabled: harnessCfg.enabled, request: harnessCfg.request } });
                 await refreshSessions();
               } catch (e) {
                 console.error("save harness failed", e);
                 setToast(`harness save error: ${e && e.message ? e.message : "unknown error"}`);
               }
               updateHarnessBtnState();
             }, 450);
           }
			        function hideHarnessMenu() {
			          harnessMenuOpen = false;
			          harnessMenu.style.display = "none";
			        }
			        async function showHarnessMenu() {
			          if (!selected) return;
			          harnessMenuOpen = true;
			          harnessMenu.style.display = "block";
			          const rect = harnessBtn.getBoundingClientRect();
			          const top = Math.min(window.innerHeight - 12, rect.bottom + 8);
			          harnessMenu.style.top = `${top}px`;
			          harnessMenu.style.left = "12px";
			          harnessMenu.style.right = "auto";
             const w = harnessMenu.offsetWidth || 320;
             const left = Math.max(12, Math.min(window.innerWidth - 12 - w, rect.right - w));
             harnessMenu.style.left = `${left}px`;
             try {
               await loadHarnessCfgForSelected();
             } catch (e) {
               console.error("load harness failed", e);
               setToast(`harness load error: ${e && e.message ? e.message : "unknown error"}`);
               hideHarnessMenu();
             }
           }
			        function toggleHarnessMenu() {
			          if (harnessMenuOpen) hideHarnessMenu();
			          else showHarnessMenu();
			        }

			        harnessBtn.onclick = (e) => {
			          e.preventDefault();
			          e.stopPropagation();
		          toggleHarnessMenu();
		        };
		        harnessMenu.onclick = (e) => e.stopPropagation();
		        if (window.__codexwebHarnessGlobalHandlers) {
		          const h = window.__codexwebHarnessGlobalHandlers;
		          if (h.docClick) document.removeEventListener("click", h.docClick);
		          if (h.resize) window.removeEventListener("resize", h.resize);
		        }
		        const onDocClick = () => {
		          if (harnessMenuOpen) hideHarnessMenu();
		        };
		        const onResize = () => {
		          if (harnessMenuOpen) hideHarnessMenu();
		        };
			        window.__codexwebHarnessGlobalHandlers = { docClick: onDocClick, resize: onResize };
			        document.addEventListener("click", onDocClick);
			        window.addEventListener("resize", onResize);
			        const harnessEnabledEl = $("#harnessEnabled");
			        const harnessRequestEl = $("#harnessRequest");
			        if (harnessEnabledEl)
			          harnessEnabledEl.onchange = (e) => {
			            if (!selected) return;
			            harnessCfg.enabled = Boolean(e.target.checked);
			            const s = sessionIndex.get(selected);
			            if (s) s.harness_enabled = harnessCfg.enabled;
			            updateHarnessBtnState();
			            scheduleHarnessSave();
			          };
        if (harnessRequestEl)
          harnessRequestEl.oninput = (e) => {
            if (!selected) return;
            harnessCfg.request = String(e.target.value ?? "");
            scheduleHarnessSave();
          };
        async function renameSessionId(sid) {
          if (!sid) return;
          const s = sessionIndex.get(sid);
          const currentAlias = s && typeof s.alias === "string" ? s.alias : "";
          const fallback = sessionDisplayName(s);
          const def = currentAlias || fallback || "";
          const next = prompt("Rename session (blank to clear):", def);
          if (next === null) return;
          try {
            const res = await api(`/api/sessions/${sid}/rename`, { method: "POST", body: { name: String(next) } });
            const alias = res && typeof res.alias === "string" ? res.alias : "";
            if (s) s.alias = alias;
            await refreshSessions();
            if (selected === sid) {
              const s2 = sessionIndex.get(sid);
              if (s2) titleLabel.textContent = sessionTitleWithId(s2);
            }
            setToast(alias ? "renamed" : "alias cleared");
          } catch (e) {
            setToast(`rename error: ${e && e.message ? e.message : "unknown error"}`);
          }
        }
        renameBtn.onclick = (e) => {
          e.preventDefault();
          e.stopPropagation();
          void renameSessionId(selected);
        };
        let fileWrap = localStorage.getItem("codexweb.fileWrap") === "1";
        if (fileLaunch.wrap !== null) {
          fileWrap = Boolean(fileLaunch.wrap);
          localStorage.setItem("codexweb.fileWrap", fileWrap ? "1" : "0");
        }
        let fileMode = fileLaunch.mode || (fileFullscreenMode ? "edit" : "view");
        let fileDirty = false;
        let fileStatusBase = "";
        let fileLoadedText = "";
        let fileAutoRefreshTimer = null;
        let fileAutoRefreshInFlight = false;
        function applyFileWrap() {
          fileViewer.classList.toggle("wrap", fileWrap);
          fileWrapBtn.classList.toggle("active", fileWrap);
          fileEditor.wrap = fileWrap ? "soft" : "off";
          fileEditor.style.whiteSpace = fileWrap ? "pre-wrap" : "pre";
        }
        function updateFileStatus() {
          const suffix = fileDirty ? " (modified)" : "";
          fileStatus.textContent = `${fileStatusBase}${suffix}`;
        }
        function setFileDirty(next) {
          fileDirty = Boolean(next);
          updateFileStatus();
          fileSaveBtn.disabled = !(fileMode === "edit" && fileDirty);
        }
        function currentFileSessionId() {
          if (selected) return selected;
          if (fileSessionOverride) return fileSessionOverride;
          return "";
        }
        function getActiveFileScrollEl() {
          if (fileMode === "preview") return filePreview;
          if (fileMode === "edit") return fileEditor;
          return fileContent;
        }
        function captureFileScrollState() {
          const el = getActiveFileScrollEl();
          if (!el) return null;
          const maxScroll = Math.max(0, el.scrollHeight - el.clientHeight);
          const ratio = maxScroll > 0 ? el.scrollTop / maxScroll : 0;
          return { mode: fileMode, ratio };
        }
        function restoreFileScroll(state) {
          if (!state || (state.mode && state.mode !== fileMode)) return;
          const el = getActiveFileScrollEl();
          if (!el) return;
          const maxScroll = Math.max(0, el.scrollHeight - el.clientHeight);
          el.scrollTop = maxScroll > 0 ? Math.round(maxScroll * state.ratio) : 0;
        }
        function updateFilePreview() {
          const text = fileEditor.value || fileContent.textContent || "";
          filePreview.innerHTML = mdToHtmlCached(text);
          renderMermaidIn(filePreview);
        }
        function buildFileEditorUrl({ path, sessionId, mode, fullscreen, wrap }) {
          const url = new URL(resolveAppUrl("/"));
          if (path) url.searchParams.set("file", path);
          if (sessionId) url.searchParams.set("session_id", sessionId);
          if (mode) url.searchParams.set("mode", mode);
          if (fullscreen) url.searchParams.set("fullscreen", "1");
          if (wrap) url.searchParams.set("wrap", "1");
          return url.toString();
        }
        function exitFileFullscreen() {
          document.body.classList.remove("file-fullscreen");
          const base = resolveAppUrl("/");
          window.close();
          setTimeout(() => {
            if (!window.closed) window.location.href = base;
          }, 200);
        }
        function openFileEditorTab() {
          const path = String(filePathInput.value || "").trim();
          if (!path) {
            setToast("enter a file path first");
            return;
          }
          const sid = currentFileSessionId();
          const mode = fileMode === "preview" ? "preview" : "edit";
          const url = buildFileEditorUrl({
            path,
            sessionId: sid,
            mode,
            fullscreen: true,
            wrap: fileWrap,
          });
          window.open(url, "_blank", "noopener");
        }
        function applyFileReadResult(res, scrollState) {
          if (!res || typeof res.text !== "string") throw new Error("invalid response");
          fileContent.textContent = res.text;
          fileEditor.value = res.text;
          fileLoadedText = res.text;
          const size = typeof res.size === "number" ? res.size : res.text.length;
          const label = res.path ? String(res.path) : String(filePathInput.value || "").trim();
          fileStatusBase = `${label} (${fmtBytes(size)})`;
          setFileDirty(false);
          setFileMode(fileMode);
          requestAnimationFrame(() => restoreFileScroll(scrollState));
        }
        function setFileMode(mode) {
          fileMode = mode;
          fileViewer.dataset.mode = mode;
          const isView = mode === "view";
          const isEdit = mode === "edit";
          const isPreview = mode === "preview";
          fileContent.style.display = isView ? "block" : "none";
          fileEditor.style.display = isEdit ? "block" : "none";
          filePreview.style.display = isPreview ? "block" : "none";
          fileViewBtn.classList.toggle("active", isView);
          fileEditBtn.classList.toggle("active", isEdit);
          filePreviewBtn.classList.toggle("active", isPreview);
          fileSaveBtn.style.display = isEdit ? "inline-flex" : "none";
          fileSaveBtn.disabled = !(isEdit && fileDirty);
          if (isPreview) updateFilePreview();
          updateFileTitle();
        }
        function updateFileTitle() {
          if (!fileFullscreenMode) {
            fileTitle.textContent = "View file";
            return;
          }
          if (fileMode === "edit") fileTitle.textContent = "Edit file";
          else if (fileMode === "preview") fileTitle.textContent = "Preview file";
          else fileTitle.textContent = "Read file";
        }
        applyFileWrap();
        setFileMode(fileMode);
        if (fileFullscreenMode) filePopoutBtn.style.display = "none";
        fileOpenInlineBtn.style.display = fileFullscreenMode ? "inline-flex" : "none";
        if (fileFullscreenMode) filePathRow.style.display = "none";

        function promptFilePath() {
          const current = String(filePathInput.value || "").trim() || localStorage.getItem("codexweb.filePath") || "";
          const next = prompt("Open file path:", current);
          if (next === null) return;
          const trimmed = String(next || "").trim();
          if (!trimmed) return;
          filePathInput.value = trimmed;
          openFilePath();
        }

        function showFileViewer() {
          if (fileFullscreenMode) document.body.classList.add("file-fullscreen");
          fileBackdrop.style.display = fileFullscreenMode ? "none" : "block";
          fileViewer.style.display = "flex";
          applyFileWrap();
          startFileAutoRefresh();
          const s = selected ? sessionIndex.get(selected) : null;
          const last = localStorage.getItem("codexweb.filePath") || "";
          const def = last || (s && s.cwd ? String(s.cwd) : "");
          if (!filePathInput.value.trim()) filePathInput.value = def;
          if (!fileFullscreenMode) {
            filePathInput.focus();
            filePathInput.select();
          } else if (fileMode === "edit") {
            fileEditor.focus();
          }
        }
        function hideFileViewer() {
          if (fileFullscreenMode) {
            exitFileFullscreen();
            return;
          }
          stopFileAutoRefresh();
          fileBackdrop.style.display = "none";
          fileViewer.style.display = "none";
        }
        async function saveFilePath() {
          const path = String(filePathInput.value || "").trim();
          if (!path) {
            fileStatus.textContent = "Enter a file path.";
            return;
          }
          fileStatus.textContent = "Saving...";
          try {
            const body = { path, text: String(fileEditor.value || "") };
            const sid = currentFileSessionId();
            if (sid) body.session_id = sid;
            const res = await api("/api/files/write", { method: "POST", body });
            if (!res || res.ok !== true) throw new Error("invalid response");
            fileLoadedText = fileEditor.value || "";
            fileContent.textContent = fileLoadedText;
            const size = typeof res.size === "number" ? res.size : fileLoadedText.length;
            const label = res.path ? String(res.path) : path;
            fileStatusBase = `${label} (${fmtBytes(size)})`;
            setFileDirty(false);
            setToast("saved");
            if (fileMode === "preview") updateFilePreview();
            if (selected || fileSessionOverride) refreshSessions().catch((e) => console.error("refreshSessions failed", e));
          } catch (e) {
            fileStatus.textContent = `save error: ${e && e.message ? e.message : "unknown error"}`;
          }
        }
        async function openFilePath() {
          const path = String(filePathInput.value || "").trim();
          if (!path) {
            fileStatus.textContent = "Enter a file path.";
            return;
          }
          const scrollState = captureFileScrollState();
          fileStatus.textContent = "Loading...";
          fileContent.textContent = "";
          fileEditor.value = "";
          filePreview.innerHTML = "";
          fileStatusBase = "";
          try {
            const body = { path };
            const sid = currentFileSessionId();
            if (sid) body.session_id = sid;
            const res = await api("/api/files/read", { method: "POST", body });
            applyFileReadResult(res, scrollState);
            localStorage.setItem("codexweb.filePath", path);
            if (selected || fileSessionOverride) refreshSessions().catch((e) => console.error("refreshSessions failed", e));
          } catch (e) {
            fileStatus.textContent = `error: ${e && e.message ? e.message : "unknown error"}`;
          }
        }
        async function autoRefreshFileIfChanged() {
          if (!fileFullscreenMode) return;
          if (document.hidden) return;
          if (fileAutoRefreshInFlight) return;
          if (fileDirty) return;
          const path = String(filePathInput.value || "").trim();
          if (!path) return;
          const scrollState = captureFileScrollState();
          fileAutoRefreshInFlight = true;
          try {
            const body = { path };
            const sid = currentFileSessionId();
            if (sid) body.session_id = sid;
            const res = await api("/api/files/read", { method: "POST", body });
            if (!res || typeof res.text !== "string") throw new Error("invalid response");
            if (res.text === fileLoadedText) return;
            applyFileReadResult(res, scrollState);
          } catch (e) {
            console.error("file auto-refresh failed", e);
          } finally {
            fileAutoRefreshInFlight = false;
          }
        }
        function startFileAutoRefresh() {
          if (!fileFullscreenMode) return;
          if (fileAutoRefreshTimer) return;
          fileAutoRefreshTimer = setInterval(() => {
            void autoRefreshFileIfChanged();
          }, FILE_AUTO_REFRESH_MS);
        }
        function stopFileAutoRefresh() {
          if (!fileAutoRefreshTimer) return;
          clearInterval(fileAutoRefreshTimer);
          fileAutoRefreshTimer = null;
        }
        function maybeLaunchFileViewer() {
          if (fileLaunchHandled || !fileLaunchActive) return;
          fileLaunchHandled = true;
          showFileViewer();
          if (fileLaunch.path) {
            filePathInput.value = fileLaunch.path;
            openFilePath();
          }
        }
        bindTap(fileBtn, () => {
          showFileViewer();
        });
        fileWrapBtn.onclick = (e) => {
          e.preventDefault();
          e.stopPropagation();
          fileWrap = !fileWrap;
          localStorage.setItem("codexweb.fileWrap", fileWrap ? "1" : "0");
          applyFileWrap();
        };
        fileViewBtn.onclick = (e) => {
          e.preventDefault();
          e.stopPropagation();
          setFileMode("view");
        };
        fileEditBtn.onclick = (e) => {
          e.preventDefault();
          e.stopPropagation();
          setFileMode("edit");
        };
        filePreviewBtn.onclick = (e) => {
          e.preventDefault();
          e.stopPropagation();
          setFileMode("preview");
        };
        filePopoutBtn.onclick = (e) => {
          e.preventDefault();
          e.stopPropagation();
          openFileEditorTab();
        };
        fileOpenInlineBtn.onclick = (e) => {
          e.preventDefault();
          e.stopPropagation();
          promptFilePath();
        };
        fileSaveBtn.onclick = (e) => {
          e.preventDefault();
          e.stopPropagation();
          if (fileMode === "edit") saveFilePath();
        };
        fileCloseBtn.onclick = (e) => {
          e.preventDefault();
          e.stopPropagation();
          hideFileViewer();
        };
        fileBackdrop.onclick = () => hideFileViewer();
        fileOpenBtn.onclick = () => openFilePath();
        filePathInput.addEventListener("keydown", (e) => {
          if (e.key !== "Enter") return;
          e.preventDefault();
          openFilePath();
        });
        fileEditor.addEventListener("input", () => {
          setFileDirty(fileEditor.value !== fileLoadedText);
          if (fileMode === "preview") updateFilePreview();
        });
        document.addEventListener("visibilitychange", () => {
          if (!fileFullscreenMode) return;
          if (document.visibilityState !== "visible") return;
          void autoRefreshFileIfChanged();
        });
        document.addEventListener("keydown", (e) => {
          if (e.key !== "Escape") return;
          if (fileViewer.style.display === "flex") hideFileViewer();
          if (sendChoice.style.display === "flex") hideSendChoice();
          if (queueViewer.style.display === "flex") hideQueueViewer();
        });

        let sendChoicePending = null;
        function showSendChoice(raw) {
          sendChoicePending = raw;
          sendChoiceBackdrop.style.display = "block";
          sendChoice.style.display = "flex";
        }
        function hideSendChoice() {
          sendChoicePending = null;
          sendChoiceBackdrop.style.display = "none";
          sendChoice.style.display = "none";
        }
        const sendChoiceNowBtn = $("#sendChoiceNow");
        const sendChoiceLaterBtn = $("#sendChoiceLater");
        const sendChoiceCancelBtn = $("#sendChoiceCancel");
        if (sendChoiceNowBtn)
          sendChoiceNowBtn.onclick = async () => {
            const raw = sendChoicePending;
            hideSendChoice();
            if (!raw) return;
            clearComposer();
            await sendText(raw);
          };
        if (sendChoiceLaterBtn)
          sendChoiceLaterBtn.onclick = () => {
            const raw = sendChoicePending;
            hideSendChoice();
            if (!raw) return;
            clearComposer();
            queueLocalMessage(raw);
          };
        if (sendChoiceCancelBtn)
          sendChoiceCancelBtn.onclick = () => {
            hideSendChoice();
          };
        sendChoiceBackdrop.onclick = () => hideSendChoice();

        function renderQueueList() {
          queueList.innerHTML = "";
          const sid = selected;
          if (!sid) {
            queueEmpty.style.display = "block";
            return;
          }
          const q = getDeferredQueue(sid);
          queueEmpty.style.display = q.length ? "none" : "block";
          if (!q.length) return;
          q.forEach((text, idx) => {
            const row = el("div", { class: "queueItem" });
            const ta = el("textarea", { class: "queueText", "aria-label": `Queued message ${idx + 1}` });
            ta.value = text;
            ta.oninput = () => {
              const q2 = getDeferredQueue(sid);
              if (idx >= q2.length) return;
              q2[idx] = String(ta.value || "");
              scheduleQueueSave();
            };
            const del = el("button", { class: "icon-btn danger", title: "Delete", "aria-label": "Delete", type: "button", html: iconSvg("trash") });
            del.onclick = (e) => {
              e.preventDefault();
              e.stopPropagation();
              const q2 = getDeferredQueue(sid);
              if (idx >= q2.length) return;
              q2.splice(idx, 1);
              saveDeferredToStorage(sid);
              updateQueueBadge();
              renderQueueList();
            };
            row.appendChild(ta);
            row.appendChild(del);
            queueList.appendChild(row);
          });
        }

        function showQueueViewer() {
          queueBackdrop.style.display = "block";
          queueViewer.style.display = "flex";
          renderQueueList();
        }

        function hideQueueViewer() {
          queueBackdrop.style.display = "none";
          queueViewer.style.display = "none";
        }

        const queueBtn = $("#queueBtn");
        if (queueBtn) {
          queueBtn.onclick = (e) => {
            e.preventDefault();
            e.stopPropagation();
            showQueueViewer();
          };
        }
        queueCloseBtn.onclick = (e) => {
          e.preventDefault();
          e.stopPropagation();
          hideQueueViewer();
        };
        queueBackdrop.onclick = () => hideQueueViewer();
        async function applySessionAlias(sessionId, alias) {
          if (!sessionId || !alias) return;
          try {
            const res = await api(`/api/sessions/${sessionId}/rename`, { method: "POST", body: { name: alias } });
            const nextAlias = res && typeof res.alias === "string" ? res.alias : alias;
            const s = sessionIndex.get(sessionId);
            if (s) s.alias = nextAlias;
          } catch (e) {
            console.warn("auto rename failed", e);
          }
        }

        async function spawnSessionWithCwd(cwd, { alias } = {}) {
          if (!cwd || !String(cwd).trim()) {
            setToast("cwd unavailable");
            return null;
          }
          try {
            setToast("starting...");
            const res = await api("/api/sessions", { method: "POST", body: { cwd: String(cwd) } });
            const brokerPid = res && res.broker_pid ? Number(res.broker_pid) : null;
            if (!brokerPid) {
              setToast("start failed");
              return null;
            }
            setToast(`started (broker ${brokerPid})`);
            for (let i = 0; i < 60; i++) {
              const sessions = await refreshSessions();
              const found = (sessions || []).find((x) => Number(x.broker_pid || 0) === brokerPid);
              if (found) {
                if (alias) await applySessionAlias(found.session_id, alias);
                selectSession(found.session_id);
                return brokerPid;
              }
              await new Promise((r) => setTimeout(r, 250));
            }
            setToast("session will appear once Codex creates a rollout log");
            return brokerPid;
          } catch (e) {
            setToast(`start error: ${e.message}`);
            return null;
          }
        }
        $("#newBtn").onclick = async () => {
          const cur = selected ? sessionIndex.get(selected) : null;
          const def = cur && cur.cwd && cur.cwd !== "?" ? cur.cwd : "";
          const cwd = prompt("New session cwd:", def);
          if (!cwd) return;
          await spawnSessionWithCwd(cwd);
        };
	        interruptBtn.onclick = async () => {
	          if (!selected) return;
	          try {
	            setToast("interrupting...");
            await api(`/api/sessions/${selected}/interrupt`, { method: "POST" });
            pollFastUntilMs = Date.now() + 2500;
            kickPoll(0);
          } catch (e) {
            setToast(`interrupt error: ${e.message}`);
          }
        };

        $("#logoutBtnSide").onclick = async () => {
          await api("/api/logout", { method: "POST" });
          renderLogin(renderApp);
        };

        duplicateBtn.onclick = async () => {
          if (!selected) return;
          const s = sessionIndex.get(selected);
          const cwd = s && s.cwd && s.cwd !== "?" ? s.cwd : "";
          if (!cwd) {
            setToast("cwd unavailable");
            return;
          }
          const base = sessionDisplayName(s) || baseName(cwd) || "Session";
          const alias = buildDuplicateAlias(base);
          await spawnSessionWithCwd(cwd, { alias });
        };

        toggleSidebarBtn.onclick = () => {
          if (isMobile()) {
            setSidebarOpen(!document.body.classList.contains("sidebar-open"));
            return;
          }
          setSidebarCollapsed(!document.body.classList.contains("sidebar-collapsed"));
        };
	        backdrop.onclick = () => setSidebarOpen(false);

	        chat.addEventListener("scroll", () => {
	          const cur = chat.scrollTop;
	          const d = cur - lastScrollTop;
	          lastScrollTop = cur;
	          if (d < 0) autoScroll = false;
          else if (isNearBottom()) autoScroll = true;
          jumpBtn.style.display = autoScroll ? "none" : "inline-flex";
        });
        chat.addEventListener(
          "wheel",
          (e) => {
            if (e.deltaY < 0) {
              autoScroll = false;
              jumpBtn.style.display = "inline-flex";
              maybeAutoLoadOlder();
            }
          },
          { passive: true }
        );
        let touchY = null;
        chat.addEventListener(
          "touchstart",
          (e) => {
            const t = e.touches && e.touches[0];
            touchY = t ? t.clientY : null;
          },
          { passive: true }
        );
        chat.addEventListener(
          "touchmove",
          (e) => {
            const t = e.touches && e.touches[0];
            if (!t || touchY === null) return;
            const dy = t.clientY - touchY;
            touchY = t.clientY;
            // Finger moves down -> content scrolls up.
            if (dy > 0) {
              autoScroll = false;
              jumpBtn.style.display = "inline-flex";
              maybeAutoLoadOlder();
            }
          },
          { passive: true }
        );
        jumpBtn.onclick = () => {
          autoScroll = true;
          jumpBtn.style.display = "none";
          scrollToBottom();
        };
        olderBtn.onclick = () => {
          void loadOlderMessages({ auto: false });
        };

	        const textarea = $("#msg");
	        const msgPh = $("#msgPh");
	        const imgInput = $("#imgInput");
        const attachBtn = $("#attachBtn");
        if (!attachBadgeEl) {
          attachBadgeEl = el("span", { class: "attachBadge", id: "attachBadge" });
          attachBtn.appendChild(attachBadgeEl);
        }
        if (!queueBadgeEl && queueBtn) {
          queueBadgeEl = el("span", { class: "attachBadge queueBadge", id: "queueBadge" });
          queueBtn.appendChild(queueBadgeEl);
        }
        const setAttachCount = (n) => {
          const next = Math.max(0, Number(n) || 0);
          attachedImages = next;
          if (!attachBadgeEl) return;
          if (next > 0) {
            attachBadgeEl.textContent = String(next);
            attachBadgeEl.style.display = "inline-flex";
          } else {
            attachBadgeEl.textContent = "";
            attachBadgeEl.style.display = "none";
          }
        };
        setAttachCount(0);
        updateQueueBadge();

        function setLastLine(text) {
          const t = String(text || "").trim();
          const elLine = $("#lastLine");
          if (!elLine) return;
          elLine.textContent = t;
          elLine.style.display = t ? "block" : "none";
        }
	        function autoGrow() {
	          const basePx = parseFloat(getComputedStyle(textarea).minHeight || "0") || 32;
	          const maxPx = 180;
	          const hasNewline = textarea.value.includes("\n");
	          if (msgPh) msgPh.style.display = textarea.value ? "none" : "flex";
	          textarea.style.height = `${basePx}px`;
	          let h = textarea.scrollHeight;
	          const needsMultiline = hasNewline || h > basePx + 1;
	          form.classList.toggle("multiline", needsMultiline);
	          textarea.style.height = needsMultiline ? "auto" : `${basePx}px`;
	          h = textarea.scrollHeight;
	          const next = needsMultiline ? Math.min(h, maxPx) : basePx;
	          textarea.style.height = `${next}px`;
	          textarea.style.overflowY = h > maxPx ? "auto" : "hidden";
	          if (autoScroll) requestAnimationFrame(() => scrollToBottom());
	        }
	        textarea.addEventListener("input", () => {
            autoGrow();
            if (selected) scheduleDraftSave(selected, textarea.value);
          });
	        textarea.addEventListener(
	          "focus",
	          () => {
	            autoScroll = true;
	            jumpBtn.style.display = "none";
	            const tick = () => {
	              updateAppHeightVar();
	              scrollToBottom();
	            };
	            requestAnimationFrame(tick);
	            setTimeout(tick, 120);
	            setTimeout(tick, 350);
	          },
	          { passive: true }
	        );
	        textarea.addEventListener(
	          "blur",
	          () => {
	            setTimeout(updateAppHeightVar, 0);
	          },
	          { passive: true }
	        );
        textarea.addEventListener("keydown", (e) => {
          if (e.key !== "Enter") return;
          if (e.isComposing) return;
          if (!(e.ctrlKey || e.metaKey)) return;
          e.preventDefault();
          form.requestSubmit();
        });
        textarea.addEventListener("paste", (e) => {
          const cd = e.clipboardData;
          if (!cd) return;
          let f = null;
          if (cd.files && cd.files.length) {
            f = cd.files[0];
          } else if (cd.items && cd.items.length) {
            for (const item of cd.items) {
              if (!item || item.kind !== "file") continue;
              const file = item.getAsFile();
              if (!file) continue;
              f = file;
              break;
            }
          }
          if (!f) return;
          const t = String(f.type || "").toLowerCase();
          const name = String(f.name || "").toLowerCase();
          const isImg =
            t.startsWith("image/") ||
            name.endsWith(".png") ||
            name.endsWith(".jpg") ||
            name.endsWith(".jpeg") ||
            name.endsWith(".webp") ||
            name.endsWith(".heic") ||
            name.endsWith(".heif");
          if (!isImg) return;
          e.preventDefault();
          void handleImageFile(f);
        });
        window.addEventListener("resize", () => {
          if (autoScroll) requestAnimationFrame(() => scrollToBottom());
        });

	        attachBtn.onclick = () => {
	          if (!selected) return;
	          imgInput.value = "";
	          imgInput.click();
	        };
		        async function handleImageFile(f) {
		          if (!selected) return;
		          if (!f) return;
		          if (sending) return;
		          try {
	            function safeStem(name) {
	              const s = String(name || "image");
	              const base = s.split("/").pop() || s;
	              const dot = base.lastIndexOf(".");
	              return (dot > 0 ? base.slice(0, dot) : base).replace(/[^a-zA-Z0-9._-]+/g, "_").slice(0, 80) || "image";
	            }
	            function extLower(name) {
	              const s = String(name || "");
	              const dot = s.lastIndexOf(".");
	              return dot >= 0 ? s.slice(dot + 1).toLowerCase() : "";
	            }
	            function isLikelyHeic(file) {
	              const t = String(file.type || "").toLowerCase();
	              const e = extLower(file.name);
	              return t.includes("heic") || t.includes("heif") || e === "heic" || e === "heif";
	            }
	            function isSupportedMime(type) {
	              const t = String(type || "").toLowerCase();
	              return t === "image/png" || t === "image/jpeg" || t === "image/jpg" || t === "image/webp";
	            }
	            function b64FromBytes(bytes) {
	              let bin = "";
	              const chunk = 0x8000;
	              for (let i = 0; i < bytes.length; i += chunk) {
	                bin += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
	              }
	              return btoa(bin);
	            }
	            async function toJpegBlob(file, { maxDim = 2048, quality = 0.86 } = {}) {
	              const url = URL.createObjectURL(file);
	              try {
	                const img = new Image();
	                img.decoding = "async";
	                img.src = url;
	                if (img.decode) await img.decode();
	                else
	                  await new Promise((resolve, reject) => {
	                    img.onload = resolve;
	                    img.onerror = () => reject(new Error("decode failed"));
	                  });
	                const w0 = img.naturalWidth || img.width || 0;
	                const h0 = img.naturalHeight || img.height || 0;
	                if (!w0 || !h0) throw new Error("invalid image dimensions");
	                const scale = Math.min(1, maxDim / Math.max(w0, h0));
	                const w = Math.max(1, Math.round(w0 * scale));
	                const h = Math.max(1, Math.round(h0 * scale));
	                const canvas = document.createElement("canvas");
	                canvas.width = w;
	                canvas.height = h;
	                const ctx = canvas.getContext("2d", { alpha: false });
	                if (!ctx) throw new Error("no canvas");
	                ctx.drawImage(img, 0, 0, w, h);
	                const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/jpeg", quality));
	                if (!blob) throw new Error("jpeg encode failed");
	                return blob;
	              } finally {
	                URL.revokeObjectURL(url);
	              }
	            }

	            setToast("attaching image...");
	            const maxBytes = 10 * 1024 * 1024;
	            let uploadBlob = f;
	            let uploadName = f.name || "image";
	            if (f.size > maxBytes || isLikelyHeic(f) || !isSupportedMime(f.type)) {
	              setToast("converting image...");
	              const stem = safeStem(f.name);
	              uploadName = `${stem}.jpg`;
	              // Try a few (dim, quality) pairs until it fits.
	              const tries = [
	                { maxDim: 2048, quality: 0.86 },
	                { maxDim: 1600, quality: 0.82 },
	                { maxDim: 1600, quality: 0.72 },
	                { maxDim: 1280, quality: 0.68 },
	                { maxDim: 1280, quality: 0.58 },
	              ];
	              let blob = null;
	              for (const t of tries) {
	                blob = await toJpegBlob(f, t);
	                if (blob.size <= maxBytes) break;
	              }
	              if (!blob || blob.size > maxBytes) throw new Error("image too large");
	              uploadBlob = blob;
	            }

	            const ab = await uploadBlob.arrayBuffer();
	            if (ab.byteLength > maxBytes) throw new Error("image too large");
	            const b64 = b64FromBytes(new Uint8Array(ab));
		            const res = await api(`/api/sessions/${selected}/inject_image`, {
		              method: "POST",
		              body: { filename: uploadName, data_b64: b64 },
		            });
		            if (res && res.ok) {
		              setToast("image attached");
		              setAttachCount(attachedImages + 1);
		            }
		            pollFastUntilMs = Date.now() + 4000;
		            kickPoll(0);
		          } catch (e) {
	            setToast(`attach error: ${e.message}`);
	          }
		        }
		        imgInput.addEventListener("change", async () => {
		          const f = imgInput.files && imgInput.files[0];
		          await handleImageFile(f);
		          imgInput.value = "";
		        });

        function clearComposer() {
          const ta = $("#msg");
          if (!ta) return;
          ta.value = "";
          autoGrow();
          if (selected) saveDraftToStorage(selected, "");
        }

        async function sendText(raw, { deferred = false } = {}) {
          if (!selected) return;
          if (!raw || !raw.trim()) return;
          if (sending) return;
          sending = true;
          $("#sendBtn").disabled = true;
          setToast("sending...");
          if (deferred && selected) {
            markDeferGate(selected);
          }

          const localId = ++localEchoSeq;
          const t0 = Date.now() / 1000;
          pendingUser.push({ id: localId, key: pendingMatchKey(raw), loose: normalizeTextForPendingMatch(raw), t0, text: raw });
          appendEvent({ role: "user", text: raw, pending: true, localId, ts: t0 });
          turnOpen = true;
          currentRunning = true;
          updateUserSummaryFromText(selected, raw);
          try {
            const res = await api(`/api/sessions/${selected}/send`, { method: "POST", body: { text: raw } });
            if (res.queued) setToast(`queued (queue ${res.queue_len})`);
            else setToast("sent");
            setAttachCount(0);
            pollFastUntilMs = Date.now() + 5000;
            kickPoll(0);
            await refreshSessions();
          } catch (e2) {
            setToast(`send error: ${e2.message}`);
            const pendingEl = chatInner.querySelector(`.msg.user[data-local-id="${localId}"]`);
            if (pendingEl) {
              pendingEl.style.opacity = "1";
              pendingEl.style.borderColor = "rgba(185, 28, 28, 0.7)";
              pendingEl.style.boxShadow = "0 0 0 2px rgba(185, 28, 28, 0.12)";
            }
            if (deferred && selected) {
              queueLocalMessage(raw, { front: true });
            }
          } finally {
            sending = false;
            $("#sendBtn").disabled = false;
            if (deferred) deferInFlight = false;
          }
        }

        form.onsubmit = async (e) => {
          e.preventDefault();
          if (!selected) return;
          const raw = $("#msg").value;
          if (!raw || !raw.trim()) return;
          if (sending) return;
          if (currentRunning) {
            showSendChoice(raw);
            return;
          }
          clearComposer();
          await sendText(raw);
        };

	        (async () => {
	          if (localStorage.getItem("codexweb.sidebarCollapsed") === "1") setSidebarCollapsed(true);
	          if (localStorage.getItem("codexweb.sidebarOpen") === "1") setSidebarOpen(true);

	          try {
	            const sessions = await refreshSessions();
	            const remembered = localStorage.getItem("codexweb.selected");
	            const first = sessions && sessions.length ? sessions[0].session_id : null;
	            let pick = remembered && sessionIndex.has(remembered) ? remembered : first;
	            if (fileLaunch.sessionId && sessionIndex.has(fileLaunch.sessionId)) pick = fileLaunch.sessionId;
	            if (pick) await selectSession(pick);
              if (fileLaunchActive) maybeLaunchFileViewer();
	          } catch (e) {
	            if (e && e.status === 401) {
	              renderLogin(renderApp);
	              return;
	            }
	            console.error("initial refreshSessions failed", e);
	            setToast(`sessions error: ${e && e.message ? e.message : "unknown error"}`);
	          } finally {
	            if (msgPh) msgPh.style.display = textarea.value ? "none" : "flex";
	            autoGrow();

	            if (sessionsTimer) clearInterval(sessionsTimer);
	            sessionsTimer = setInterval(async () => {
	              try {
	                await refreshSessions();
	              } catch (e2) {
	                if (e2 && e2.status === 401) {
	                  if (sessionsTimer) clearInterval(sessionsTimer);
	                  sessionsTimer = null;
	                  renderLogin(renderApp);
	                  return;
	                }
	                console.error("refreshSessions timer failed", e2);
	              }
	            }, 2500);
	          }
	        })();
      }

      (async function boot() {
        try {
          await api("/api/me");
          renderApp();
        } catch (e) {
          if (e && e.status === 401) {
            renderLogin(renderApp);
            return;
          }
          console.error("boot auth check failed", e);
          const err = document.createElement("pre");
          err.textContent = `error: unable to contact server (${e && e.message ? e.message : "unknown error"})`;
          document.body.innerHTML = "";
          document.body.appendChild(err);
        }
      })();
