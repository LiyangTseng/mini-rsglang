# Upstream: mini-sglang

The Python/CUDA backend and the frozen Python frontend are vendored unmodified from upstream mini-sglang.

## Source

- Repository: https://github.com/sgl-project/mini-sglang
- Commit: `9a91cfafe754aa85daee49998176275667eb58f2` (short `9a91cfa`)
- Commit date: Sun May 17 20:37:42 2026 +0800
- Commit title: "[Fix] Stabilize decode batch request order across TP ranks (#113)"
- Upstream root tree: `02d3e4ad34ec00c88f549fd9d287a4588958d824`
- Vendored at `vendor/mini-sglang/` via `git archive` of the commit above (all 121 tracked entries, including the `.dockerignore` symlink).
- The machine-readable SHA lives in `vendor/UPSTREAM_SHA` (created by plan 01-02 and read by the launcher, `rsg-wire` and the fixture manifest).

## License

mini-sglang is MIT licensed, "Copyright (c) 2026 sgl-project". The LICENSE file is kept verbatim at `vendor/mini-sglang/LICENSE` and must never be removed or edited. Any future modification of a vendored file stays under that MIT license.

## Frozen paths

Paths are relative to `vendor/mini-sglang/`.

**Tier A: frontend-only. Never modifiable, no exception.**

- `python/minisgl/server/api_server.py`
- `python/minisgl/server/launch.py`
- `python/minisgl/server/__init__.py`
- `python/minisgl/__main__.py`
- `python/minisgl/shell.py`
- `python/minisgl/tokenizer/`
- `LICENSE`

**Tier B: shared wire/config contract. Modifiable only as a row marked shared backend fix "yes"; any change also requires regenerating the wire fixtures.**

- `python/minisgl/message/`
- `python/minisgl/core.py`
- `python/minisgl/utils/mp.py`
- `python/minisgl/scheduler/config.py`
- `python/minisgl/server/args.py`
- `python/minisgl/utils/hf.py`

**Tier C: measurement tools, frozen for fair benchmarking. Treated like Tier A.**

- `benchmark/`
- `python/minisgl/benchmark/`

## Modified files

Paths are relative to `vendor/mini-sglang/`. Every vendored file that differs from pristine `9a91cfa` must have a row here. Phase 1 modifies zero vendored files (the readiness handshake is a launcher-side wrapper, per D-09).

| Path | Reason | Shared backend fix (yes/no) |
|---|---|---|

## Verification

`scripts/check_upstream.py` (delivered by plan 01-06) diffs `vendor/mini-sglang/` against pristine `9a91cfa` and enforces the tiers above. Offline proof of zero modifications: `git rev-parse HEAD:vendor/mini-sglang` equals `02d3e4ad34ec00c88f549fd9d287a4588958d824`.
