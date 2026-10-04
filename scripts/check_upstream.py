#!/usr/bin/env python3
"""Prove vendor/mini-sglang/ equals pristine upstream except for the files UPSTREAM.md lists (D-03),
and that the frozen Python frontend is untouched (D-04).

Online (default): fetch the pinned commit into a fresh temporary directory, then compare every
vendored path (committed, uncommitted and untracked-but-not-ignored) with the pristine tree. A path
differs when its bytes, its symlink-ness, its symlink target or its owner-execute bit differ.

  - Tier A and Tier C paths may never differ, even when listed (FROZEN_TIER_A).
  - Tier B paths may differ only when listed with shared backend fix "yes".
  - Any other differing path must be listed (UNLISTED_CHANGE).
  - A listed path that does not differ is STALE_LISTING.

--offline: no network. Requires the committed vendored tree to hash to the pinned upstream root
tree and no uncommitted change under the vendored directory, so it can only prove a fully pristine
tree (an empty modified table).

Standard library only, so it runs on the GPU box before any environment exists. Never writes under
the vendored directory. Exit codes: 0 OK, 1 violations, 2 environment error (git, network).
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

UPSTREAM_URL = "https://github.com/sgl-project/mini-sglang"
# Upstream commit -> its root tree hash, for --offline and for checking the fetched commit.
KNOWN_TREES = {
    "9a91cfafe754aa85daee49998176275667eb58f2": "02d3e4ad34ec00c88f549fd9d287a4588958d824",
}

# Paths relative to vendor/mini-sglang/. A trailing "/" covers the whole directory.
TIER_A = (  # frontend-only: never modifiable
    "python/minisgl/server/api_server.py",
    "python/minisgl/server/launch.py",
    "python/minisgl/server/__init__.py",
    "python/minisgl/__main__.py",
    "python/minisgl/shell.py",
    "python/minisgl/tokenizer/",
    "LICENSE",
)
TIER_B = (  # shared wire/config contract: only as a shared backend fix
    "python/minisgl/message/",
    "python/minisgl/core.py",
    "python/minisgl/utils/mp.py",
    "python/minisgl/scheduler/config.py",
    "python/minisgl/server/args.py",
    "python/minisgl/utils/hf.py",
)
TIER_C = (  # measurement tools: frozen for fair benchmarking, treated like Tier A
    "benchmark/",
    "python/minisgl/benchmark/",
)

LICENSE_ATTRIBUTION = "Copyright (c) 2026 sgl-project"
TABLE_HEADING = "## Modified files"
TABLE_HEADER = "| Path | Reason | Shared backend fix (yes/no) |"
TABLE_SEPARATOR = "|---|---|---|"
SHA_RE = re.compile(r"^[0-9a-f]{40}$")


class EnvError(Exception):
    """A failure of the environment (git, network, filesystem), not of the vendored tree."""


class ParseError(Exception):
    pass


def run(cmd: list[str], cwd: Path | None = None) -> bytes:
    try:
        proc = subprocess.run(cmd, cwd=cwd, capture_output=True)
    except OSError as e:
        raise EnvError(f"cannot run {cmd[0]}: {e}") from e
    if proc.returncode != 0:
        stderr = proc.stderr.decode(errors="replace").strip()
        raise EnvError(f"{' '.join(cmd)} failed (exit {proc.returncode}): {stderr}")
    return proc.stdout


def under(path: str, entries: tuple[str, ...]) -> bool:
    return any(path == e or (e.endswith("/") and path.startswith(e)) for e in entries)


def parse_modified_table(text: str) -> dict[str, str]:
    """Rows of the '## Modified files' table as {path: "yes"|"no"}."""
    lines = text.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.strip() == TABLE_HEADING)
    except StopIteration:
        raise ParseError(f"heading {TABLE_HEADING!r} not found") from None
    i = start + 1
    while i < len(lines) and not lines[i].lstrip().startswith("|"):
        if lines[i].startswith("## "):
            raise ParseError(f"no table under {TABLE_HEADING!r}")
        i += 1
    if i >= len(lines) or lines[i].strip() != TABLE_HEADER:
        raise ParseError(f"table header {TABLE_HEADER!r} not found under {TABLE_HEADING!r}")
    if i + 1 >= len(lines) or lines[i + 1].strip() != TABLE_SEPARATOR:
        raise ParseError(f"table separator {TABLE_SEPARATOR!r} must follow the header")
    rows: dict[str, str] = {}
    for line in lines[i + 2 :]:
        line = line.strip()
        if not line.startswith("|"):
            break
        cells = [c.strip() for c in line.strip("|").split("|")]
        if len(cells) != 3:
            raise ParseError(f"row needs 3 cells: {line!r}")
        path, _reason, fix = cells
        path = path.strip("`")
        if not path or path.startswith("/") or ".." in path.split("/"):
            raise ParseError(f"bad path in row: {line!r}")
        if fix not in ("yes", "no"):
            raise ParseError(f"shared backend fix must be 'yes' or 'no' in row: {line!r}")
        if path in rows:
            raise ParseError(f"duplicate row for {path!r}")
        rows[path] = fix
    return rows


def vendored_paths(repo: Path, vendor_rel: str) -> set[str]:
    """Committed, uncommitted and untracked-but-not-ignored paths under the vendored directory."""
    out = run(
        ["git", "-C", str(repo), "ls-files", "--cached", "--others", "--exclude-standard", "-z",
         "--", vendor_rel]
    )
    prefix = vendor_rel + "/"
    return {p[len(prefix) :] for p in out.decode().split("\0") if p.startswith(prefix)}


def pristine_paths(pristine: Path) -> set[str]:
    paths = set()
    for root, dirs, files in os.walk(pristine, followlinks=False):
        for name in dirs + files:
            full = os.path.join(root, name)
            if os.path.islink(full) or not os.path.isdir(full):
                paths.add(os.path.relpath(full, pristine).replace(os.sep, "/"))
    return paths


def describe(path: Path) -> tuple | None:
    """What makes a path equal: kind, symlink target or bytes plus the owner-execute bit."""
    try:
        st = os.lstat(path)
    except FileNotFoundError:
        return None
    if stat.S_ISLNK(st.st_mode):
        return ("symlink", os.readlink(path))
    if stat.S_ISREG(st.st_mode):
        return ("file", path.read_bytes(), bool(st.st_mode & stat.S_IXUSR))
    return ("other", stat.S_IFMT(st.st_mode))


def difference(pristine: tuple | None, vendored: tuple | None) -> str | None:
    if pristine == vendored:
        return None
    if pristine is None:
        return "added"
    if vendored is None:
        return "removed"
    if pristine[0] != vendored[0]:
        return f"modified ({pristine[0]} replaced by {vendored[0]})"
    if pristine[0] == "symlink":
        return "modified (symlink target)"
    if pristine[0] == "file" and pristine[1] == vendored[1]:
        return "modified (executable bit)"
    return "modified"


def compare(vendor_dir: Path, vendored: set[str], pristine_dir: Path, listed: dict[str, str]):
    """Violations (category, path, detail) from a per-path comparison, plus the path count."""
    violations = []
    pristine = pristine_paths(pristine_dir)
    union = vendored | pristine
    differing = set()
    for path in sorted(union):
        p = describe(pristine_dir / path) if path in pristine else None
        v = describe(vendor_dir / path) if path in vendored else None
        kind = difference(p, v)
        if kind is None:
            continue
        differing.add(path)
        fix = listed.get(path)
        if under(path, TIER_A + TIER_C):
            note = " (listed, but frozen paths can never change)" if fix else ""
            violations.append(("FROZEN_TIER_A", path, f"{kind} under a frozen path{note}"))
        elif fix is None:
            violations.append(("UNLISTED_CHANGE", path, f"{kind}, not listed in UPSTREAM.md"))
        elif under(path, TIER_B) and fix != "yes":
            violations.append(
                ("TIER_B_REQUIRES_SHARED_FIX", path,
                 f"{kind}; Tier B paths change only as a shared backend fix 'yes'")
            )
    for path in sorted(set(listed) - differing):
        violations.append(("STALE_LISTING", path, "listed in UPSTREAM.md but identical to pristine"))
    return violations, len(union)


def fetch_pristine(sha: str, workdir: Path) -> Path:
    clone = workdir / "upstream"
    run(["git", "clone", "--quiet", "--filter=blob:none", "--no-checkout", UPSTREAM_URL, str(clone)])
    tree = run(["git", "-C", str(clone), "rev-parse", f"{sha}^{{tree}}"]).decode().strip()
    if sha in KNOWN_TREES and tree != KNOWN_TREES[sha]:
        raise EnvError(f"fetched {sha} has root tree {tree}, expected {KNOWN_TREES[sha]}")
    tar = workdir / "pristine.tar"
    run(["git", "-C", str(clone), "archive", "-o", str(tar), sha])
    pristine = workdir / "pristine"
    pristine.mkdir()
    run(["tar", "-xf", str(tar), "-C", str(pristine)])
    return pristine


def check_offline(repo: Path, vendor_rel: str, sha: str, listed: dict[str, str]):
    violations = []
    if sha not in KNOWN_TREES:
        violations.append(("OFFLINE_UNSUPPORTED", "vendor/UPSTREAM_SHA",
                           f"no pinned tree hash for {sha}; run without --offline"))
        return violations, ""
    if listed:
        violations.append(("OFFLINE_UNSUPPORTED", "UPSTREAM.md",
                           f"{len(listed)} listed modifications; offline mode can only prove a "
                           "fully pristine tree, run without --offline"))
        return violations, ""
    expected = KNOWN_TREES[sha]
    tree = run(["git", "-C", str(repo), "rev-parse", f"HEAD:{vendor_rel}"]).decode().strip()
    if tree != expected:
        violations.append(("TREE_HASH_MISMATCH", vendor_rel,
                           f"committed tree {tree} != pristine {expected}; run without --offline "
                           "for a per-path diff"))
    status = run(["git", "-C", str(repo), "status", "--porcelain", "-z", "--untracked-files=all",
                  "--", vendor_rel]).decode().split("\0")
    prefix = vendor_rel + "/"
    i = 0
    while i < len(status):
        entry = status[i]
        i += 1
        if not entry:
            continue
        code, path = entry[:2], entry[3:]
        if code[0] in "RC":
            i += 1  # the rename/copy source follows as its own entry
        rel = path[len(prefix) :] if path.startswith(prefix) else path
        violations.append(("UNLISTED_CHANGE", rel, f"uncommitted change ({code.strip()})"))
    return violations, tree


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parent.parent)
    ap.add_argument("--vendor-dir", default="vendor/mini-sglang")
    ap.add_argument("--upstream-md", default="UPSTREAM.md")
    ap.add_argument("--sha-file", default="vendor/UPSTREAM_SHA")
    ap.add_argument("--pristine-dir", type=Path, help="use this extracted pristine tree, no fetch")
    ap.add_argument("--offline", action="store_true", help="tree-hash check only, no network")
    args = ap.parse_args(argv)

    repo = args.repo_root.resolve()
    vendor_rel = Path(args.vendor_dir).as_posix().rstrip("/")
    vendor_dir = repo / vendor_rel
    violations: list[tuple[str, str, str]] = []

    try:
        sha = (repo / args.sha_file).read_text().strip()
    except OSError as e:
        sha = ""
        violations.append(("UPSTREAM_SHA_INVALID", args.sha_file, f"cannot read: {e}"))
    if not violations and not SHA_RE.match(sha):
        violations.append(("UPSTREAM_SHA_INVALID", args.sha_file,
                           f"must hold a 40-char lowercase hex SHA, got {sha!r}"))
    listed: dict[str, str] = {}
    try:
        md = (repo / args.upstream_md).read_text()
        listed = parse_modified_table(md)
        if sha and sha not in md:
            violations.append(("UPSTREAM_MD_SHA_MISSING", args.upstream_md,
                               f"does not mention the pinned SHA {sha}"))
    except (OSError, ParseError) as e:
        violations.append(("UPSTREAM_MD_PARSE_ERROR", args.upstream_md, str(e)))
    lic = vendor_dir / "LICENSE"
    if not lic.is_file() or lic.is_symlink() or LICENSE_ATTRIBUTION not in lic.read_text(errors="replace"):
        violations.append(("LICENSE_MISSING", "LICENSE",
                           f"vendored LICENSE missing or lacks {LICENSE_ATTRIBUTION!r}"))

    blocking = {"UPSTREAM_SHA_INVALID", "UPSTREAM_MD_PARSE_ERROR"}
    summary = ""
    try:
        if any(cat in blocking for cat, _, _ in violations):
            pass  # cannot judge the tree without a SHA and a parsed table
        elif args.offline:
            found, tree = check_offline(repo, vendor_rel, sha, listed)
            violations += found
            summary = f"offline, tree {tree} matches pristine {sha[:7]}, {len(listed)} listed modifications"
        else:
            vendored = vendored_paths(repo, vendor_rel)
            if args.pristine_dir is not None:
                if not args.pristine_dir.is_dir():
                    raise EnvError(f"--pristine-dir {args.pristine_dir} is not a directory")
                found, n = compare(vendor_dir, vendored, args.pristine_dir, listed)
            else:
                workdir = Path(tempfile.mkdtemp(prefix="check_upstream-"))
                try:
                    found, n = compare(vendor_dir, vendored, fetch_pristine(sha, workdir), listed)
                finally:
                    shutil.rmtree(workdir, ignore_errors=True)
            violations += found
            summary = f"{n} paths compared, {len(listed)} listed modifications"
    except EnvError as e:
        print(f"check_upstream: ERROR {e}", file=sys.stderr)
        return 2

    for cat, path, detail in sorted(violations, key=lambda v: (v[1], v[0])):
        print(f"FAIL {cat} {path}: {detail}")
    if violations:
        print(f"check_upstream: FAIL ({len(violations)} violations)")
        return 1
    print(f"check_upstream: OK ({summary})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
