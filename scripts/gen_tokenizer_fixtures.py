#!/usr/bin/env python3
"""Generate the golden tokenizer fixtures from upstream's own `load_tokenizer()` (D-05..D-08).

Every fixture is produced by loading the real tokenizer for each model in
scripts/tokenizer_fixtures/models.py (via the vendored minisgl.utils.hf.load_tokenizer()) and
running every discovered scripts/tokenizer_fixtures/corpus_*.py module's generate() against it.

Usage:
  scripts/gen_tokenizer_fixtures.py            write fixtures/tokenizer/*.json
  scripts/gen_tokenizer_fixtures.py --out DIR  write them to DIR instead
  scripts/gen_tokenizer_fixtures.py --check    regenerate into a temp dir and diff with fixtures/tokenizer

Exit codes: 0 ok, 1 fixtures differ (--check), 2 environment error.
The model registry must stay in step with crates/rsg-tokenizer/tests/common/mod.rs.
"""

from __future__ import annotations

import argparse
import importlib
import json
import pkgutil
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
VENDOR_PY = REPO / "vendor" / "mini-sglang" / "python"
FIXTURES_DIR = REPO / "fixtures" / "tokenizer"
TOKENIZER_FIXTURES_PKG = "tokenizer_fixtures"

sys.path.insert(0, str(REPO / "scripts"))


class EnvError(Exception):
    pass


class GatedAccessError(Exception):
    """Raised when a gated model's `load_tokenizer()` call fails for an auth-shaped reason (a
    401/403 from huggingface_hub) -- the D-04 clean-skip signal, distinct from `EnvError` so a
    genuine bug (a broken template, a corrupted cache, a real network outage) still surfaces as
    a hard failure instead of being silently treated as "gated access unavailable" (T-04-08: the
    catch scope below is deliberately narrow, never a bare `except Exception`)."""

    def __init__(self, repo_id: str, detail: str):
        super().__init__(f"{repo_id}: {detail}")
        self.repo_id = repo_id
        self.detail = detail


def _is_auth_shaped(exc: BaseException) -> bool:
    """True iff `exc`'s cause/context chain contains huggingface_hub's `GatedRepoError` or
    `RepositoryNotFoundError`. These are the two exception types
    `transformers.AutoTokenizer.from_pretrained` re-raises (wrapped in a plain `OSError`, via
    `raise ... from e`) for a 403 gated-access denial or a 401 unauthenticated-to-gated/private
    repo `[VERIFIED: transformers/utils/hub.py, huggingface_hub/utils/_http.py, read directly
    this session]`. Any other exception (e.g. a connection error, a malformed template, a bug
    elsewhere) is left to propagate as a hard failure by the caller."""
    from huggingface_hub.errors import GatedRepoError, RepositoryNotFoundError

    seen: set[int] = set()
    cause: BaseException | None = exc
    while cause is not None and id(cause) not in seen:
        if isinstance(cause, (GatedRepoError, RepositoryNotFoundError)):
            return True
        seen.add(id(cause))
        cause = cause.__cause__ or cause.__context__
    return False


def _load_tokenizer_for_fixture(load_tokenizer, spec):
    """Load `spec`'s tokenizer; convert an auth-shaped hf-hub failure for a gated model into
    `GatedAccessError` (D-04). Every other failure -- a non-gated model's failure of any kind, or
    a gated model's non-auth-shaped failure -- propagates unchanged, to be wrapped as a hard
    `EnvError` by the caller."""
    try:
        return load_tokenizer(spec.repo_id)
    except Exception as exc:
        if spec.gated and _is_auth_shaped(exc):
            raise GatedAccessError(spec.repo_id, str(exc)) from exc
        raise


def _load_upstream():
    """Import `load_tokenizer` from the vendored tree, and nowhere else (T-04-01)."""
    sys.path.insert(0, str(VENDOR_PY))
    try:
        import minisgl
        from minisgl.utils.hf import load_tokenizer
    except ImportError as exc:
        raise EnvError(f"cannot import upstream load_tokenizer: {exc}") from exc
    origin = Path(list(minisgl.__path__)[0]).resolve()
    if not origin.is_relative_to(VENDOR_PY.resolve()):
        raise EnvError(f"minisgl resolved to {origin}, not the vendored tree under {VENDOR_PY}")
    return load_tokenizer


def _discover_corpus_modules():
    """Import every tokenizer_fixtures/corpus_*.py module exposing OUTPUT_NAME and generate."""
    package = importlib.import_module(TOKENIZER_FIXTURES_PKG)
    modules = []
    for info in pkgutil.iter_modules(package.__path__, prefix=f"{TOKENIZER_FIXTURES_PKG}."):
        name = info.name.rsplit(".", 1)[-1]
        if not name.startswith("corpus_"):
            continue
        module = importlib.import_module(info.name)
        if hasattr(module, "OUTPUT_NAME") and hasattr(module, "generate"):
            modules.append(module)
    return modules


def _models():
    from tokenizer_fixtures.models import MODELS

    return MODELS


def generate(out_dir: Path) -> tuple[int, set[str]]:
    """Write id_corpus.json plus every model's per-corpus fixture files; return (file count,
    slugs skipped for D-04 clean gated-access-unavailable reasons)."""
    load_tokenizer = _load_upstream()
    corpus_modules = _discover_corpus_modules()

    from tokenizer_fixtures.corpus_ids import CASES as ID_CASES

    out_dir.mkdir(parents=True, exist_ok=True)
    count = 0
    skipped: set[str] = set()

    id_corpus = {"cases": [{"name": name, "text": text} for name, text in ID_CASES]}
    (out_dir / "id_corpus.json").write_text(json.dumps(id_corpus, indent=2) + "\n")
    count += 1

    for spec in _models():
        try:
            tokenizer = _load_tokenizer_for_fixture(load_tokenizer, spec)
        except GatedAccessError as exc:
            print(f"SKIP {spec.slug}: gated access unavailable ({exc.detail})", file=sys.stderr)
            skipped.add(spec.slug)
            continue
        except Exception as exc:
            raise EnvError(
                f"failed to load tokenizer for {spec.slug} ({spec.repo_id}): {exc}"
            ) from exc
        model_dir = out_dir / spec.slug
        model_dir.mkdir(parents=True, exist_ok=True)
        for module in corpus_modules:
            cases = module.generate(tokenizer)
            payload = {"cases": cases}
            (model_dir / f"{module.OUTPUT_NAME}.json").write_text(
                json.dumps(payload, indent=2) + "\n"
            )
            count += 1

    return count, skipped


def _json_files(root: Path) -> set[Path]:
    if not root.exists():
        return set()
    return {p.relative_to(root) for p in root.rglob("*.json")}


def _is_under_skipped_model(rel: Path, skipped: set[str]) -> bool:
    return len(rel.parts) > 0 and rel.parts[0] in skipped


def check(committed: Path) -> int:
    """Regenerate into a temp dir and diff against the committed fixtures; return the exit code.

    A model skipped for D-04 gated-access-unavailable reasons is excluded from the diff
    entirely: its committed fixtures are left untouched, reported as neither a false "missing"
    (no generated counterpart) nor a false "stale" (bytes differ) result. Every other model's
    diff logic is unchanged.
    """
    with tempfile.TemporaryDirectory() as tmp:
        fresh = Path(tmp)
        count, skipped = generate(fresh)
        diffs = []
        fresh_files = _json_files(fresh)
        committed_files = _json_files(committed)
        for rel in sorted(committed_files - fresh_files):
            if _is_under_skipped_model(rel, skipped):
                continue
            diffs.append((str(rel), "committed fixture has no generated counterpart"))
        for rel in sorted(fresh_files - committed_files):
            diffs.append((str(rel), "generated file is missing from the committed fixtures"))
        for rel in sorted(fresh_files & committed_files):
            if (fresh / rel).read_bytes() != (committed / rel).read_bytes():
                diffs.append((str(rel), "bytes differ"))
    for rel, reason in diffs:
        print(f"DIFF {rel}: {reason}")
    if diffs:
        return 1
    print(f"gen_tokenizer_fixtures: fixtures match ({count} files)")
    return 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=FIXTURES_DIR, help="output directory")
    parser.add_argument(
        "--check", action="store_true", help="regenerate into a temp dir and diff"
    )
    args = parser.parse_args(argv)
    try:
        if args.check:
            return check(args.out)
        count, _skipped = generate(args.out)
    except EnvError as exc:
        print(f"gen_tokenizer_fixtures: error: {exc}", file=sys.stderr)
        return 2
    print(f"gen_tokenizer_fixtures: wrote {count} files to {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
