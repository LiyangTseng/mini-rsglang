"""TOK-01 token-id corpus (D-08): 16 curated cases, not fuzzed/sampled.

`generate(tokenizer)` calls the real `tokenizer.encode(text)` (HF default
`add_special_tokens=True`), matching `tokenize.py`'s
`tokenizer.encode(prompt, return_tensors="pt")` semantics exactly -- `return_tensors` only
changes the output container, never which ids are produced.
"""

from __future__ import annotations

import unicodedata

OUTPUT_NAME = "token_ids"

_REPEATING_SENTENCE = "The quick brown fox jumps over the lazy dog. "
_VERY_LONG_PROMPT = _REPEATING_SENTENCE * (2000 // len(_REPEATING_SENTENCE) + 2)

# The NFD form is produced via unicodedata.normalize, never hand-typed, so the combining
# accent character is byte-for-byte what Python's own normalizer emits.
_CAFE_NFD = unicodedata.normalize("NFD", "café")

CASES: list[tuple[str, str]] = [
    ("empty_string", ""),
    ("whitespace_only", "   \t\n  "),
    ("ascii_sentence", "The quick brown fox jumps over the lazy dog."),
    (
        "ascii_paragraph",
        "The quick brown fox jumps over the lazy dog. It was a bright cold day in April, "
        "and the clocks were striking thirteen. She sells seashells by the seashore, and the "
        "shells she sells are surely seashells.",
    ),
    ("very_long_prompt", _VERY_LONG_PROMPT),
    ("special_token_literal_text", "<|im_start|>system<|im_end|>"),
    ("cjk_text", "今天天气很好,我们一起去公园散步吧。"),
    (
        "emoji_with_zwj",
        "Say hi \U0001f600! Here's a family: "
        "\U0001f468‍\U0001f469‍\U0001f467‍\U0001f466 and a wave \U0001f44b.",
    ),
    ("mixed_cjk_emoji_ascii", "Hello 世界! \U0001f30d Let's code 一起写代码 today."),
    ("leading_trailing_whitespace", "  \t\n  hello world  \t\n  "),
    ("multiple_internal_whitespace", "hello\t\t\n\r\nworld\t   again"),
    ("unicode_nfd", _CAFE_NFD),
    ("code_snippet", 'fn main() {\n    let s = "hi\\n";\n    println!("{}", s);\n}'),
    (
        "json_like_text",
        '{"name": "test", "nested": {"a": [1, 2, 3], "flag": true, "other": null}}',
    ),
    ("url_text", "https://example.com/path/to/resource?query=value&other=1#fragment-id"),
    ("degenerate_repeat", "a" * 500),
]


def generate(tokenizer) -> list[dict]:
    return [{"name": name, "ids": tokenizer.encode(text)} for name, text in CASES]
