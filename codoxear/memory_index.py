from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import re
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .memory_store import ChunkRecord
from .memory_store import MemoryStore
from .memory_store import SourceRecord
from .memory_store import cosine_similarity


HEADING_RE = re.compile(r"^(#{1,6})\s+(.*\S)\s*$")
WORD_RE = re.compile(r"[A-Za-z0-9_./:-]+")
DEFAULT_DIM = 256
DEFAULT_OPENAI_EMBED_MODEL = "text-embedding-3-small"
DEFAULT_OPENAI_BASE_URL = "https://api.openai.com/v1"
DEFAULT_MEMORY_DOCS_DIR = ".memory/docs"


@dataclass
class MemoryConfig:
    repo_root: Path
    memory_root: Path
    index_db_path: Path
    index_manifest_path: Path
    registry_path: Path
    chunks_path: Path
    provider: str
    openai_model: str
    openai_base_url: str
    top_k_default: int


@dataclass
class SearchMatch:
    chunk_id: str
    note_id: str
    file_path: str
    title: str
    kind: str
    updated_at: str
    score: float
    snippet: str
    heading_path: list[str]


@dataclass
class IndexRefreshResult:
    refreshed: bool
    reason: str
    summary: dict[str, Any]


@dataclass
class SourceStat:
    path: Path
    note_id: str
    file_path: str
    kind: str
    updated_at: str
    size: int
    mtime_ns: int


@dataclass
class SourceInput:
    source: SourceRecord
    text: str


def resolve_project_root(*, start: Path | None = None) -> Path:
    candidate = (start or Path(os.environ.get("CODOXEAR_MEMORY_CALLER_CWD", "") or os.getcwd())).resolve()
    search_roots = [candidate, *candidate.parents]
    for path in search_roots:
        if (path / DEFAULT_MEMORY_DOCS_DIR).is_dir():
            return path
        if (path / "AGENTS.md").is_file() and ((path / ".git").exists() or (path / DEFAULT_MEMORY_DOCS_DIR).exists()):
            return path
    return candidate


class EmbeddingProvider:
    def embed_texts(self, texts: list[str]) -> list[list[float]]:
        raise NotImplementedError


class HashEmbeddingProvider(EmbeddingProvider):
    def __init__(self, *, dim: int = DEFAULT_DIM) -> None:
        self.dim = max(32, int(dim))

    def embed_texts(self, texts: list[str]) -> list[list[float]]:
        return [self._embed_one(text) for text in texts]

    def _embed_one(self, text: str) -> list[float]:
        vec = [0.0] * self.dim
        for token in _tokenize(text):
            digest = hashlib.sha256(token.encode("utf-8")).digest()
            bucket = int.from_bytes(digest[:4], "big") % self.dim
            sign = -1.0 if (digest[4] & 1) else 1.0
            vec[bucket] += sign
        norm = sum(v * v for v in vec) ** 0.5
        if norm > 0.0:
            vec = [v / norm for v in vec]
        return vec


class OpenAIEmbeddingProvider(EmbeddingProvider):
    def __init__(self, *, model: str, api_key: str, base_url: str) -> None:
        self.model = model
        self.api_key = api_key.strip()
        if not self.api_key:
            raise RuntimeError(
                "An embedding API key is required for provider=openai; set CODOXEAR_MEMORY_OPENAI_API_KEY or OPENAI_API_KEY"
            )
        self.base_url = base_url.rstrip("/") or DEFAULT_OPENAI_BASE_URL

    def embed_texts(self, texts: list[str]) -> list[list[float]]:
        url = self.base_url + "/embeddings"
        body = json.dumps({"model": self.model, "input": texts}).encode("utf-8")
        req = urllib.request.Request(
            url,
            data=body,
            headers={
                "Authorization": f"Bearer {self.api_key}",
                "Content-Type": "application/json",
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                raw = json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"OpenAI embeddings request failed: {exc.code} {detail}") from exc
        data = raw.get("data")
        if not isinstance(data, list):
            raise RuntimeError("invalid OpenAI embeddings response: missing data")
        out: list[list[float]] = []
        for item in data:
            embedding = item.get("embedding") if isinstance(item, dict) else None
            if not isinstance(embedding, list):
                raise RuntimeError("invalid OpenAI embeddings response: missing embedding")
            out.append([float(x) for x in embedding])
        return out


def default_config(*, repo_root: Path | None = None) -> MemoryConfig:
    root = resolve_project_root(start=repo_root)
    memory_root = root / ".memory"
    index_root = memory_root / "index"
    memory_api_key = os.environ.get("CODOXEAR_MEMORY_OPENAI_API_KEY", "").strip()
    global_api_key = os.environ.get("OPENAI_API_KEY", "").strip()
    provider = os.environ.get("CODOXEAR_MEMORY_EMBED_PROVIDER", "").strip().lower()
    if not provider:
        provider = "openai" if (memory_api_key or global_api_key) else "hash"
    model = os.environ.get("CODOXEAR_MEMORY_OPENAI_MODEL", DEFAULT_OPENAI_EMBED_MODEL).strip() or DEFAULT_OPENAI_EMBED_MODEL
    base_url = os.environ.get("CODOXEAR_MEMORY_OPENAI_BASE_URL", "").strip() or os.environ.get("OPENAI_BASE_URL", DEFAULT_OPENAI_BASE_URL).strip() or DEFAULT_OPENAI_BASE_URL
    return MemoryConfig(
        repo_root=root,
        memory_root=memory_root,
        index_db_path=index_root / "index.sqlite3",
        index_manifest_path=index_root / "manifest.json",
        registry_path=index_root / "registry.jsonl",
        chunks_path=index_root / "chunks.jsonl",
        provider=provider,
        openai_model=model,
        openai_base_url=base_url,
        top_k_default=5,
    )


def source_files(config: MemoryConfig) -> list[Path]:
    out: list[Path] = []
    agents = config.repo_root / "AGENTS.md"
    if agents.is_file():
        out.append(agents)
    docs = config.repo_root / DEFAULT_MEMORY_DOCS_DIR
    if docs.is_dir():
        out.extend(sorted(p for p in docs.rglob("*.md") if p.is_file()))
    return out


def build_index(config: MemoryConfig) -> dict[str, Any]:
    provider = make_provider(config)
    files = source_files(config)
    stats = scan_source_stats(config, files)
    snapshot = current_source_snapshot(config, stats)
    source_inputs = [prepare_source_input(stat) for stat in stats]
    sources, chunks = materialize_source_inputs(source_inputs)
    vectors = provider.embed_texts([chunk.text for chunk in chunks]) if chunks else []
    stored: list[ChunkRecord] = []
    for chunk, vector in zip(chunks, vectors):
        stored.append(
            ChunkRecord(
                chunk_id=chunk.chunk_id,
                note_id=chunk.note_id,
                file_path=chunk.file_path,
                title=chunk.title,
                heading_path=chunk.heading_path,
                kind=chunk.kind,
                updated_at=chunk.updated_at,
                text=chunk.text,
                source_hash=chunk.source_hash,
                vector=vector,
            )
        )

    config.index_db_path.parent.mkdir(parents=True, exist_ok=True)
    store = MemoryStore(config.index_db_path)
    meta = {
        "provider": config.provider,
        "openai_model": config.openai_model,
        "openai_base_url": config.openai_base_url,
        "built_at": _utc_now_iso(),
        "repo_root": str(config.repo_root),
        "source_count": len(files),
        "source_fingerprint": snapshot["source_fingerprint"],
        "source_latest_mtime": snapshot["source_latest_mtime"],
        "chunk_count": len(stored),
    }
    store.replace_index(sources=sources, chunks=stored, meta=meta)
    _write_registry_from_store(config, store)
    config.index_manifest_path.write_text(json.dumps(meta, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return meta


@dataclass
class PreparedChunk:
    chunk_id: str
    note_id: str
    file_path: str
    title: str
    heading_path: list[str]
    kind: str
    updated_at: str
    text: str
    source_hash: str


def chunk_files(config: MemoryConfig, files: list[Path]) -> list[PreparedChunk]:
    out: list[PreparedChunk] = []
    for path in files:
        rel = path.relative_to(config.repo_root).as_posix()
        note_id = rel
        kind = classify_kind(rel)
        updated_at = _mtime_iso(path)
        source_hash = _file_hash(path)
        text = path.read_text(encoding="utf-8", errors="replace")
        sections = split_markdown_sections(text)
        if not sections:
            sections = [([path.stem], text.strip())]
        used = 0
        for heading_path, body in sections:
            body = body.strip()
            if not body:
                continue
            used += 1
            title = " > ".join(heading_path) if heading_path else rel
            slug = slugify(title) or f"chunk-{used}"
            out.append(
                PreparedChunk(
                    chunk_id=f"{note_id}#{slug}-{used}",
                    note_id=note_id,
                    file_path=rel,
                    title=title,
                    heading_path=list(heading_path),
                    kind=kind,
                    updated_at=updated_at,
                    text=body,
                    source_hash=source_hash,
                )
            )
    return out


def scan_source_stats(config: MemoryConfig, files: list[Path] | None = None) -> list[SourceStat]:
    out: list[SourceStat] = []
    for path in files if files is not None else source_files(config):
        rel = path.relative_to(config.repo_root).as_posix()
        stat = path.stat()
        out.append(
            SourceStat(
                path=path,
                note_id=rel,
                file_path=rel,
                kind=classify_kind(rel),
                updated_at=_mtime_iso(path),
                size=int(stat.st_size),
                mtime_ns=int(stat.st_mtime_ns),
            )
        )
    return out


def prepare_source_input(stat: SourceStat) -> SourceInput:
    text = stat.path.read_text(encoding="utf-8", errors="replace")
    return SourceInput(
        source=SourceRecord(
            note_id=stat.note_id,
            file_path=stat.file_path,
            kind=stat.kind,
            updated_at=stat.updated_at,
            source_hash=hashlib.sha256(text.encode("utf-8")).hexdigest(),
            size=stat.size,
            mtime_ns=stat.mtime_ns,
        ),
        text=text,
    )


def materialize_source_inputs(source_inputs: list[SourceInput]) -> tuple[list[SourceRecord], list[PreparedChunk]]:
    sources = [source_input.source for source_input in source_inputs]
    chunks: list[PreparedChunk] = []
    for source_input in source_inputs:
        sections = split_markdown_sections(source_input.text)
        if not sections:
            sections = [([Path(source_input.source.file_path).stem], source_input.text.strip())]
        used = 0
        for heading_path, body in sections:
            body = body.strip()
            if not body:
                continue
            used += 1
            title = " > ".join(heading_path) if heading_path else source_input.source.file_path
            slug = slugify(title) or f"chunk-{used}"
            chunks.append(
                PreparedChunk(
                    chunk_id=f"{source_input.source.note_id}#{slug}-{used}",
                    note_id=source_input.source.note_id,
                    file_path=source_input.source.file_path,
                    title=title,
                    heading_path=list(heading_path),
                    kind=source_input.source.kind,
                    updated_at=source_input.source.updated_at,
                    text=body,
                    source_hash=source_input.source.source_hash,
                )
            )
    return sources, chunks


def split_markdown_sections(text: str) -> list[tuple[list[str], str]]:
    lines = text.splitlines()
    headings: list[str] = []
    current_path: list[str] = []
    current: list[str] = []
    sections: list[tuple[list[str], str]] = []

    def flush() -> None:
        if not current:
            return
        body = "\n".join(current).strip()
        if body:
            path = current_path[:] if current_path else ([headings[0]] if headings else [])
            sections.append((path, body))

    for line in lines:
        match = HEADING_RE.match(line)
        if match:
            flush()
            level = len(match.group(1))
            title = match.group(2).strip()
            while len(headings) >= level:
                headings.pop()
            headings.append(title)
            current_path = headings[:]
            current = [line]
            continue
        current.append(line)
    flush()
    return sections


def search_index(
    config: MemoryConfig,
    *,
    query: str,
    top_k: int | None = None,
    kind: str | None = None,
    path_prefix: str | None = None,
) -> list[SearchMatch]:
    store = MemoryStore(config.index_db_path)
    meta = store.read_meta()
    db_provider = str(meta.get("provider") or config.provider)
    query_provider = make_provider(config, provider_override=db_provider)
    query_vec = query_provider.embed_texts([query])[0]
    path_filter = (path_prefix or "").strip()
    kind_filter = (kind or "").strip().lower()
    q_tokens = set(_tokenize(query))
    matches: list[SearchMatch] = []
    for chunk in store.all_chunks():
        if kind_filter and chunk.kind.lower() != kind_filter:
            continue
        if path_filter and (not chunk.file_path.startswith(path_filter)):
            continue
        semantic = cosine_similarity(query_vec, chunk.vector)
        lexical = lexical_overlap_score(q_tokens, set(_tokenize(chunk.text + " " + chunk.title)))
        score = (semantic * 0.85) + (lexical * 0.15)
        if score <= 0.0:
            continue
        matches.append(
            SearchMatch(
                chunk_id=chunk.chunk_id,
                note_id=chunk.note_id,
                file_path=chunk.file_path,
                title=chunk.title,
                kind=chunk.kind,
                updated_at=chunk.updated_at,
                score=score,
                snippet=compact_snippet(chunk.text),
                heading_path=chunk.heading_path,
            )
        )
    matches.sort(key=lambda item: (item.score, item.updated_at), reverse=True)
    limit = max(1, int(top_k or config.top_k_default))
    return matches[:limit]


def current_source_snapshot(config: MemoryConfig, stats: list[SourceStat] | None = None) -> dict[str, Any]:
    source_stats = stats if stats is not None else scan_source_stats(config)
    latest_mtime_ns = 0
    digest = hashlib.sha256()
    for stat in source_stats:
        latest_mtime_ns = max(latest_mtime_ns, int(stat.mtime_ns))
        digest.update(stat.file_path.encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(int(stat.mtime_ns)).encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(int(stat.size)).encode("utf-8"))
        digest.update(b"\n")
    latest_iso = (
        datetime.datetime.fromtimestamp(latest_mtime_ns / 1_000_000_000, tz=datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
        if latest_mtime_ns > 0
        else None
    )
    return {
        "source_count": len(source_stats),
        "source_fingerprint": digest.hexdigest(),
        "source_latest_mtime": latest_iso,
    }


def read_manifest(config: MemoryConfig) -> dict[str, Any] | None:
    if not config.index_manifest_path.is_file():
        return None
    try:
        return json.loads(config.index_manifest_path.read_text(encoding="utf-8"))
    except Exception:
        return None


def index_staleness(config: MemoryConfig) -> tuple[bool, str, dict[str, Any] | None]:
    if not config.index_db_path.is_file():
        return True, "missing index db", None
    manifest = read_manifest(config)
    if not isinstance(manifest, dict):
        return True, "missing manifest", None
    snapshot = current_source_snapshot(config)
    if str(manifest.get("provider") or "") != str(config.provider):
        return True, "embedding provider changed", manifest
    if str(manifest.get("openai_model") or "") != str(config.openai_model):
        return True, "embedding model changed", manifest
    if str(manifest.get("openai_base_url") or "") != str(config.openai_base_url):
        return True, "embedding base url changed", manifest
    if int(manifest.get("source_count") or -1) != int(snapshot["source_count"]):
        return True, "source count changed", manifest
    if str(manifest.get("source_fingerprint") or "") != str(snapshot["source_fingerprint"]):
        return True, "source fingerprint changed", manifest
    return False, "up to date", manifest


def ensure_index_current(config: MemoryConfig, *, force: bool = False) -> IndexRefreshResult:
    if force:
        summary = build_index(config)
        return IndexRefreshResult(refreshed=True, reason="forced rebuild", summary=summary)
    store = MemoryStore(config.index_db_path)
    stale, reason, manifest = index_staleness(config)
    if stale and reason in ("missing index db", "missing manifest", "embedding provider changed", "embedding model changed", "embedding base url changed"):
        summary = build_index(config)
        return IndexRefreshResult(refreshed=True, reason=reason, summary=summary)

    files = source_files(config)
    stats = scan_source_stats(config, files)
    snapshot = current_source_snapshot(config, stats)
    stored_sources = {source.file_path: source for source in store.all_sources()}
    current_by_path = {stat.file_path: stat for stat in stats}
    removed = sorted(set(stored_sources) - set(current_by_path))
    changed_stats: list[SourceStat] = []
    for file_path, stat in current_by_path.items():
        existing = stored_sources.get(file_path)
        if existing is None:
            changed_stats.append(stat)
            continue
        if int(existing.mtime_ns) != int(stat.mtime_ns) or int(existing.size) != int(stat.size):
            changed_stats.append(stat)

    if not changed_stats and not removed:
        return IndexRefreshResult(refreshed=False, reason="up to date", summary=dict(manifest or {}))

    provider = make_provider(config)
    source_inputs = [prepare_source_input(stat) for stat in changed_stats]
    updated_sources, changed_chunks = materialize_source_inputs(source_inputs)
    vectors = provider.embed_texts([chunk.text for chunk in changed_chunks]) if changed_chunks else []
    updated_chunks: list[ChunkRecord] = []
    for chunk, vector in zip(changed_chunks, vectors):
        updated_chunks.append(
            ChunkRecord(
                chunk_id=chunk.chunk_id,
                note_id=chunk.note_id,
                file_path=chunk.file_path,
                title=chunk.title,
                heading_path=chunk.heading_path,
                kind=chunk.kind,
                updated_at=chunk.updated_at,
                text=chunk.text,
                source_hash=chunk.source_hash,
                vector=vector,
            )
        )

    summary = {
        "provider": config.provider,
        "openai_model": config.openai_model,
        "openai_base_url": config.openai_base_url,
        "built_at": _utc_now_iso(),
        "repo_root": str(config.repo_root),
        "source_count": len(stats),
        "source_fingerprint": snapshot["source_fingerprint"],
        "source_latest_mtime": snapshot["source_latest_mtime"],
        "chunk_count": None,
    }
    store.apply_incremental(
        updated_sources=updated_sources,
        updated_chunks=updated_chunks,
        removed_file_paths=removed,
        meta=summary,
    )
    summary["chunk_count"] = store.chunk_count()
    store.apply_incremental(updated_sources=[], updated_chunks=[], removed_file_paths=[], meta=summary)
    _write_registry_from_store(config, store)
    config.index_manifest_path.write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    refresh_reason = "source fingerprint changed"
    if len(current_by_path) != len(stored_sources):
        refresh_reason = "source count changed"
    return IndexRefreshResult(refreshed=True, reason=refresh_reason, summary=summary)


def read_note(config: MemoryConfig, note_id: str) -> dict[str, Any]:
    path = (config.repo_root / note_id).resolve()
    try:
        path.relative_to(config.repo_root.resolve())
    except Exception:
        raise ValueError(f"note_id escapes repo root: {note_id}")
    if not path.is_file():
        raise FileNotFoundError(f"note not found: {note_id}")
    text = path.read_text(encoding="utf-8", errors="replace")
    return {
        "note_id": note_id,
        "file_path": note_id,
        "updated_at": _mtime_iso(path),
        "content": text,
    }


def make_provider(config: MemoryConfig, *, provider_override: str | None = None) -> EmbeddingProvider:
    provider = (provider_override or config.provider or "hash").strip().lower()
    if provider == "openai":
        api_key = os.environ.get("CODOXEAR_MEMORY_OPENAI_API_KEY", "").strip() or os.environ.get("OPENAI_API_KEY", "").strip()
        base_url = os.environ.get("CODOXEAR_MEMORY_OPENAI_BASE_URL", "").strip() or os.environ.get("OPENAI_BASE_URL", config.openai_base_url).strip() or config.openai_base_url
        return OpenAIEmbeddingProvider(model=config.openai_model, api_key=api_key, base_url=base_url)
    return HashEmbeddingProvider()


def classify_kind(rel: str) -> str:
    if rel == "AGENTS.md":
        return "guide"
    if rel.startswith(f"{DEFAULT_MEMORY_DOCS_DIR}/features/"):
        return "feature"
    if rel.startswith(f"{DEFAULT_MEMORY_DOCS_DIR}/flows/"):
        return "flow"
    if rel.startswith(f"{DEFAULT_MEMORY_DOCS_DIR}/records/"):
        return "record"
    return "note"


def lexical_overlap_score(query_tokens: set[str], doc_tokens: set[str]) -> float:
    if not query_tokens or not doc_tokens:
        return 0.0
    common = len(query_tokens & doc_tokens)
    if common <= 0:
        return 0.0
    return common / max(len(query_tokens), 1)


def compact_snippet(text: str, *, max_chars: int = 280) -> str:
    cleaned = " ".join(text.split())
    if len(cleaned) <= max_chars:
        return cleaned
    return cleaned[: max_chars - 1].rstrip() + "…"


def slugify(text: str) -> str:
    raw = re.sub(r"[^a-z0-9]+", "-", text.lower())
    return raw.strip("-")


def _tokenize(text: str) -> list[str]:
    return [tok.lower() for tok in WORD_RE.findall(text)]


def _file_hash(path: Path) -> str:
    data = path.read_bytes()
    return hashlib.sha256(data).hexdigest()


def _mtime_iso(path: Path) -> str:
    return datetime.datetime.fromtimestamp(path.stat().st_mtime, tz=datetime.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def _utc_now_iso() -> str:
    return datetime.datetime.now(tz=datetime.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def _write_registry_from_store(config: MemoryConfig, store: MemoryStore) -> None:
    config.registry_path.parent.mkdir(parents=True, exist_ok=True)
    sources = sorted(store.all_sources(), key=lambda source: source.file_path)
    chunks = sorted(store.all_chunks(), key=lambda chunk: (chunk.file_path, chunk.chunk_id))
    with config.registry_path.open("w", encoding="utf-8") as registry_fp, config.chunks_path.open("w", encoding="utf-8") as chunks_fp:
        for source in sources:
            registry_fp.write(
                json.dumps(
                    {
                        "note_id": source.note_id,
                        "file_path": source.file_path,
                        "kind": source.kind,
                        "updated_at": source.updated_at,
                        "title": Path(source.file_path).stem,
                    },
                    ensure_ascii=False,
                )
                + "\n"
            )
        for chunk in chunks:
            chunks_fp.write(
                json.dumps(
                    {
                        "chunk_id": chunk.chunk_id,
                        "note_id": chunk.note_id,
                        "file_path": chunk.file_path,
                        "title": chunk.title,
                        "heading_path": chunk.heading_path,
                        "kind": chunk.kind,
                        "updated_at": chunk.updated_at,
                        "text": chunk.text,
                    },
                    ensure_ascii=False,
                )
                + "\n"
            )


def _print_json(obj: Any) -> None:
    sys.stdout.write(json.dumps(obj, ensure_ascii=False, indent=2) + "\n")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Build or query the local .memory index for Codoxear .memory/docs + AGENTS.md")
    parser.add_argument("--root", default=".", help="repo root containing .memory/docs/ and AGENTS.md")
    parser.add_argument("--search", default="", help="query to search after loading the index")
    parser.add_argument("--top-k", type=int, default=5, help="number of matches to return")
    parser.add_argument("--kind", default="", help="optional kind filter")
    parser.add_argument("--path-prefix", default="", help="optional file path prefix filter")
    parser.add_argument("--force", action="store_true", help="force a full rebuild even if the index is already current")
    args = parser.parse_args(argv)

    config = default_config(repo_root=Path(args.root))
    refresh = ensure_index_current(config, force=bool(args.force))
    summary = refresh.summary
    if not args.search:
        _print_json({"refreshed": refresh.refreshed, "reason": refresh.reason, "summary": summary})
        return 0
    matches = search_index(
        config,
        query=args.search,
        top_k=args.top_k,
        kind=args.kind or None,
        path_prefix=args.path_prefix or None,
    )
    _print_json({"refreshed": refresh.refreshed, "reason": refresh.reason, "summary": summary, "matches": [match.__dict__ for match in matches]})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
