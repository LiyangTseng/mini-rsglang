#!/usr/bin/env python3
"""Enforce the experiment-data rules in docs/agents/experiments.md (ADR 0001).

Fails when:

  a. a file that existed under docs/experiments/*/runs/ at the merge base with the base ref
     (default origin/main) is modified, renamed or deleted. Committed runs are immutable; a wrong
     run is superseded by a new one. Skipped when there is no merge base or nothing existed under
     runs/ there.
  b. a *.manifest.json or an rsg-bench report (report.json / report.md) under docs/ sits outside
     docs/experiments/<YYYY-MM>-<slug>/runs/<YYYY-MM-DD>-<label>/.
  c. a relative link, image path or heading anchor in a tracked *.md file (outside vendor/ and
     .planning/) does not resolve.

Checks the working tree, so uncommitted changes count. Standard library only.
Exit codes: 0 OK, 1 violations, 2 environment error (git).
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import unquote

REPO_ROOT = Path(__file__).resolve().parents[1]
RUNS_RE = re.compile(r"^docs/experiments/[^/]+/runs/")
RUN_FILE_RE = re.compile(
    r"^docs/experiments/\d{4}-\d{2}-[a-z0-9]+(?:-[a-z0-9]+)*"
    r"/runs/\d{4}-\d{2}-\d{2}-[a-z0-9]+(?:-[a-z0-9]+)*/[^/]+$"
)
SKIP_MD_PREFIXES = ("vendor/", ".planning/")
REPORT_GENERATED_BY = "crates/rsg-bench report"
REPORT_MD_TITLE = "# Frontend Benchmark Report"

FENCE_RE = re.compile(r"^\s*(```|~~~)")
INLINE_CODE_RE = re.compile(r"(`+)(?:(?!\1).)+?\1")
LINK_RE = re.compile(r"!?\[(?:[^\[\]]|\[[^\]]*\])*\]\(\s*<?([^)\s>]+)>?(?:\s+\"[^\"]*\")?\s*\)")
REF_DEF_RE = re.compile(r"^\s{0,3}\[[^\]]+\]:\s*<?(\S+?)>?(?:\s+.*)?$")
HTML_ATTR_RE = re.compile(r"<(?:img|a)\b[^>]*?\b(?:src|href)=\"([^\"]+)\"", re.IGNORECASE)
HEADING_RE = re.compile(r"^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$")
HTML_ID_RE = re.compile(r"<a\s+(?:name|id)=\"([^\"]+)\"", re.IGNORECASE)
SCHEME_RE = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")


def git(*args: str) -> str:
    result = subprocess.run(
        ["git", *args], cwd=REPO_ROOT, capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {result.stderr.strip()}")
    return result.stdout


def tracked_and_untracked() -> list[str]:
    out = git("ls-files", "-z", "--cached", "--others", "--exclude-standard")
    return sorted({p for p in out.split("\0") if p and (REPO_ROOT / p).exists()})


# --- a. committed runs are immutable ---------------------------------------------


def check_immutable_runs(base_ref: str) -> list[str]:
    try:
        base = git("merge-base", "HEAD", base_ref).strip()
    except RuntimeError:
        print(f"check_experiments: (a) skipped: no merge base with {base_ref}")
        return []
    at_base = {
        p for p in git("ls-tree", "-r", "--name-only", base, "--", "docs/experiments").splitlines()
        if RUNS_RE.match(p)
    }
    if not at_base:
        print("check_experiments: (a) skipped: nothing under docs/experiments/*/runs/ at the merge base")
        return []
    problems = []
    # Without a pathspec so a run file moved out of docs/experiments still pairs up as a rename.
    for line in git("diff", "--name-status", "-M", base).splitlines():
        status, *paths = line.split("\t")
        old = paths[0]
        if old not in at_base:
            continue
        if status.startswith("R"):
            problems.append(f"{old}: committed run file renamed to {paths[1]}")
        elif status.startswith("D"):
            problems.append(f"{old}: committed run file deleted")
        else:
            problems.append(f"{old}: committed run file modified")
    return problems


# --- b. manifests and reports live in a run directory ----------------------------


def is_rsg_bench_report(path: str) -> bool:
    """Recognizes `rsg-bench report` output by content, whatever the file is called."""
    full = REPO_ROOT / path
    try:
        if path.endswith(".json"):
            doc = json.loads(full.read_text(encoding="utf-8"))
            return isinstance(doc, dict) and doc.get("generated_by") == REPORT_GENERATED_BY
        if path.endswith(".md"):
            with full.open(encoding="utf-8") as fh:
                return fh.readline().rstrip("\n") == REPORT_MD_TITLE
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        pass
    return False


def check_layout(files: list[str]) -> list[str]:
    problems = []
    for path in files:
        if not path.startswith("docs/"):
            continue
        if path.endswith(".manifest.json"):
            kind = "manifest"
        elif is_rsg_bench_report(path):
            kind = "rsg-bench report"
        else:
            continue
        if not RUN_FILE_RE.match(path):
            problems.append(
                f"{path}: {kind} outside docs/experiments/<YYYY-MM>-<slug>/runs/<YYYY-MM-DD>-<label>/"
            )
    return problems


# --- c. relative links and anchors resolve ---------------------------------------


def github_slug(text: str) -> str:
    text = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", text)  # links -> their text
    text = re.sub(r"<[^>]+>", "", text)  # inline HTML
    text = text.replace("`", "").strip().lower()
    text = re.sub(r"[^\w\- ]", "", text)
    return text.replace(" ", "-")


def markdown_lines(text: str) -> list[tuple[int, str]]:
    """Lines outside fenced code blocks, with inline code spans blanked."""
    out, in_fence = [], False
    for n, line in enumerate(text.splitlines(), 1):
        if FENCE_RE.match(line):
            in_fence = not in_fence
            continue
        if not in_fence:
            out.append((n, INLINE_CODE_RE.sub("", line)))
    return out


_anchor_cache: dict[Path, set[str]] = {}


def anchors(path: Path) -> set[str]:
    if path not in _anchor_cache:
        seen: dict[str, int] = {}
        found: set[str] = set()
        for _, line in markdown_lines(path.read_text(encoding="utf-8")):
            found.update(HTML_ID_RE.findall(line))
            m = HEADING_RE.match(line)
            if not m:
                continue
            slug = github_slug(m.group(2))
            count = seen.get(slug, 0)
            seen[slug] = count + 1
            found.add(slug if count == 0 else f"{slug}-{count}")
        _anchor_cache[path] = found
    return _anchor_cache[path]


def check_links(files: list[str]) -> list[str]:
    problems = []
    for rel in files:
        if not rel.endswith(".md") or rel.startswith(SKIP_MD_PREFIXES):
            continue
        src = REPO_ROOT / rel
        for n, line in markdown_lines(src.read_text(encoding="utf-8")):
            targets = LINK_RE.findall(line) + HTML_ATTR_RE.findall(line)
            m = REF_DEF_RE.match(line)
            if m:
                targets.append(m.group(1))
            for target in targets:
                if SCHEME_RE.match(target) or target.startswith("//"):
                    continue
                file_part, _, fragment = target.partition("#")
                file_part = unquote(file_part)
                if file_part.startswith("/"):
                    dest = REPO_ROOT / file_part.lstrip("/")
                else:
                    dest = (src.parent / file_part) if file_part else src
                if not dest.exists():
                    problems.append(f"{rel}:{n}: link target not found: {target}")
                    continue
                if fragment and dest.is_file() and dest.suffix == ".md":
                    if unquote(fragment).lower() not in anchors(dest.resolve()):
                        problems.append(f"{rel}:{n}: heading anchor not found: {target}")
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "--base", default="origin/main", help="ref whose merge base defines committed runs"
    )
    ns = parser.parse_args()
    try:
        files = tracked_and_untracked()
        problems = (
            [f"(a) {p}" for p in check_immutable_runs(ns.base)]
            + [f"(b) {p}" for p in check_layout(files)]
            + [f"(c) {p}" for p in check_links(files)]
        )
    except RuntimeError as exc:
        print(f"check_experiments: {exc}", file=sys.stderr)
        return 2
    for p in problems:
        print(p, file=sys.stderr)
    if problems:
        print(
            f"check_experiments: {len(problems)} violation(s); see docs/agents/experiments.md",
            file=sys.stderr,
        )
        return 1
    print("check_experiments: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
