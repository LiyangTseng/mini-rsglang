"""TOK-03 detokenize-stream corpus (D-09): derived, not hand-written.

Replays the real token-id streams for the TOK-01 corpus's CJK/emoji/mixed-script cases
(`scripts/tokenizer_fixtures/corpus_ids.CASES`), one token at a time, through the real upstream
`DetokenizeManager` -- this is how actual multi-byte UTF-8 splits occur, never a synthetic id
sequence. Also adds one explicit `finished_eos` case (built from the `ascii_sentence` corpus
entry plus the model's real EOS id appended), exercising `detokenize.py`'s branch that excludes
the EOS token itself from `decoded_ids` when `finished=True`.
"""

from __future__ import annotations

import sys
from pathlib import Path

OUTPUT_NAME = "detok_streams"

# D-09: derived from the TOK-01 corpus's CJK/emoji/mixed-script entries -- real BPE tokenization
# is what actually produces multi-byte UTF-8 splits, so a synthetic id sequence would test a
# scenario that might not occur in practice.
_STREAM_CASE_NAMES = ("cjk_text", "emoji_with_zwj", "mixed_cjk_emoji_ascii")
_FINISHED_EOS_SOURCE_CASE = "ascii_sentence"


def _load_upstream_detokenizer():
    """Imports `DetokenizeManager`/`DetokenizeMsg` from the vendored tree, reusing
    `gen_tokenizer_fixtures.py`'s own origin-checked `_load_upstream()` (T-04-01) rather than
    duplicating the origin-check guard here."""
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
    from gen_tokenizer_fixtures import _load_upstream

    _load_upstream()  # runs the sys.path.insert(VENDOR_PY) + origin check; return value unused
    from minisgl.message import DetokenizeMsg
    from minisgl.tokenizer.detokenize import DetokenizeManager

    return DetokenizeManager, DetokenizeMsg


def _replay(manager, detokenize_msg_cls, uid: int, token_ids: list[int]) -> list[str]:
    """Replays `token_ids` one at a time through `manager`, `finished=True` only on the last id,
    returning the list of incremental chunks in call order."""
    chunks = []
    last_index = len(token_ids) - 1
    for i, tid in enumerate(token_ids):
        msg = detokenize_msg_cls(uid=uid, next_token=tid, finished=(i == last_index))
        [chunk] = manager.detokenize([msg])
        chunks.append(chunk)
    return chunks


def generate(tokenizer) -> list[dict]:
    DetokenizeManager, DetokenizeMsg = _load_upstream_detokenizer()

    from tokenizer_fixtures.corpus_ids import CASES

    case_text = dict(CASES)
    cases = []
    uid = 0

    for name in _STREAM_CASE_NAMES:
        token_ids = tokenizer.encode(case_text[name])
        manager = DetokenizeManager(tokenizer=tokenizer)
        chunks = _replay(manager, DetokenizeMsg, uid, token_ids)
        cases.append({"name": name, "token_ids": token_ids, "chunks": chunks})
        uid += 1

    # finished_eos (D-09): a short real encode with the model's real EOS id explicitly appended,
    # replayed with finished=True only on that trailing EOS call -- exercises the branch that
    # excludes the EOS token itself from decoded_ids.
    base_ids = tokenizer.encode(case_text[_FINISHED_EOS_SOURCE_CASE])
    token_ids = base_ids + [tokenizer.eos_token_id]
    manager = DetokenizeManager(tokenizer=tokenizer)
    chunks = _replay(manager, DetokenizeMsg, uid, token_ids)
    cases.append({"name": "finished_eos", "token_ids": token_ids, "chunks": chunks})

    return cases
