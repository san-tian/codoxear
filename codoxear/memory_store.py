from __future__ import annotations

import json
import math
import sqlite3
from dataclasses import dataclass
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 2


@dataclass
class ChunkRecord:
    chunk_id: str
    note_id: str
    file_path: str
    title: str
    heading_path: list[str]
    kind: str
    updated_at: str
    text: str
    source_hash: str
    vector: list[float]


@dataclass
class SourceRecord:
    note_id: str
    file_path: str
    kind: str
    updated_at: str
    source_hash: str
    size: int
    mtime_ns: int


class MemoryStore:
    def __init__(self, db_path: Path) -> None:
        self.db_path = db_path
        self.db_path.parent.mkdir(parents=True, exist_ok=True)

    def connect(self) -> sqlite3.Connection:
        conn = sqlite3.connect(self.db_path)
        conn.row_factory = sqlite3.Row
        return conn

    def initialize(self) -> None:
        with self.connect() as conn:
            conn.executescript(
                """
                CREATE TABLE IF NOT EXISTS meta (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS chunks (
                    chunk_id TEXT PRIMARY KEY,
                    note_id TEXT NOT NULL,
                    file_path TEXT NOT NULL,
                    title TEXT NOT NULL,
                    heading_path_json TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    text TEXT NOT NULL,
                    source_hash TEXT NOT NULL,
                    vector_json TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_chunks_note_id ON chunks(note_id);
                CREATE INDEX IF NOT EXISTS idx_chunks_kind ON chunks(kind);
                CREATE INDEX IF NOT EXISTS idx_chunks_file_path ON chunks(file_path);

                CREATE TABLE IF NOT EXISTS sources (
                    file_path TEXT PRIMARY KEY,
                    note_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    source_hash TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    mtime_ns INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_sources_note_id ON sources(note_id);
                CREATE INDEX IF NOT EXISTS idx_sources_kind ON sources(kind);
                """
            )
            conn.execute(
                "INSERT OR REPLACE INTO meta(key, value) VALUES(?, ?)",
                ("schema_version", str(SCHEMA_VERSION)),
            )
            conn.commit()

    def replace_index(self, *, sources: list[SourceRecord], chunks: list[ChunkRecord], meta: dict[str, Any]) -> None:
        self.initialize()
        with self.connect() as conn:
            conn.execute("DELETE FROM chunks")
            conn.execute("DELETE FROM sources")
            conn.executemany(
                """
                INSERT INTO sources(file_path, note_id, kind, updated_at, source_hash, size, mtime_ns)
                VALUES(?, ?, ?, ?, ?, ?, ?)
                """,
                [
                    (
                        source.file_path,
                        source.note_id,
                        source.kind,
                        source.updated_at,
                        source.source_hash,
                        int(source.size),
                        int(source.mtime_ns),
                    )
                    for source in sources
                ],
            )
            conn.executemany(
                """
                INSERT INTO chunks(
                    chunk_id,
                    note_id,
                    file_path,
                    title,
                    heading_path_json,
                    kind,
                    updated_at,
                    text,
                    source_hash,
                    vector_json
                ) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                [
                    (
                        chunk.chunk_id,
                        chunk.note_id,
                        chunk.file_path,
                        chunk.title,
                        json.dumps(chunk.heading_path, ensure_ascii=False),
                        chunk.kind,
                        chunk.updated_at,
                        chunk.text,
                        chunk.source_hash,
                        json.dumps(chunk.vector, ensure_ascii=False),
                    )
                    for chunk in chunks
                ],
            )
            for key, value in meta.items():
                conn.execute(
                    "INSERT OR REPLACE INTO meta(key, value) VALUES(?, ?)",
                    (key, json.dumps(value, ensure_ascii=False)),
            )
            conn.commit()

    def apply_incremental(
        self,
        *,
        updated_sources: list[SourceRecord],
        updated_chunks: list[ChunkRecord],
        removed_file_paths: list[str],
        meta: dict[str, Any],
    ) -> None:
        self.initialize()
        targets = {path for path in removed_file_paths}
        targets.update(source.file_path for source in updated_sources)
        with self.connect() as conn:
            if targets:
                placeholders = ",".join("?" for _ in targets)
                conn.execute(f"DELETE FROM chunks WHERE file_path IN ({placeholders})", tuple(targets))
                conn.execute(f"DELETE FROM sources WHERE file_path IN ({placeholders})", tuple(targets))
            if updated_sources:
                conn.executemany(
                    """
                    INSERT INTO sources(file_path, note_id, kind, updated_at, source_hash, size, mtime_ns)
                    VALUES(?, ?, ?, ?, ?, ?, ?)
                    """,
                    [
                        (
                            source.file_path,
                            source.note_id,
                            source.kind,
                            source.updated_at,
                            source.source_hash,
                            int(source.size),
                            int(source.mtime_ns),
                        )
                        for source in updated_sources
                    ],
                )
            if updated_chunks:
                conn.executemany(
                    """
                    INSERT INTO chunks(
                        chunk_id,
                        note_id,
                        file_path,
                        title,
                        heading_path_json,
                        kind,
                        updated_at,
                        text,
                        source_hash,
                        vector_json
                    ) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                    [
                        (
                            chunk.chunk_id,
                            chunk.note_id,
                            chunk.file_path,
                            chunk.title,
                            json.dumps(chunk.heading_path, ensure_ascii=False),
                            chunk.kind,
                            chunk.updated_at,
                            chunk.text,
                            chunk.source_hash,
                            json.dumps(chunk.vector, ensure_ascii=False),
                        )
                        for chunk in updated_chunks
                    ],
                )
            for key, value in meta.items():
                conn.execute(
                    "INSERT OR REPLACE INTO meta(key, value) VALUES(?, ?)",
                    (key, json.dumps(value, ensure_ascii=False)),
                )
            conn.commit()

    def read_meta(self) -> dict[str, Any]:
        self.initialize()
        out: dict[str, Any] = {}
        with self.connect() as conn:
            for row in conn.execute("SELECT key, value FROM meta"):
                key = str(row["key"])
                raw = row["value"]
                try:
                    out[key] = json.loads(raw)
                except Exception:
                    out[key] = raw
        return out

    def all_chunks(self) -> list[ChunkRecord]:
        self.initialize()
        rows: list[ChunkRecord] = []
        with self.connect() as conn:
            for row in conn.execute(
                "SELECT chunk_id, note_id, file_path, title, heading_path_json, kind, updated_at, text, source_hash, vector_json FROM chunks"
            ):
                rows.append(
                    ChunkRecord(
                        chunk_id=str(row["chunk_id"]),
                        note_id=str(row["note_id"]),
                        file_path=str(row["file_path"]),
                        title=str(row["title"]),
                        heading_path=list(json.loads(row["heading_path_json"])),
                        kind=str(row["kind"]),
                        updated_at=str(row["updated_at"]),
                        text=str(row["text"]),
                        source_hash=str(row["source_hash"]),
                        vector=[float(x) for x in json.loads(row["vector_json"])],
                    )
                )
        return rows

    def all_sources(self) -> list[SourceRecord]:
        self.initialize()
        rows: list[SourceRecord] = []
        with self.connect() as conn:
            for row in conn.execute(
                "SELECT note_id, file_path, kind, updated_at, source_hash, size, mtime_ns FROM sources"
            ):
                rows.append(
                    SourceRecord(
                        note_id=str(row["note_id"]),
                        file_path=str(row["file_path"]),
                        kind=str(row["kind"]),
                        updated_at=str(row["updated_at"]),
                        source_hash=str(row["source_hash"]),
                        size=int(row["size"]),
                        mtime_ns=int(row["mtime_ns"]),
                    )
                )
        return rows

    def chunk_count(self) -> int:
        self.initialize()
        with self.connect() as conn:
            row = conn.execute("SELECT COUNT(*) AS n FROM chunks").fetchone()
        return int(row["n"] if row is not None else 0)


def cosine_similarity(a: list[float], b: list[float]) -> float:
    if not a or not b or len(a) != len(b):
        return 0.0
    dot = 0.0
    norm_a = 0.0
    norm_b = 0.0
    for av, bv in zip(a, b):
        dot += av * bv
        norm_a += av * av
        norm_b += bv * bv
    if norm_a <= 0.0 or norm_b <= 0.0:
        return 0.0
    return dot / math.sqrt(norm_a * norm_b)
