"""The parity prompt corpus (PAR-01/PAR-02): schema, loader and the sha256
used by sidecar.build_meta for reproducibility.

Task 1 (06-01) implemented generic corpus validation only. Plan 06-03 adds
the canonical-corpus-specific checks (category coverage, per-category
content properties, etc.) via validate_canonical, wired into load_corpus
when the resolved path is the repo's canonical fixtures/parity/corpus.json,
without changing load_corpus's/CorpusItem's existing names or shape.

Standard library only.
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .. import handshake

CANONICAL_CORPUS = "fixtures/parity/corpus.json"

_VALID_ROLES = {"system", "user", "assistant"}
_VALID_KINDS = {"chat", "raw"}

# D-01: the curated 128-item corpus, split across eight categories.
CATEGORIES = ("short", "long", "multi_turn", "code", "cjk", "emoji", "raw", "edge")

CANONICAL_COUNTS = {
    "short": 24,
    "long": 12,
    "multi_turn": 24,
    "code": 20,
    "cjk": 16,
    "emoji": 12,
    "raw": 12,
    "edge": 8,
}

# U+4E00-9FFF: CJK Unified Ideographs. U+3040-30FF: Hiragana + Katakana.
# U+AC00-D7AF: Hangul Syllables.
_CJK_RANGES = ((0x4E00, 0x9FFF), (0x3040, 0x30FF), (0xAC00, 0xD7AF))
_EMOJI_ZWJ = 0x200D
_EMOJI_MIN = 0x1F000


class CorpusError(ValueError):
    """Raised by load_corpus() with every validation problem found at once."""

    def __init__(self, errors):
        self.errors = list(errors)
        super().__init__("; ".join(self.errors))


@dataclass
class CorpusItem:
    id: str
    category: str
    kind: str  # "chat" or "raw"
    messages: "list[dict[str, str]] | None"
    prompt: "str | None"
    max_tokens: int
    source: str


def corpus_sha256(path: Path) -> str:
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def _is_plain_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def load_corpus(path: Path) -> "list[CorpusItem]":
    path = Path(path)
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise CorpusError([f"{path}: could not read: {exc}"]) from exc
    try:
        doc = json.loads(text)
    except json.JSONDecodeError as exc:
        raise CorpusError([f"{path}: invalid JSON: {exc}"]) from exc

    errors: "list[str]" = []
    if not isinstance(doc, dict):
        raise CorpusError([f"{path}: top level must be an object"])

    if doc.get("schema_version") != 1:
        errors.append(f"schema_version must be 1, got {doc.get('schema_version')!r}")

    items_raw = doc.get("items")
    if not isinstance(items_raw, list) or not items_raw:
        errors.append("items must be a non-empty list")
        items_raw = []

    seen_ids: set = set()
    items: "list[CorpusItem]" = []
    for i, raw in enumerate(items_raw):
        prefix = f"items[{i}]"
        if not isinstance(raw, dict):
            errors.append(f"{prefix}: must be an object")
            continue

        item_id = raw.get("id")
        if not isinstance(item_id, str) or not item_id:
            errors.append(f"{prefix}.id: must be a non-empty string")
        elif item_id in seen_ids:
            errors.append(f"{prefix}.id: duplicate id {item_id!r}")
        else:
            seen_ids.add(item_id)

        category = raw.get("category")
        if not isinstance(category, str) or not category:
            errors.append(f"{prefix}.category: must be a non-empty string")

        kind = raw.get("kind")
        if kind not in _VALID_KINDS:
            errors.append(f"{prefix}.kind: must be 'chat' or 'raw', got {kind!r}")

        messages = raw.get("messages")
        prompt = raw.get("prompt")
        if kind == "chat":
            if not isinstance(messages, list) or not messages:
                errors.append(f"{prefix}.messages: chat item needs a non-empty list")
            else:
                for j, msg in enumerate(messages):
                    mpath = f"{prefix}.messages[{j}]"
                    if not isinstance(msg, dict):
                        errors.append(f"{mpath}: must be an object")
                        continue
                    role = msg.get("role")
                    content = msg.get("content")
                    if role not in _VALID_ROLES:
                        errors.append(f"{mpath}.role: must be one of {sorted(_VALID_ROLES)}")
                    if not isinstance(content, str):
                        errors.append(f"{mpath}.content: must be a string")
                if (
                    isinstance(messages, list)
                    and messages
                    and isinstance(messages[-1], dict)
                    and messages[-1].get("role") != "user"
                ):
                    errors.append(f"{prefix}.messages: last message role must be 'user'")
        elif kind == "raw":
            if not isinstance(prompt, str) or not prompt:
                errors.append(f"{prefix}.prompt: raw item needs a non-empty string")

        max_tokens = raw.get("max_tokens")
        if not _is_plain_int(max_tokens) or not (1 <= max_tokens <= 4096):
            errors.append(f"{prefix}.max_tokens: must be an int in 1..4096, got {max_tokens!r}")

        source = raw.get("source", "")
        if not isinstance(source, str):
            errors.append(f"{prefix}.source: must be a string")

        items.append(
            CorpusItem(
                id=item_id if isinstance(item_id, str) else f"<invalid-{i}>",
                category=category if isinstance(category, str) else "",
                kind=kind if kind in _VALID_KINDS else "",
                messages=messages if isinstance(messages, list) else None,
                prompt=prompt if isinstance(prompt, str) else None,
                max_tokens=max_tokens if _is_plain_int(max_tokens) else 0,
                source=source if isinstance(source, str) else "",
            )
        )

    if errors:
        raise CorpusError(errors)

    if path.resolve() == handshake.repo_root() / CANONICAL_CORPUS:
        canonical_errors = validate_canonical(items)
        if canonical_errors:
            raise CorpusError(canonical_errors)

    return items


def _content_text(item: "CorpusItem") -> str:
    if item.kind == "raw":
        return item.prompt or ""
    if item.messages:
        return "".join(
            m.get("content", "") for m in item.messages if isinstance(m, dict)
        )
    return ""


def _messages_key(messages: "list[dict] | None"):
    if messages is None:
        return None
    return tuple(
        (m.get("role"), m.get("content")) for m in messages if isinstance(m, dict)
    )


def _contains_cjk(text: str) -> bool:
    return any(
        any(lo <= ord(c) <= hi for lo, hi in _CJK_RANGES) for c in text
    )


def _contains_emoji(text: str) -> bool:
    return any(ord(c) >= _EMOJI_MIN or ord(c) == _EMOJI_ZWJ for c in text)


def _has_whitespace_and_newline_run(item: "CorpusItem") -> bool:
    text = _content_text(item)
    if not text:
        return False
    return text != text.strip() and "\n\n\n" in text


def validate_canonical(items: "list[CorpusItem]") -> "list[str]":
    """Full-corpus checks for the curated 128-item canonical corpus (D-01).

    Returns every problem found; an empty list means the corpus matches the
    canonical shape exactly. Called by load_corpus only when the resolved
    path is the repo's canonical fixtures/parity/corpus.json, and directly
    by tests against corpora loaded from other paths.
    """
    errors: "list[str]" = []

    if len(items) != 128:
        errors.append(f"canonical corpus must have exactly 128 items, got {len(items)}")

    counts: "dict[str, int]" = {}
    for item in items:
        counts[item.category] = counts.get(item.category, 0) + 1

    for category in CATEGORIES:
        expected = CANONICAL_COUNTS[category]
        actual = counts.get(category, 0)
        if actual != expected:
            errors.append(f"category {category!r} has {actual} items, expected {expected}")
    for category in counts:
        if category not in CANONICAL_COUNTS:
            errors.append(f"unexpected category {category!r} with {counts[category]} items")

    seen_content: set = set()
    for item in items:
        is_raw_kind = item.kind == "raw"
        is_raw_category = item.category == "raw"
        if is_raw_category != is_raw_kind:
            errors.append(
                f"item {item.id!r}: category 'raw' must match kind 'raw' exactly "
                f"(category={item.category!r}, kind={item.kind!r})"
            )

        key = (item.kind, _messages_key(item.messages), item.prompt)
        if key in seen_content:
            errors.append(f"item {item.id!r}: duplicate content (same kind/messages/prompt)")
        else:
            seen_content.add(key)

        if item.category == "multi_turn":
            messages = item.messages or []
            if len(messages) < 3:
                errors.append(
                    f"item {item.id!r}: multi_turn item must have at least 3 messages, "
                    f"got {len(messages)}"
                )
            if not any(
                isinstance(m, dict) and m.get("role") == "assistant" for m in messages
            ):
                errors.append(
                    f"item {item.id!r}: multi_turn item must have at least one assistant message"
                )

        total_len = len(_content_text(item))
        if item.category == "long" and total_len < 2000:
            errors.append(
                f"item {item.id!r}: long item must have at least 2000 characters of "
                f"content, got {total_len}"
            )
        if item.category == "short" and total_len > 160:
            errors.append(
                f"item {item.id!r}: short item must have at most 160 characters of "
                f"content, got {total_len}"
            )

        if item.category == "cjk" and not _contains_cjk(_content_text(item)):
            errors.append(f"item {item.id!r}: cjk item must contain at least one CJK character")

        if item.category == "emoji" and not _contains_emoji(_content_text(item)):
            errors.append(
                f"item {item.id!r}: emoji item must contain at least one emoji code point"
            )

        if item.category != "edge" and item.max_tokens != 128:
            errors.append(
                f"item {item.id!r}: non-edge item must have max_tokens 128, "
                f"got {item.max_tokens}"
            )

    edge_items = [item for item in items if item.category == "edge"]
    if not any(item.max_tokens == 1 for item in edge_items):
        errors.append("edge category must contain at least one item with max_tokens 1")
    if not any(item.max_tokens == 2 for item in edge_items):
        errors.append("edge category must contain at least one item with max_tokens 2")
    if not any("<|im_start|>" in _content_text(item) for item in edge_items):
        errors.append(
            "edge category must contain at least one item whose user content includes "
            "a chat-template special-token literal (for example '<|im_start|>')"
        )
    if not any(_has_whitespace_and_newline_run(item) for item in edge_items):
        errors.append(
            "edge category must contain at least one item with leading and trailing "
            "whitespace plus a run of 3 or more newlines"
        )

    return errors
