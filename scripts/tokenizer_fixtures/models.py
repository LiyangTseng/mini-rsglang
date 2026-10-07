"""The Python mirror of rsg_tokenizer's model registry (`crates/rsg-tokenizer/src/lib.rs`'s
`ModelSpec`/`QWEN3_0_6B` and `tests/common/mod.rs`'s `MODELS`). Single source of truth for both
this generator and the Rust test parametrization, by slug.

Only `qwen3-0.6b` is listed after this plan; Plan 04-04/04-05 append
`llama-3.2-1b-instruct`, never a parallel `if model == "llama"` code path.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass
class ModelSpec:
    slug: str
    repo_id: str
    gated: bool


MODELS: list[ModelSpec] = [
    ModelSpec(slug="qwen3-0.6b", repo_id="Qwen/Qwen3-0.6B", gated=False),
    ModelSpec(slug="llama-3.2-1b-instruct", repo_id="meta-llama/Llama-3.2-1B-Instruct", gated=True),
]
