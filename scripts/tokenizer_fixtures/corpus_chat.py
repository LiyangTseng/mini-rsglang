"""TOK-02 chat corpus (D-12): 8 conversation shapes, with case 4 split into two genuinely
distinct fixture entries (empty-string system message vs. missing system key) -- 9 total.

`generate(tokenizer)` calls the real `tokenizer.apply_chat_template(messages, tokenize=False,
add_generation_prompt=True)` -- the exact call `tokenize.py` makes: no `tools` kwarg, no
`date_string` kwarg, ever passed.

Pitfall 3 (04-RESEARCH.md): a chat template that calls Jinja's `strftime_now(...)` global (Llama)
embeds the real wall-clock date into the rendered prompt, which would make a committed fixture
date-sensitive. `generate()` detects this generically -- by checking whether `tokenizer.
chat_template` contains the substring `"strftime_now"` -- rather than branching on model
identity, so there is never an `if model == "llama"` special case here. When detected, and
`now_override` is not None, the frozen clock from `_frozen_strftime_now` is installed only for
the duration of this call; the template's own `date_string` kwarg is still never passed, so the
real upstream call shape (`tokenize=False, add_generation_prompt=True`, no extra kwargs) is
unchanged. Qwen3's template has no `strftime_now` call, so its generation path is byte-for-byte
unaffected by this mechanism.
"""

from __future__ import annotations

from contextlib import contextmanager

OUTPUT_NAME = "chat_prompts"

# A fixed instant, chosen once, used ONLY to freeze `strftime_now` during fixture generation for
# any template that calls it (Llama). Production `strftime_now` callers (the Rust
# `build_environment(now_override=None)` path) still use the real clock -- this constant never
# reaches production code.
FROZEN_NOW = "06 Oct 2026"


@contextmanager
def _frozen_strftime_now(fixed: str):
    """Monkeypatches the `datetime` name inside `transformers.utils.chat_template_utils` so its
    nested `strftime_now(fmt)` global (`return datetime.now().strftime(fmt)`, read directly from
    that module this session) returns `fixed` unconditionally, regardless of `fmt` or the real
    wall-clock date -- mirroring the Rust `build_environment(Some(fixed))` override's own
    "returns the fixed string unconditionally" semantics (04-04 SUMMARY), rather than trying to
    match chrono's format-specifier behavior here. Restores the real `datetime` on exit, so this
    can never leak into an unrelated call elsewhere in the same process.
    """
    import transformers.utils.chat_template_utils as _chat_template_utils

    class _FrozenDateTime:
        @classmethod
        def now(cls, tz=None):
            return cls()

        def strftime(self, fmt):  # noqa: ARG002 - fmt ignored; always the frozen string
            return fixed

    original = _chat_template_utils.datetime
    _chat_template_utils.datetime = _FrozenDateTime
    try:
        yield
    finally:
        _chat_template_utils.datetime = original

_TOKYO_TRIP_USER_TURN = {
    "role": "user",
    "content": "Can you help me plan a trip to Tokyo?",
}


def _long_multi_turn() -> list[dict]:
    messages: list[dict] = [
        {"role": "system", "content": "You are a helpful assistant."},
    ]
    for i in range(1, 6):
        messages.append(
            {"role": "user", "content": f"User turn {i}: what about topic {i}?"}
        )
        messages.append(
            {
                "role": "assistant",
                "content": f"Assistant reply {i} about topic {i}.",
            }
        )
    return messages


# (name, messages) -- 9 entries covering D-12's 8 cases, with case 4 split in two.
CONVERSATIONS: list[tuple[str, list[dict]]] = [
    (
        "single_turn_no_system",
        [{"role": "user", "content": "Hello, how are you?"}],
    ),
    (
        "system_plus_single_turn",
        [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "What is 2+2?"},
        ],
    ),
    (
        "multi_turn",
        [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Hi"},
            {"role": "assistant", "content": "Hello! How can I help you today?"},
            {"role": "user", "content": "Tell me a joke."},
        ],
    ),
    (
        "system_empty_string",
        [{"role": "system", "content": ""}, _TOKYO_TRIP_USER_TURN],
    ),
    (
        "system_missing_key",
        [_TOKYO_TRIP_USER_TURN],
    ),
    (
        "consecutive_same_role",
        [
            {"role": "user", "content": "What's the capital of France?"},
            {"role": "user", "content": "And what about Germany?"},
        ],
    ),
    (
        "non_ascii_content",
        [
            {
                "role": "user",
                "content": "今天天气真好 😊 Let's go to 公园 for a walk! 🌳🚶",
            }
        ],
    ),
    ("long_multi_turn", _long_multi_turn()),
    (
        "tool_calling",
        [
            {"role": "user", "content": "What's the weather in Paris?"},
            {
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {
                        "function": {
                            "name": "get_weather",
                            "arguments": {"location": "Paris", "unit": "celsius"},
                        }
                    }
                ],
            },
        ],
    ),
]


def _render_all(tokenizer) -> list[dict]:
    return [
        {
            "name": name,
            "messages": messages,
            "prompt": tokenizer.apply_chat_template(
                messages,
                tokenize=False,
                add_generation_prompt=True,
            ),
        }
        for name, messages in CONVERSATIONS
    ]


def generate(tokenizer, now_override: str | None = FROZEN_NOW) -> list[dict]:
    chat_template = getattr(tokenizer, "chat_template", None) or ""
    if now_override is not None and "strftime_now" in chat_template:
        with _frozen_strftime_now(now_override):
            return _render_all(tokenizer)
    return _render_all(tokenizer)
