from __future__ import annotations

import datetime
import hashlib
import json
import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from typing import Iterator

from .constants import CONTEXT_WINDOW_BASELINE_TOKENS
from .pi_log import pi_assistant_thinking_count
from .pi_log import pi_assistant_tool_use_count
from .pi_log import pi_assistant_text
from .pi_log import pi_assistant_is_final_turn_end
from .pi_log import pi_message_role
from .pi_log import pi_token_update
from .pi_log import pi_user_text
from .voice_push import ClassifiedAssistantMessage


_OAI_MEM_CITATION_TAIL_RE = re.compile(r"\s*<oai-mem-citation>\s*.*?</oai-mem-citation>\s*\Z", re.DOTALL)
_ASK_USER_TOOL_NAMES = {"ask_user", "AskUserQuestion"}


@dataclass(frozen=True)
class JsonlRecord:
    start: int
    end: int
    obj: dict[str, Any]


@dataclass(frozen=True)
class PositionedChatEvent:
    event: dict[str, Any]
    start: int
    end: int


def _parse_iso8601_to_epoch(ts: str) -> float | None:
    t = ts.strip()
    if t.endswith("Z"):
        t = t[:-1] + "+00:00"
    try:
        return datetime.datetime.fromisoformat(t).timestamp()
    except ValueError:
        return None


def _event_ts(obj: dict[str, Any]) -> float | None:
    ts = obj.get("ts")
    if isinstance(ts, (int, float)):
        return float(ts)
    ts2 = obj.get("timestamp")
    if isinstance(ts2, (int, float)):
        return float(ts2)
    if isinstance(ts2, str):
        v = _parse_iso8601_to_epoch(ts2)
        if v is not None:
            return float(v)
    return None


def _strip_oai_mem_citation_tail(text: str) -> str:
    # Delivery notifications should follow the assistant reply itself, not the appended memory-citation envelope.
    return _OAI_MEM_CITATION_TAIL_RE.sub("", text)


def _sidebar_conversation_ts(obj: dict[str, Any]) -> float | None:
    typ = obj.get("type")
    if typ == "event_msg":
        p = obj.get("payload")
        if not isinstance(p, dict):
            raise ValueError("invalid event_msg payload")
        pt = p.get("type")
        if pt == "user_message" and isinstance(p.get("message"), str):
            return _event_ts(obj)
        if pt in ("task_complete", "turn_complete"):
            last_msg = p.get("last_agent_message")
            if isinstance(last_msg, str) and last_msg.strip():
                return _event_ts(obj)
        if pt == "agent_message":
            msg = p.get("message")
            phase = p.get("phase")
            if isinstance(msg, str) and msg.strip() and phase == "final_answer":
                return _event_ts(obj)
        return None

    if typ == "message":
        if pi_user_text(obj):
            return _event_ts(obj)
        if pi_assistant_text(obj):
            return _event_ts(obj)
        return None

    if typ == "response_item":
        p = obj.get("payload")
        if not isinstance(p, dict):
            raise ValueError("invalid response_item payload")
        if p.get("type") != "message" or p.get("role") != "assistant":
            return None
        phase = p.get("phase")
        end_turn = p.get("end_turn")
        if phase != "final_answer" and end_turn is not True:
            return None
        content = p.get("content")
        if not isinstance(content, list):
            raise ValueError("invalid assistant message content")
        for part in content:
            if isinstance(part, dict) and part.get("type") == "output_text" and isinstance(part.get("text"), str) and part.get("text"):
                return _event_ts(obj)
        return None

    return None


def _context_percent_remaining(*, tokens_in_context: int, context_window: int) -> int:
    if context_window <= CONTEXT_WINDOW_BASELINE_TOKENS:
        return 0
    effective = context_window - CONTEXT_WINDOW_BASELINE_TOKENS
    used = max(tokens_in_context - CONTEXT_WINDOW_BASELINE_TOKENS, 0)
    remaining = max(effective - used, 0)
    return int(round((remaining / effective) * 100.0))


def _text_message_id(*, message_class: str, text: str, ts: float | None) -> str:
    ts_ms = int(round(ts * 1000.0)) if isinstance(ts, (int, float)) else None
    payload = json.dumps({"class": message_class, "text": " ".join(text.split()), "ts_ms": ts_ms}, ensure_ascii=False, sort_keys=True)
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def _non_empty_string(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    value = value.strip()
    return value or None


def _coerce_tool_arguments(args: Any) -> dict[str, Any]:
    if isinstance(args, str):
        try:
            args = json.loads(args)
        except (TypeError, ValueError):
            args = {}
    return args if isinstance(args, dict) else {}


def _tool_text_from_content(content: Any) -> str | None:
    if isinstance(content, list):
        parts: list[str] = []
        for item in content:
            if not isinstance(item, dict):
                continue
            block_type = item.get("type")
            text = item.get("text")
            if block_type in ("text", "output_text", "input_text") and isinstance(text, str) and text:
                parts.append(text)
        if parts:
            return "".join(parts)
    if isinstance(content, str) and content.strip():
        return content
    return None


def _tool_result_text(payload: dict[str, Any]) -> str | None:
    for key in ("content", "output", "text", "result"):
        value = payload.get(key)
        text = _tool_text_from_content(value)
        if text:
            return text
        if isinstance(value, (dict, list)) and value:
            return json.dumps(value, ensure_ascii=False, sort_keys=True)
    return None


def _tool_call_summary(name: str, args: dict[str, Any]) -> str | None:
    if name in _ASK_USER_TOOL_NAMES:
        question = _non_empty_string(args.get("question"))
        if question is not None:
            return question
        questions = args.get("questions")
        if isinstance(questions, list):
            for item in questions:
                if not isinstance(item, dict):
                    continue
                question = _non_empty_string(item.get("question"))
                if question is not None:
                    return question
        return None
    for key in ("command", "query", "prompt", "path", "file_path", "url", "subject"):
        value = _non_empty_string(args.get(key))
        if value is not None:
            return value
    return None


def _normalized_bool_arg(
    args: dict[str, Any], *keys: str, default: bool = False
) -> bool:
    for key in keys:
        if key in args:
            return bool(args.get(key))
    return default


def _normalize_ask_user_questions(questions: Any) -> list[dict[str, Any]]:
    if not isinstance(questions, list):
        return []
    normalized_questions: list[dict[str, Any]] = []
    for item in questions:
        if not isinstance(item, dict):
            continue
        question = item.get("question") if isinstance(item.get("question"), str) else ""
        header = item.get("header") if isinstance(item.get("header"), str) else ""
        options = item.get("options") if isinstance(item.get("options"), list) else []
        if not question:
            continue
        normalized_questions.append(
            {
                "header": header,
                "question": question,
                "options": [option for option in options],
                "multiSelect": _normalized_bool_arg(
                    item,
                    "allow_multiple",
                    "allowMultiple",
                    "multiSelect",
                ),
            }
        )
    return normalized_questions


def _normalize_ask_user_args(args: Any) -> dict[str, Any]:
    normalized = _coerce_tool_arguments(args)
    question = (
        normalized.get("question")
        if isinstance(normalized.get("question"), str)
        else ""
    )
    context = (
        normalized.get("context")
        if isinstance(normalized.get("context"), str)
        else ""
    )
    options = normalized.get("options")
    allow_freeform = _normalized_bool_arg(
        normalized, "allow_freeform", "allowFreeform", default=True
    )
    allow_multiple = _normalized_bool_arg(normalized, "allow_multiple", "allowMultiple")
    timeout_ms = next(
        (
            normalized.get(key)
            for key in ("timeout_ms", "timeoutMs", "timeout")
            if isinstance(normalized.get(key), int)
        ),
        None,
    )

    normalized_questions = _normalize_ask_user_questions(normalized.get("questions"))
    header = ""
    if normalized_questions:
        first = normalized_questions[0]
        question = first.get("question") if isinstance(first.get("question"), str) else question
        header = first.get("header") if isinstance(first.get("header"), str) else ""
        if not context and header:
            context = header
        first_options = first.get("options")
        if isinstance(first_options, list):
            options = first_options
        allow_freeform = _normalized_bool_arg(
            first, "allow_freeform", "allowFreeform", default=allow_freeform
        )
        allow_multiple = _normalized_bool_arg(
            first,
            "allow_multiple",
            "allowMultiple",
            "multiSelect",
            default=allow_multiple,
        )
        timeout_ms = next(
            (
                first.get(key)
                for key in ("timeout_ms", "timeoutMs", "timeout")
                if isinstance(first.get(key), int)
            ),
            timeout_ms,
        )

    result = {
        "question": question,
        "context": context,
        "options": list(options) if isinstance(options, list) else [],
        "allow_freeform": allow_freeform,
        "allow_multiple": allow_multiple,
        "timeout_ms": timeout_ms,
    }
    if header:
        result["header"] = header
    if normalized_questions:
        result["questions"] = normalized_questions
    return result


def _ask_user_event(
    args: Any, *, call_id: str | None, ts: float, resolved: bool = False
) -> dict[str, Any]:
    return {
        "type": "ask_user",
        "tool_call_id": call_id,
        **_normalize_ask_user_args(args),
        "resolved": resolved,
        "ts": float(ts),
    }


def _normalize_ask_user_answer(
    answer: Any, *, allow_multiple: bool
) -> str | list[str] | None:
    if isinstance(answer, str):
        return answer
    if allow_multiple and isinstance(answer, list):
        normalized = [item for item in answer if isinstance(item, str)]
        return normalized or None
    return None


def _normalize_ask_user_result(
    details: dict[str, Any],
    *,
    allow_multiple: bool,
    question: str = "",
    content_text: str = "",
) -> tuple[str | list[str] | None, bool]:
    answers = details.get("answers")
    if isinstance(answers, dict) and question:
        answer = _normalize_ask_user_answer(
            answers.get(question), allow_multiple=allow_multiple
        )
        if answer is not None:
            return answer, False

    answer = _normalize_ask_user_answer(
        details.get("answer"), allow_multiple=allow_multiple
    )
    was_custom = bool(details.get("wasCustom"))
    if answer is not None:
        return answer, was_custom

    response = details.get("response")
    if isinstance(response, dict):
        kind = response.get("kind") if isinstance(response.get("kind"), str) else ""
        selections = response.get("selections")
        if isinstance(selections, list):
            normalized = [item for item in selections if isinstance(item, str) and item]
            if normalized:
                if allow_multiple or len(normalized) > 1:
                    return normalized, was_custom or kind == "custom"
                return normalized[0], was_custom or kind == "custom"
        value = response.get("value")
        if isinstance(value, str) and value:
            return value, was_custom or kind == "custom"
        comment = response.get("comment")
        if isinstance(comment, str) and comment.strip():
            return comment.strip(), True

    if question and isinstance(content_text, str) and content_text.strip():
        match = re.search(rf'"{re.escape(question)}"\s*=\s*"([^"]+)"', content_text)
        if match:
            return match.group(1), was_custom

    return None, was_custom


def _single_chat_event(obj: dict[str, Any]) -> dict[str, Any] | None:
    typ = obj.get("type")
    if typ == "message":
        user_text = pi_user_text(obj)
        if isinstance(user_text, str) and user_text:
            ets = _event_ts(obj)
            evp: dict[str, Any] = {"role": "user", "text": user_text}
            if ets is not None:
                evp["ts"] = ets
            return evp

        assistant_text = pi_assistant_text(obj)
        if isinstance(assistant_text, str) and assistant_text:
            ets = _event_ts(obj)
            message_class = "final_response" if pi_assistant_is_final_turn_end(obj) else "narration"
            eva: dict[str, Any] = {
                "role": "assistant",
                "text": assistant_text,
                "message_class": message_class,
                "message_id": _text_message_id(message_class=message_class, text=assistant_text, ts=ets),
            }
            if ets is not None:
                eva["ts"] = ets
            return eva
        payload = obj.get("message")
        if not isinstance(payload, dict):
            return None
        role = payload.get("role")
        ets = _event_ts(obj)
        if role == "assistant":
            content = payload.get("content")
            if not isinstance(content, list):
                return None
            for item in content:
                if not isinstance(item, dict) or item.get("type") != "toolCall":
                    continue
                name = _non_empty_string(item.get("name")) or "tool"
                call_id = _non_empty_string(item.get("id"))
                args = _coerce_tool_arguments(item.get("arguments"))
                if name in _ASK_USER_TOOL_NAMES:
                    if ets is None:
                        return None
                    return _ask_user_event(args, call_id=call_id, ts=ets, resolved=False)
                ev: dict[str, Any] = {"type": "tool", "name": name}
                if ets is not None:
                    ev["ts"] = ets
                if call_id is not None:
                    ev["tool_call_id"] = call_id
                summary = _tool_call_summary(name, args)
                if summary is not None:
                    ev["text"] = summary
                return ev
        if role == "toolResult":
            name = _non_empty_string(payload.get("toolName")) or "tool"
            call_id = _non_empty_string(payload.get("toolCallId"))
            text = _tool_result_text(payload)
            if name in _ASK_USER_TOOL_NAMES and ets is not None:
                details = payload.get("details")
                details = details if isinstance(details, dict) else {}
                event = _ask_user_event({}, call_id=call_id, ts=ets, resolved=True)
                answer, was_custom = _normalize_ask_user_result(
                    details,
                    allow_multiple=bool(event.get("allow_multiple")),
                    question=str(event.get("question") or ""),
                    content_text=text or "",
                )
                if answer is not None:
                    event["answer"] = answer
                event["cancelled"] = bool(details.get("cancelled"))
                event["was_custom"] = was_custom
                return event
            if text is None and payload.get("isError") is not True:
                return None
            ev = {"type": "tool_result", "name": name}
            if ets is not None:
                ev["ts"] = ets
            if call_id is not None:
                ev["tool_call_id"] = call_id
            if text is not None:
                ev["text"] = text
            if payload.get("isError") is True:
                ev["is_error"] = True
            return ev
        return None

    if typ == "event_msg":
        p = obj.get("payload")
        if not isinstance(p, dict):
            raise ValueError("invalid event_msg payload")
        if p.get("type") != "user_message":
            return None
        msg = p.get("message")
        if not isinstance(msg, str):
            return None
        ets = _event_ts(obj)
        ev: dict[str, Any] = {"role": "user", "text": msg}
        if ets is not None:
            ev["ts"] = ets
        return ev

    if typ == "response_item":
        p = obj.get("payload")
        if not isinstance(p, dict):
            raise ValueError("invalid response_item payload")
        ets = _event_ts(obj)
        pt = p.get("type")
        if pt == "function_call":
            name = _non_empty_string(p.get("name")) or "tool"
            call_id = _non_empty_string(p.get("call_id"))
            args = _coerce_tool_arguments(p.get("arguments"))
            if name in _ASK_USER_TOOL_NAMES:
                if ets is None:
                    return None
                return _ask_user_event(args, call_id=call_id, ts=ets, resolved=False)
            ev: dict[str, Any] = {"type": "tool", "name": name}
            if ets is not None:
                ev["ts"] = ets
            if call_id is not None:
                ev["tool_call_id"] = call_id
            summary = _tool_call_summary(name, args)
            if summary is not None:
                ev["text"] = summary
            return ev
        if pt in ("custom_tool_call", "web_search_call", "local_shell_call"):
            raw_name = p.get("name")
            if pt == "web_search_call":
                name = "web_search"
            elif pt == "local_shell_call":
                name = "local_shell"
            else:
                name = _non_empty_string(raw_name) or "tool"
            call_id = _non_empty_string(p.get("call_id"))
            args = _coerce_tool_arguments(p.get("arguments"))
            if not args:
                args = p
            ev = {"type": "tool", "name": name}
            if ets is not None:
                ev["ts"] = ets
            if call_id is not None:
                ev["tool_call_id"] = call_id
            summary = _tool_call_summary(name, args)
            if summary is not None:
                ev["text"] = summary
            return ev
        if pt in ("function_call_output", "custom_tool_call_output"):
            call_id = _non_empty_string(p.get("call_id"))
            name = _non_empty_string(p.get("name")) or "tool"
            text = _tool_result_text(p)
            if name in _ASK_USER_TOOL_NAMES and ets is not None:
                details = p.get("details")
                details = details if isinstance(details, dict) else {}
                event = _ask_user_event({}, call_id=call_id, ts=ets, resolved=True)
                answer, was_custom = _normalize_ask_user_result(
                    details,
                    allow_multiple=bool(event.get("allow_multiple")),
                    question=str(event.get("question") or ""),
                    content_text=text or "",
                )
                if answer is not None:
                    event["answer"] = answer
                event["cancelled"] = bool(details.get("cancelled"))
                event["was_custom"] = was_custom
                return event
            if text is None and p.get("is_error") is not True:
                return None
            ev = {"type": "tool_result", "name": name}
            if ets is not None:
                ev["ts"] = ets
            if call_id is not None:
                ev["tool_call_id"] = call_id
            if text is not None:
                ev["text"] = text
            if p.get("is_error") is True:
                ev["is_error"] = True
            return ev
        if pt != "message" or p.get("role") != "assistant":
            return None
        content = p.get("content")
        if not isinstance(content, list):
            raise ValueError("invalid assistant message content")
        out_text_parts: list[str] = []
        for part in content:
            if not isinstance(part, dict):
                continue
            if part.get("type") == "output_text" and isinstance(part.get("text"), str):
                out_text_parts.append(part["text"])
        if not out_text_parts:
            return None
        text = "".join(out_text_parts)
        ets = _event_ts(obj)
        message_class = "final_response" if (p.get("phase") == "final_answer" or p.get("end_turn") is True) else "narration"
        ev2: dict[str, Any] = {
            "role": "assistant",
            "text": text,
            "message_class": message_class,
            "message_id": _text_message_id(message_class=message_class, text=text, ts=ets),
        }
        if ets is not None:
            ev2["ts"] = ets
        return ev2

    return None


def _extract_token_update(objs: list[dict[str, Any]]) -> dict[str, Any] | None:
    # Prefer the newest token_count in this batch.
    for obj in reversed(objs):
        pi_token = pi_token_update(obj)
        if pi_token is not None:
            return pi_token
        if obj.get("type") != "event_msg":
            continue
        p = obj.get("payload")
        if not isinstance(p, dict):
            raise ValueError("invalid token_count payload")
        if p.get("type") != "token_count":
            continue
        info = p.get("info")
        if not isinstance(info, dict) or not isinstance(info.get("total_token_usage"), dict):
            continue
        ctx = info.get("model_context_window")
        last = info.get("last_token_usage")
        if not isinstance(ctx, int) or not isinstance(last, dict):
            continue
        tt = last.get("total_tokens")
        if not isinstance(tt, int):
            continue
        return {
            "context_window": ctx,
            "tokens_in_context": tt,
            "tokens_remaining": max(ctx - tt, 0),
            "percent_remaining": _context_percent_remaining(tokens_in_context=tt, context_window=ctx),
            "baseline_tokens": CONTEXT_WINDOW_BASELINE_TOKENS,
            "as_of": obj.get("timestamp") if isinstance(obj.get("timestamp"), str) else None,
        }
    return None


def _pi_message_keeps_turn_busy(obj: dict[str, Any]) -> bool:
    role = pi_message_role(obj)
    if role == "toolResult":
        return True
    return (pi_assistant_thinking_count(obj) > 0) or (pi_assistant_tool_use_count(obj) > 0)


def _parse_jsonl_line(raw_line: bytes | str) -> dict[str, Any] | None:
    if isinstance(raw_line, bytes):
        try:
            line = raw_line.decode("utf-8")
        except UnicodeDecodeError:
            return None
    else:
        line = raw_line
    try:
        obj = json.loads(line)
    except json.JSONDecodeError:
        return None
    return obj if isinstance(obj, dict) else None


def _read_jsonl_tail(path: Path, max_bytes: int) -> list[dict[str, Any]]:
    with path.open("rb") as f:
        f.seek(0, os.SEEK_END)
        size = f.tell()
        start = max(0, size - max_bytes)
        f.seek(start)
        data = f.read()

    if not data:
        return []
    if start > 0:
        nl = data.find(b"\n")
        if nl >= 0:
            data = data[nl + 1 :]

    out: list[dict[str, Any]] = []
    for line in data.splitlines():
        obj = _parse_jsonl_line(line)
        if obj is not None:
            out.append(obj)
    return out


def _read_jsonl_records_from_offset(path: Path, offset: int, *, max_bytes: int) -> tuple[list[JsonlRecord], int]:
    with path.open("rb") as f:
        f.seek(0, os.SEEK_END)
        size = f.tell()
        start = max(0, min(int(offset), size))
        f.seek(start)
        target = max(1, int(max_bytes))
        chunk_size = max(64 * 1024, min(target, 1024 * 1024))
        data = f.read(target)
        if b"\n" not in data:
            extra: list[bytes] = []
            while True:
                chunk = f.read(chunk_size)
                if not chunk:
                    break
                extra.append(chunk)
                if b"\n" in chunk:
                    break
            if extra:
                data += b"".join(extra)

    if not data:
        return [], start

    last_nl = data.rfind(b"\n")
    if last_nl < 0:
        return [], start
    data = data[: last_nl + 1]
    new_off = start + last_nl + 1

    out: list[JsonlRecord] = []
    pos = start
    for raw_line in data.splitlines(keepends=True):
        end = pos + len(raw_line)
        line = raw_line.rstrip(b"\r\n")
        obj = _parse_jsonl_line(line)
        if obj is not None:
            out.append(JsonlRecord(start=pos, end=end, obj=obj))
        pos = end
    return out, new_off


def _iter_jsonl_objects_reverse(path: Path, *, block_bytes: int = 64 * 1024) -> Iterator[dict[str, Any]]:
    if block_bytes <= 0:
        raise ValueError("block_bytes must be positive")
    with path.open("rb") as f:
        f.seek(0, os.SEEK_END)
        offset = f.tell()
        carry = b""
        while offset > 0:
            read_size = min(block_bytes, offset)
            offset -= read_size
            f.seek(offset)
            chunk = f.read(read_size)
            data = chunk + carry
            parts = data.split(b"\n")
            if offset > 0:
                carry = parts[0]
                parts = parts[1:]
            else:
                carry = b""
            for raw_line in reversed(parts):
                line = raw_line.rstrip(b"\r")
                if not line:
                    continue
                obj = _parse_jsonl_line(line)
                if obj is not None:
                    yield obj
        if carry:
            line = carry.rstrip(b"\r")
            if line:
                obj = _parse_jsonl_line(line)
                if obj is not None:
                    yield obj


def _iter_jsonl_records_reverse(path: Path, *, before: int | None = None, block_bytes: int = 64 * 1024) -> Iterator[JsonlRecord]:
    if block_bytes <= 0:
        raise ValueError("block_bytes must be positive")
    with path.open("rb") as f:
        f.seek(0, os.SEEK_END)
        size = f.tell()
        end = size if before is None else max(0, min(int(before), size))
        offset = end
        carry = b""
        drop_trailing_partial = False
        if end > 0:
            f.seek(end - 1)
            drop_trailing_partial = f.read(1) != b"\n"
        while offset > 0:
            read_size = min(block_bytes, offset)
            offset -= read_size
            f.seek(offset)
            chunk = f.read(read_size)
            data = chunk + carry
            parts = data.split(b"\n")
            if drop_trailing_partial and parts:
                parts = parts[:-1]
                drop_trailing_partial = False
            if offset > 0:
                leading = parts[0] if parts else b""
                carry = leading
                parts = parts[1:] if parts else []
                pos = offset + len(leading) + 1
            else:
                carry = b""
                pos = 0
            batch: list[JsonlRecord] = []
            for raw_line in parts:
                start = pos
                end_off = start + len(raw_line) + 1
                pos = end_off
                line = raw_line.rstrip(b"\r")
                if not line:
                    continue
                obj = _parse_jsonl_line(line)
                if obj is not None:
                    batch.append(JsonlRecord(start=start, end=end_off, obj=obj))
            for record in reversed(batch):
                yield record


def _read_chat_page_reverse(
    log_path: Path,
    *,
    limit: int,
    before_byte: int | None = None,
    skip_events: int = 0,
) -> tuple[list[dict[str, Any]], int, bool, int]:
    size = int(log_path.stat().st_size)
    end = size if before_byte is None else max(0, min(int(before_byte), size))
    page_limit = max(0, int(limit))
    skip = max(0, int(skip_events))
    if page_limit <= 0 or end <= 0:
        return [], 0, False, size

    newest_first: list[PositionedChatEvent] = []
    skipped = 0
    has_older = False
    for record in _iter_jsonl_records_reverse(log_path, before=end):
        event = _single_chat_event(record.obj)
        if event is None:
            continue
        if skipped < skip:
            skipped += 1
            continue
        if len(newest_first) < page_limit:
            newest_first.append(PositionedChatEvent(event=event, start=record.start, end=record.end))
            continue
        has_older = True
        break

    newest_first.reverse()
    events = [item.event for item in newest_first]
    next_before = newest_first[0].start if newest_first else 0
    return events, next_before, has_older, size


def _read_chat_tail_page(log_path: Path, *, limit: int) -> tuple[list[dict[str, Any]], int, int, bool]:
    events, before_byte, has_older, after_byte = _read_chat_page_reverse(log_path, limit=limit, before_byte=None, skip_events=0)
    return events, before_byte, after_byte, has_older


def _read_chat_history_page(log_path: Path, *, before_byte: int, limit: int) -> tuple[list[dict[str, Any]], int, bool]:
    events, next_before, has_older, _after_byte = _read_chat_page_reverse(
        log_path,
        limit=limit,
        before_byte=before_byte,
        skip_events=0,
    )
    return events, next_before, has_older


def _read_chat_live_delta(
    log_path: Path,
    *,
    after_byte: int,
    max_bytes: int = 2 * 1024 * 1024,
) -> tuple[list[dict[str, Any]], int, dict[str, int], dict[str, bool], dict[str, Any], dict[str, Any] | None]:
    records, next_after = _read_jsonl_records_from_offset(log_path, after_byte, max_bytes=max_bytes)
    objs = [record.obj for record in records]
    events, meta, flags, diag = _extract_chat_events(objs)
    token_update = _extract_token_update(objs)
    return events, next_after, meta, flags, diag, token_update


def _find_latest_token_update(log_path: Path, max_scan_bytes: int = 32 * 1024 * 1024) -> dict[str, Any] | None:
    scan = min(256 * 1024, max_scan_bytes)
    if scan <= 0:
        return None
    while scan <= max_scan_bytes:
        token = _extract_token_update(_read_jsonl_tail(log_path, scan))
        if token is not None:
            return token
        scan *= 2


def _find_latest_turn_context(log_path: Path, max_scan_bytes: int = 8 * 1024 * 1024) -> dict[str, Any] | None:
    scan = min(256 * 1024, max_scan_bytes)
    if scan <= 0:
        return None
    while scan <= max_scan_bytes:
        objs = _read_jsonl_tail(log_path, scan)
        for obj in reversed(objs):
            if not isinstance(obj, dict):
                continue
            if obj.get("type") != "turn_context":
                continue
            payload = obj.get("payload")
            if isinstance(payload, dict):
                return payload
        scan *= 2
    return None
    return None


def _extract_chat_events(
    objs: list[dict[str, Any]],
) -> tuple[list[dict[str, Any]], dict[str, int], dict[str, bool], dict[str, Any]]:
    events: list[dict[str, Any]] = []
    total_thinking = 0
    total_tools = 0
    total_system = 0
    turn_start = False
    turn_end = False
    turn_aborted = False
    tool_names: set[str] = set()
    last_tool: str | None = None
    known_tool_names: dict[str, str] = {}
    pending_ask_user_calls: dict[str, dict[str, Any]] = {}
    def event_ts(o: dict[str, Any]) -> float | None:
        ts = o.get("ts")
        if isinstance(ts, (int, float)):
            return float(ts)
        ts2 = o.get("timestamp")
        if isinstance(ts2, (int, float)):
            return float(ts2)
        if isinstance(ts2, str):
            v = _parse_iso8601_to_epoch(ts2)
            if v is not None:
                return float(v)
        return None

    def text_message_id(*, message_class: str, text: str, ts: float | None) -> str:
        ts_ms = int(round(ts * 1000.0)) if isinstance(ts, (int, float)) else None
        payload = json.dumps({"class": message_class, "text": " ".join(text.split()), "ts_ms": ts_ms}, ensure_ascii=False, sort_keys=True)
        return hashlib.sha256(payload.encode("utf-8")).hexdigest()

    for obj in objs:
        typ = obj.get("type")
        if typ == "message":
            user_text = pi_user_text(obj)
            if isinstance(user_text, str) and user_text:
                turn_start = True
                ets = event_ts(obj)
                evp: dict[str, Any] = {"role": "user", "text": user_text}
                if ets is not None:
                    evp["ts"] = ets
                events.append(evp)
                continue

            assistant_text = pi_assistant_text(obj)
            tool_count = pi_assistant_tool_use_count(obj)
            thinking_count = pi_assistant_thinking_count(obj)
            if thinking_count > 0:
                total_thinking += thinking_count
            if tool_count > 0:
                total_tools += tool_count
                tool_names.add("pi_tool")
                last_tool = "pi_tool"
            payload = obj.get("message")
            if isinstance(payload, dict) and payload.get("role") == "assistant":
                content = payload.get("content")
                if isinstance(content, list):
                    ets = event_ts(obj)
                    for item in content:
                        if not isinstance(item, dict) or item.get("type") != "toolCall":
                            continue
                        name = _non_empty_string(item.get("name")) or "tool"
                        call_id = _non_empty_string(item.get("id"))
                        args = _coerce_tool_arguments(item.get("arguments"))
                        tool_names.add("pi_tool")
                        if call_id is not None:
                            known_tool_names[call_id] = name
                        if ets is None:
                            continue
                        if name in _ASK_USER_TOOL_NAMES:
                            event = _ask_user_event(args, call_id=call_id, ts=ets, resolved=False)
                            events.append(event)
                            if call_id is not None:
                                pending_ask_user_calls[call_id] = dict(event)
                            continue
                        event = {"type": "tool", "name": name, "ts": ets}
                        if call_id is not None:
                            event["tool_call_id"] = call_id
                        summary = _tool_call_summary(name, args)
                        if summary is not None:
                            event["text"] = summary
                        events.append(event)
            elif isinstance(payload, dict) and payload.get("role") == "toolResult":
                total_tools += 1
                tool_names.add("pi_tool")
                last_tool = "pi_tool"
                ets = event_ts(obj)
                call_id = _non_empty_string(payload.get("toolCallId"))
                name = _non_empty_string(payload.get("toolName")) or (
                    known_tool_names.get(call_id) if call_id is not None else None
                ) or "tool"
                details = payload.get("details")
                details = details if isinstance(details, dict) else {}
                text = _tool_result_text(payload)
                if ets is not None and (name in _ASK_USER_TOOL_NAMES or (call_id is not None and call_id in pending_ask_user_calls)):
                    base = dict(pending_ask_user_calls.get(call_id, _ask_user_event({}, call_id=call_id, ts=ets, resolved=True)))
                    base["resolved"] = True
                    base["ts"] = ets
                    answer, was_custom = _normalize_ask_user_result(
                        details,
                        allow_multiple=bool(base.get("allow_multiple")),
                        question=str(base.get("question") or ""),
                        content_text=text or "",
                    )
                    if answer is not None:
                        base["answer"] = answer
                    base["cancelled"] = bool(details.get("cancelled"))
                    base["was_custom"] = was_custom
                    events.append(base)
                elif text is not None or payload.get("isError") is True:
                    event = {"type": "tool_result", "name": name}
                    if ets is not None:
                        event["ts"] = ets
                    if call_id is not None:
                        event["tool_call_id"] = call_id
                    if text is not None:
                        event["text"] = text
                    if payload.get("isError") is True:
                        event["is_error"] = True
                    events.append(event)
            if isinstance(assistant_text, str) and assistant_text:
                ets = event_ts(obj)
                message_class = "final_response" if pi_assistant_is_final_turn_end(obj) else "narration"
                if message_class == "final_response":
                    turn_end = True
                eva: dict[str, Any] = {
                    "role": "assistant",
                    "text": assistant_text,
                    "message_class": message_class,
                    "message_id": text_message_id(message_class=message_class, text=assistant_text, ts=ets),
                }
                if ets is not None:
                    eva["ts"] = ets
                events.append(eva)
            continue

        if typ == "event_msg":
            p = obj.get("payload")
            if not isinstance(p, dict):
                raise ValueError("invalid event_msg payload")
            pt = p.get("type")
            if pt == "user_message":
                msg = p.get("message")
                if isinstance(msg, str):
                    turn_start = True
                    ets = event_ts(obj)
                    ev: dict[str, Any] = {"role": "user", "text": msg}
                    if ets is not None:
                        ev["ts"] = ets
                    events.append(ev)
                continue
            if pt == "agent_reasoning":
                total_thinking += 1
                continue
            if pt == "turn_aborted":
                turn_aborted = True
                continue
            if pt in ("task_complete", "turn_complete"):
                turn_end = True
                continue
            if pt == "token_count":
                continue
            if pt == "task_complete":
                turn_end = True
                continue

        if typ == "response_item":
            p = obj.get("payload")
            if not isinstance(p, dict):
                raise ValueError("invalid response_item payload")
            pt = p.get("type")
            if pt == "message":
                role = p.get("role")
                if role in ("developer", "system"):
                    total_system += 1
                    continue
                if role == "assistant":
                    content = p.get("content")
                    if not isinstance(content, list):
                        raise ValueError("invalid assistant message content")
                    out_text_parts: list[str] = []
                    for part in content:
                        if not isinstance(part, dict):
                            continue
                        if part.get("type") == "output_text" and isinstance(part.get("text"), str):
                            out_text_parts.append(part["text"])
                    if out_text_parts:
                        text = "".join(out_text_parts)
                        ets = event_ts(obj)
                        message_class = "final_response" if (p.get("phase") == "final_answer" or p.get("end_turn") is True) else "narration"
                        ev2: dict[str, Any] = {
                            "role": "assistant",
                            "text": text,
                            "message_class": message_class,
                            "message_id": text_message_id(message_class=message_class, text=text, ts=ets),
                        }
                        if ets is not None:
                            ev2["ts"] = ets
                        events.append(ev2)
                    continue

            if pt == "reasoning":
                total_thinking += 1
                continue
            if pt == "function_call":
                nm = p.get("name")
                call_id = _non_empty_string(p.get("call_id"))
                args = _coerce_tool_arguments(p.get("arguments"))
                if isinstance(nm, str) and nm:
                    tool_names.add(nm)
                    last_tool = nm
                    if call_id is not None:
                        known_tool_names[call_id] = nm
                total_tools += 1
                ets = event_ts(obj)
                if isinstance(nm, str) and nm and ets is not None:
                    if nm in _ASK_USER_TOOL_NAMES:
                        event = _ask_user_event(args, call_id=call_id, ts=ets, resolved=False)
                        events.append(event)
                        if call_id is not None:
                            pending_ask_user_calls[call_id] = dict(event)
                    else:
                        event = {"type": "tool", "name": nm, "ts": ets}
                        if call_id is not None:
                            event["tool_call_id"] = call_id
                        summary = _tool_call_summary(nm, args)
                        if summary is not None:
                            event["text"] = summary
                        events.append(event)
                continue
            if pt in ("custom_tool_call", "web_search_call", "local_shell_call"):
                total_tools += 1
                if pt == "web_search_call":
                    name = "web_search"
                elif pt == "local_shell_call":
                    name = "local_shell"
                else:
                    name = _non_empty_string(p.get("name")) or "tool"
                call_id = _non_empty_string(p.get("call_id"))
                if call_id is not None:
                    known_tool_names[call_id] = name
                tool_names.add(name)
                last_tool = name
                ets = event_ts(obj)
                if ets is not None:
                    args = _coerce_tool_arguments(p.get("arguments"))
                    if not args:
                        args = p
                    event = {"type": "tool", "name": name, "ts": ets}
                    if call_id is not None:
                        event["tool_call_id"] = call_id
                    summary = _tool_call_summary(name, args)
                    if summary is not None:
                        event["text"] = summary
                    events.append(event)
                continue
            if pt in ("function_call_output", "custom_tool_call_output"):
                total_tools += 1
                call_id = _non_empty_string(p.get("call_id"))
                name = _non_empty_string(p.get("name")) or (
                    known_tool_names.get(call_id) if call_id is not None else None
                ) or "tool"
                tool_names.add(name)
                last_tool = name
                ets = event_ts(obj)
                text = _tool_result_text(p)
                details = p.get("details")
                details = details if isinstance(details, dict) else {}
                if ets is not None and (name in _ASK_USER_TOOL_NAMES or (call_id is not None and call_id in pending_ask_user_calls)):
                    base = dict(pending_ask_user_calls.get(call_id, _ask_user_event({}, call_id=call_id, ts=ets, resolved=True)))
                    base["resolved"] = True
                    base["ts"] = ets
                    answer, was_custom = _normalize_ask_user_result(
                        details,
                        allow_multiple=bool(base.get("allow_multiple")),
                        question=str(base.get("question") or ""),
                        content_text=text or "",
                    )
                    if answer is not None:
                        base["answer"] = answer
                    base["cancelled"] = bool(details.get("cancelled"))
                    base["was_custom"] = was_custom
                    events.append(base)
                elif ets is not None and (text is not None or p.get("is_error") is True):
                    event = {"type": "tool_result", "name": name, "ts": ets}
                    if call_id is not None:
                        event["tool_call_id"] = call_id
                    if text is not None:
                        event["text"] = text
                    if p.get("is_error") is True:
                        event["is_error"] = True
                    events.append(event)
                continue

    return (
        events,
        {"thinking": total_thinking, "tool": total_tools, "system": total_system},
        {"turn_start": turn_start, "turn_end": turn_end, "turn_aborted": turn_aborted},
        {"tool_names": sorted(tool_names), "last_tool": last_tool},
    )


def _extract_delivery_messages(objs: list[dict[str, Any]]) -> list[ClassifiedAssistantMessage]:
    out: list[ClassifiedAssistantMessage] = []
    seen: set[str] = set()
    last_text_key: tuple[str, str] | None = None

    def _text_message_id(*, message_class: str, text: str, ts: float | None) -> str:
        ts_ms = int(round(ts * 1000.0)) if isinstance(ts, (int, float)) else None
        payload = json.dumps({"class": message_class, "text": " ".join(text.split()), "ts_ms": ts_ms}, ensure_ascii=False, sort_keys=True)
        return hashlib.sha256(payload.encode("utf-8")).hexdigest()

    for obj in objs:
        if not isinstance(obj, dict):
            continue
        typ = obj.get("type")
        message_class: str | None = None
        text = ""
        if typ == "message":
            text = pi_assistant_text(obj) or ""
            if not text.strip():
                continue
            message_class = "final_response" if pi_assistant_is_final_turn_end(obj) else "narration"
        elif typ == "event_msg":
            payload = obj.get("payload")
            if not isinstance(payload, dict):
                raise ValueError("invalid event_msg payload")
            if payload.get("type") != "agent_message":
                continue
            message = payload.get("message")
            if not isinstance(message, str) or not message.strip():
                continue
            text = message
            message_class = "final_response" if payload.get("phase") == "final_answer" else "narration"
        elif typ == "response_item":
            payload = obj.get("payload")
            if not isinstance(payload, dict):
                raise ValueError("invalid response_item payload")
            if payload.get("type") != "message" or payload.get("role") != "assistant":
                continue
            content = payload.get("content")
            if not isinstance(content, list):
                raise ValueError("invalid assistant message content")
            text_parts: list[str] = []
            for part in content:
                if isinstance(part, dict) and part.get("type") == "output_text" and isinstance(part.get("text"), str):
                    text_parts.append(part["text"])
            text = "".join(text_parts)
            if not text.strip():
                continue
            message_class = "final_response" if (payload.get("phase") == "final_answer" or payload.get("end_turn") is True) else "narration"
        else:
            continue
        text = _strip_oai_mem_citation_tail(text)
        if not text.strip():
            continue
        ts = _event_ts(obj)
        normalized_text = " ".join(text.split())
        text_key = (str(message_class), normalized_text)
        if last_text_key == text_key:
            continue
        message_id = _text_message_id(message_class=message_class, text=text, ts=ts)
        if message_id in seen:
            continue
        seen.add(message_id)
        last_text_key = text_key
        out.append(ClassifiedAssistantMessage(message_id=message_id, message_class=message_class, text=text, ts=ts))
    return out


def _read_chat_tail_snapshot(
    log_path: Path,
    *,
    min_events: int,
    initial_scan_bytes: int,
    max_scan_bytes: int,
) -> tuple[list[dict[str, Any]], dict[str, Any] | None, int, bool, int]:
    size = int(log_path.stat().st_size)
    scan = min(max(256 * 1024, int(initial_scan_bytes)), int(max_scan_bytes))
    if scan <= 0:
        return [], None, 0, True, size

    best_events: list[dict[str, Any]] = []
    best_token: dict[str, Any] | None = None
    while True:
        objs = _read_jsonl_tail(log_path, scan)
        events, _meta, _flags, _diag = _extract_chat_events(objs)
        best_events = events
        tok = _extract_token_update(objs)
        if tok is not None:
            best_token = tok
        if len(events) >= min_events or scan >= max_scan_bytes:
            break
        next_scan = min(scan * 2, max_scan_bytes)
        if next_scan <= scan:
            break
        scan = next_scan

    scan_complete = (size <= scan)
    return best_events, best_token, scan, scan_complete, size


def _read_chat_events_from_tail(
    log_path: Path,
    min_events: int = 120,
    max_scan_bytes: int = 128 * 1024 * 1024,
) -> list[dict[str, Any]]:
    events, _token, _scan_bytes, _scan_complete, _size = _read_chat_tail_snapshot(
        log_path,
        min_events=min_events,
        initial_scan_bytes=min(256 * 1024, max_scan_bytes),
        max_scan_bytes=max_scan_bytes,
    )
    return events


def _has_assistant_output_text(obj: dict[str, Any]) -> bool:
    if obj.get("type") == "message":
        return bool(pi_assistant_text(obj))
    p = obj.get("payload")
    if not isinstance(p, dict):
        raise ValueError("invalid response_item payload")
    if p.get("type") != "message" or p.get("role") != "assistant":
        return False
    content = p.get("content")
    if not isinstance(content, list):
        raise ValueError("invalid assistant message content")
    for part in content:
        if isinstance(part, dict) and part.get("type") == "output_text" and isinstance(part.get("text"), str) and part.get("text"):
            return True
    return False


def _analyze_log_chunk(
    objs: list[dict[str, Any]],
) -> tuple[int, int, int, float | None, float | None, dict[str, Any] | None, list[dict[str, Any]]]:
    d_th = 0
    d_tools = 0
    d_sys = 0
    last_chat_ts: float | None = None
    last_assistant_ts: float | None = None
    token_update = _extract_token_update(objs)
    chat_events, _meta, _flags, _diag = _extract_chat_events(objs)

    for obj in objs:
        typ = obj.get("type")
        sidebar_ts = _sidebar_conversation_ts(obj)
        if sidebar_ts is not None:
            last_chat_ts = sidebar_ts
        if typ == "message":
            if pi_user_text(obj):
                d_th = 0
                d_tools = 0
                d_sys = 0
                continue
            d_th += pi_assistant_thinking_count(obj)
            d_tools += pi_assistant_tool_use_count(obj)
            continue
        if typ == "event_msg":
            p = obj.get("payload")
            if not isinstance(p, dict):
                raise ValueError("invalid event_msg payload")
            pt = p.get("type")
            if pt == "agent_reasoning":
                d_th += 1
            if pt == "user_message":
                d_th = 0
                d_tools = 0
                d_sys = 0
        if typ == "response_item":
            p = obj.get("payload")
            if not isinstance(p, dict):
                raise ValueError("invalid response_item payload")
            pt = p.get("type")
            if pt == "reasoning":
                d_th += 1
            if pt in (
                "function_call",
                "function_call_output",
                "custom_tool_call",
                "custom_tool_call_output",
                "web_search_call",
                "local_shell_call",
            ):
                d_tools += 1
            if pt == "message" and p.get("role") in ("developer", "system"):
                d_sys += 1

    return d_th, d_tools, d_sys, last_chat_ts, last_assistant_ts, token_update, chat_events


def _last_conversation_ts_from_tail(
    log_path: Path,
    *,
    max_scan_bytes: int | None = None,
) -> float | None:
    # Keep the argument for compatibility with older callers, but recover the
    # last conversation timestamp exactly by scanning JSONL records backward.
    _ = max_scan_bytes
    for obj in _iter_jsonl_objects_reverse(log_path):
        ts = _sidebar_conversation_ts(obj)
        if ts is not None:
            return ts
    return None


def _last_assistant_ts_from_tail(
    path: Path,
    *,
    max_scan_bytes: int,
) -> float | None:
    scan = 256 * 1024
    while True:
        objs = _read_jsonl_tail(path, scan)
        last_assistant: float | None = None
        for obj in objs:
            typ = obj.get("type")
            if typ == "event_msg":
                p = obj.get("payload")
                if not isinstance(p, dict):
                    raise ValueError("invalid event_msg payload")
                pt = p.get("type")
                if pt == "agent_message":
                    msg = p.get("message")
                    if isinstance(msg, str) and msg.strip():
                        ts = _event_ts(obj)
                        if ts is not None:
                            last_assistant = float(ts)
                    continue
            if typ == "response_item" and _has_assistant_output_text(obj):
                ts = _event_ts(obj)
                if ts is not None:
                    last_assistant = float(ts)
                continue
        if last_assistant is not None:
            return float(last_assistant)
        if scan >= max_scan_bytes:
            return None
        scan *= 2


def _compute_idle_from_log(path: Path, max_scan_bytes: int = 8 * 1024 * 1024) -> bool | None:
    sz = int(path.stat().st_size)

    scan = min(256 * 1024, max_scan_bytes)
    if scan <= 0:
        return None
    objs: list[dict[str, Any]] = []
    saw_terminal_signal = False
    idle = True

    while True:
        objs = _read_jsonl_tail(path, scan)
        saw_terminal_signal = False
        idle = True
        for obj in objs:
            typ = obj.get("type")
            if typ == "message":
                if pi_user_text(obj):
                    saw_terminal_signal = True
                    idle = False
                    continue
                if pi_assistant_text(obj):
                    saw_terminal_signal = True
                    idle = pi_assistant_is_final_turn_end(obj)
                    continue
                if _pi_message_keeps_turn_busy(obj):
                    saw_terminal_signal = True
                    idle = False
                    continue
            if typ == "event_msg":
                p = obj.get("payload")
                if not isinstance(p, dict):
                    raise ValueError("invalid event_msg payload")
                pt = p.get("type")
                if pt == "user_message" and isinstance(p.get("message"), str):
                    saw_terminal_signal = True
                    idle = False
                    continue
                if pt == "agent_message":
                    msg = p.get("message")
                    if isinstance(msg, str) and msg.strip():
                        saw_terminal_signal = True
                        idle = False
                    continue
                if pt == "agent_reasoning":
                    saw_terminal_signal = True
                    idle = False
                    continue
                if pt in ("turn_aborted", "thread_rolled_back", "task_complete", "turn_complete"):
                    saw_terminal_signal = True
                    idle = True
                    continue
            if typ == "response_item":
                p = obj.get("payload")
                if not isinstance(p, dict):
                    raise ValueError("invalid response_item payload")
                pt = p.get("type")
                if _has_assistant_output_text(obj):
                    saw_terminal_signal = True
                    idle = (p.get("end_turn") is True)
                    continue
                if pt == "reasoning":
                    saw_terminal_signal = True
                    idle = False
                    continue
                if pt in (
                    "function_call",
                    "function_call_output",
                    "custom_tool_call",
                    "custom_tool_call_output",
                    "web_search_call",
                    "local_shell_call",
                ):
                    saw_terminal_signal = True
                    idle = False
                    continue

        if saw_terminal_signal or scan >= max_scan_bytes:
            break
        scan *= 2

    if not objs:
        return None

    if not saw_terminal_signal:
        return True if sz <= 128 * 1024 else False

    return idle


def _last_chat_role_ts_from_tail(
    path: Path,
    *,
    max_scan_bytes: int,
) -> tuple[str, float] | None:
    def event_ts(o: dict[str, Any]) -> float | None:
        ts = o.get("ts")
        if isinstance(ts, (int, float)):
            return float(ts)
        ts2 = o.get("timestamp")
        if isinstance(ts2, (int, float)):
            return float(ts2)
        if isinstance(ts2, str):
            v = _parse_iso8601_to_epoch(ts2)
            if v is not None:
                return float(v)
        return None

    scan = 256 * 1024
    while scan <= max_scan_bytes:
        objs = _read_jsonl_tail(path, scan)
        last_user: tuple[int, float | None] | None = None
        last_assistant: tuple[int, float | None] | None = None
        for i, obj in enumerate(objs):
            typ = obj.get("type")
            if typ == "message":
                if pi_user_text(obj):
                    last_user = (i, event_ts(obj))
                    continue
                if pi_assistant_text(obj) or _pi_message_keeps_turn_busy(obj):
                    last_assistant = (i, event_ts(obj))
                    continue
            if typ == "event_msg":
                p = obj.get("payload")
                if not isinstance(p, dict):
                    raise ValueError("invalid event_msg payload")
                pt = p.get("type")
                if pt == "user_message" and isinstance(p.get("message"), str):
                    last_user = (i, event_ts(obj))
                    continue
                if pt == "agent_message":
                    msg = p.get("message")
                    if isinstance(msg, str) and msg.strip():
                        last_assistant = (i, event_ts(obj))
                        continue
            if typ == "response_item" and _has_assistant_output_text(obj):
                last_assistant = (i, event_ts(obj))

        best: tuple[str, tuple[int, float | None]] | None = None
        if last_user is not None:
            best = ("user", last_user)
        if last_assistant is not None:
            if best is None or last_assistant[0] > best[1][0]:
                best = ("assistant", last_assistant)
        if best is not None:
            role, (_i, ts) = best
            if ts is None:
                return None
            return (role, float(ts))
        scan *= 2
    return None
