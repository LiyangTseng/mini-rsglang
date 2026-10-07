"""The parity prompt corpus (PAR-01/PAR-02): schema, loader and the sha256
used by sidecar.build_meta for reproducibility.

Task 1 implements generic corpus validation only. Plan 06-03 adds the
canonical-corpus-specific checks (category coverage, etc.) without changing
these names or CorpusItem's shape.

Standard library only.
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

CANONICAL_CORPUS = "fixtures/parity/corpus.json"

_VALID_ROLES = {"system", "user", "assistant"}
_VALID_KINDS = {"chat", "raw"}


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

    return items
