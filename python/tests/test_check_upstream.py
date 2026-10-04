"""Hermetic tests of scripts/check_upstream.py (BASE-01, D-03/D-04), run against temp git repos.

Each test copies a template repo holding the committed vendored tree, UPSTREAM.md and
vendor/UPSTREAM_SHA, mutates it, and runs the script with --pristine-dir pointing at a
pristine extraction of the same tree. No network.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[2]
SCRIPT = REPO / "scripts" / "check_upstream.py"
VENDOR = "vendor/mini-sglang"
TABLE_HEADER = "| Path | Reason | Shared backend fix (yes/no) |\n|---|---|---|\n"


def _git(*args: str, cwd: Path) -> None:
    subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True)


def _extract(dest: Path, *archive_args: str) -> None:
    """git archive from the real repo, extracted into dest (keeps the symlink)."""
    dest.mkdir(parents=True, exist_ok=True)
    tar = dest.parent / f"{dest.name}.tar"
    subprocess.run(
        ["git", "-C", str(REPO), "archive", "-o", str(tar), *archive_args],
        check=True,
        capture_output=True,
    )
    subprocess.run(["tar", "-xf", str(tar), "-C", str(dest)], check=True, capture_output=True)
    tar.unlink()


@pytest.fixture(scope="session")
def template(tmp_path_factory: pytest.TempPathFactory) -> tuple[Path, Path]:
    base = tmp_path_factory.mktemp("check_upstream")
    pristine = base / "pristine"
    _extract(pristine, f"HEAD:{VENDOR}")
    repo = base / "repo"
    _extract(repo, "HEAD", VENDOR)
    shutil.copy2(REPO / "UPSTREAM.md", repo / "UPSTREAM.md")
    shutil.copy2(REPO / "vendor" / "UPSTREAM_SHA", repo / "vendor" / "UPSTREAM_SHA")
    _git("init", "-q", cwd=repo)
    _git("add", "-f", ".", cwd=repo)
    _git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "vendored", cwd=repo)
    return repo, pristine


@pytest.fixture
def repo(template: tuple[Path, Path], tmp_path: Path) -> Path:
    src, _ = template
    dst = tmp_path / "repo"
    shutil.copytree(src, dst, symlinks=True)
    return dst


@pytest.fixture
def pristine(template: tuple[Path, Path]) -> Path:
    return template[1]


def run(repo: Path, pristine: Path | None = None, *extra: str) -> subprocess.CompletedProcess:
    cmd = [sys.executable, str(SCRIPT), "--repo-root", str(repo)]
    if pristine is not None:
        cmd += ["--pristine-dir", str(pristine)]
    return subprocess.run([*cmd, *extra], capture_output=True, text=True, timeout=120)


def vendored(repo: Path, rel: str) -> Path:
    return repo / VENDOR / rel


def edit(repo: Path, rel: str) -> None:
    with open(vendored(repo, rel), "a") as f:
        f.write("\n# local edit\n")


def list_rows(repo: Path, *rows: tuple[str, str]) -> None:
    md = repo / "UPSTREAM.md"
    text = md.read_text()
    body = "".join(f"| {path} | test edit | {fix} |\n" for path, fix in rows)
    assert TABLE_HEADER in text
    md.write_text(text.replace(TABLE_HEADER, TABLE_HEADER + body))


def fail_lines(out: str) -> list[str]:
    return [line for line in out.splitlines() if line.startswith("FAIL ")]


def test_pristine_tree_passes(repo: Path, pristine: Path):
    r = run(repo, pristine)
    assert r.returncode == 0, r.stdout + r.stderr
    assert r.stdout.splitlines()[-1].startswith("check_upstream: OK")
    assert "0 listed modifications" in r.stdout


def test_unlisted_edit_fails(repo: Path, pristine: Path):
    edit(repo, "python/minisgl/scheduler/scheduler.py")
    r = run(repo, pristine)
    assert r.returncode == 1
    assert fail_lines(r.stdout) == [
        "FAIL UNLISTED_CHANGE python/minisgl/scheduler/scheduler.py: modified, not listed in UPSTREAM.md"
    ]


def test_listed_backend_edit_passes(repo: Path, pristine: Path):
    edit(repo, "python/minisgl/scheduler/scheduler.py")
    list_rows(repo, ("python/minisgl/scheduler/scheduler.py", "no"))
    r = run(repo, pristine)
    assert r.returncode == 0, r.stdout + r.stderr
    assert "1 listed modifications" in r.stdout


def test_tier_a_edit_fails_even_when_listed(repo: Path, pristine: Path):
    edit(repo, "python/minisgl/server/api_server.py")
    list_rows(repo, ("python/minisgl/server/api_server.py", "yes"))
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL FROZEN_TIER_A python/minisgl/server/api_server.py" in r.stdout


def test_tier_a_directory_edit_fails(repo: Path, pristine: Path):
    edit(repo, "python/minisgl/tokenizer/detokenize.py")
    list_rows(repo, ("python/minisgl/tokenizer/detokenize.py", "yes"))
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL FROZEN_TIER_A python/minisgl/tokenizer/detokenize.py" in r.stdout


def test_tier_b_edit_listed_no_fails(repo: Path, pristine: Path):
    edit(repo, "python/minisgl/message/backend.py")
    list_rows(repo, ("python/minisgl/message/backend.py", "no"))
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL TIER_B_REQUIRES_SHARED_FIX python/minisgl/message/backend.py" in r.stdout


def test_tier_b_edit_listed_yes_passes(repo: Path, pristine: Path):
    edit(repo, "python/minisgl/message/backend.py")
    list_rows(repo, ("python/minisgl/message/backend.py", "yes"))
    r = run(repo, pristine)
    assert r.returncode == 0, r.stdout + r.stderr


def test_tier_b_edit_unlisted_fails(repo: Path, pristine: Path):
    edit(repo, "python/minisgl/core.py")
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL UNLISTED_CHANGE python/minisgl/core.py" in r.stdout


def test_tier_c_edit_fails_even_when_listed(repo: Path, pristine: Path):
    edit(repo, "benchmark/online/bench_simple.py")
    list_rows(repo, ("benchmark/online/bench_simple.py", "yes"))
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL FROZEN_TIER_A benchmark/online/bench_simple.py" in r.stdout


def test_untracked_new_file_fails_but_ignored_file_does_not(repo: Path, pristine: Path):
    vendored(repo, "python/minisgl/extra.py").write_text("x = 1\n")
    cache = vendored(repo, "python/minisgl/__pycache__")
    cache.mkdir()
    (cache / "x.pyc").write_bytes(b"\0")
    r = run(repo, pristine)
    assert r.returncode == 1
    assert fail_lines(r.stdout) == [
        "FAIL UNLISTED_CHANGE python/minisgl/extra.py: added, not listed in UPSTREAM.md"
    ]


def test_symlink_replaced_by_same_content_file_fails(repo: Path, pristine: Path):
    link = vendored(repo, ".dockerignore")
    assert link.is_symlink()
    content = link.read_bytes()
    link.unlink()
    link.write_bytes(content)
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL UNLISTED_CHANGE .dockerignore" in r.stdout


def test_executable_bit_change_fails(repo: Path, pristine: Path):
    path = vendored(repo, "README.md")
    os.chmod(path, path.stat().st_mode | 0o100)
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL UNLISTED_CHANGE README.md" in r.stdout


def test_deleted_tracked_file_fails(repo: Path, pristine: Path):
    vendored(repo, "python/minisgl/scheduler/cache.py").unlink()
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL UNLISTED_CHANGE python/minisgl/scheduler/cache.py: removed" in r.stdout


def test_listed_unmodified_path_is_stale(repo: Path, pristine: Path):
    list_rows(repo, ("python/minisgl/scheduler/scheduler.py", "no"))
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL STALE_LISTING python/minisgl/scheduler/scheduler.py" in r.stdout


def test_missing_table_header_is_parse_error(repo: Path, pristine: Path):
    md = repo / "UPSTREAM.md"
    md.write_text(md.read_text().replace(TABLE_HEADER, ""))
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL UPSTREAM_MD_PARSE_ERROR" in r.stdout


def test_bad_shared_fix_value_is_parse_error(repo: Path, pristine: Path):
    edit(repo, "python/minisgl/scheduler/scheduler.py")
    list_rows(repo, ("python/minisgl/scheduler/scheduler.py", "maybe"))
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL UPSTREAM_MD_PARSE_ERROR" in r.stdout


def test_sha_missing_from_upstream_md_fails(repo: Path, pristine: Path):
    md = repo / "UPSTREAM.md"
    sha = (repo / "vendor" / "UPSTREAM_SHA").read_text().strip()
    md.write_text(md.read_text().replace(sha, "0" * 40))
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL UPSTREAM_MD_SHA_MISSING" in r.stdout


def test_license_attribution_removed_fails(repo: Path, pristine: Path):
    lic = vendored(repo, "LICENSE")
    lic.write_text(lic.read_text().replace("Copyright (c) 2026 sgl-project", "Copyright"))
    r = run(repo, pristine)
    assert r.returncode == 1
    assert "FAIL LICENSE_MISSING LICENSE" in r.stdout
    assert "FAIL FROZEN_TIER_A LICENSE" in r.stdout


def test_violations_print_sorted_by_path(repo: Path, pristine: Path):
    edit(repo, "python/minisgl/scheduler/scheduler.py")
    edit(repo, "README.md")
    edit(repo, "python/minisgl/engine/engine.py")
    r = run(repo, pristine)
    assert r.returncode == 1
    paths = [line.split()[2].rstrip(":") for line in fail_lines(r.stdout)]
    assert paths == sorted(paths)
    assert len(paths) == 3
    assert r.stdout.splitlines()[-1] == "check_upstream: FAIL (3 violations)"


def test_offline_passes_on_clean_temp_repo(repo: Path):
    r = run(repo, None, "--offline")
    assert r.returncode == 0, r.stdout + r.stderr
    assert r.stdout.splitlines()[-1].startswith("check_upstream: OK")


def test_offline_fails_on_uncommitted_edit(repo: Path):
    edit(repo, "python/minisgl/scheduler/scheduler.py")
    r = run(repo, None, "--offline")
    assert r.returncode == 1
    assert "python/minisgl/scheduler/scheduler.py" in r.stdout


def test_offline_refuses_listed_modifications(repo: Path):
    list_rows(repo, ("python/minisgl/scheduler/scheduler.py", "no"))
    r = run(repo, None, "--offline")
    assert r.returncode == 1
    assert "without --offline" in r.stdout


def test_offline_on_real_repo_passes():
    r = subprocess.run(
        [sys.executable, str(SCRIPT), "--offline"], capture_output=True, text=True, timeout=120
    )
    assert r.returncode == 0, r.stdout + r.stderr
    assert r.stdout.splitlines()[-1].startswith("check_upstream: OK")


def test_check_never_writes_under_vendor(repo: Path, pristine: Path):
    before = subprocess.run(
        ["git", "status", "--porcelain", "--ignored", "--", VENDOR],
        cwd=repo, capture_output=True, text=True, check=True,
    ).stdout
    assert run(repo, pristine).returncode == 0
    after = subprocess.run(
        ["git", "status", "--porcelain", "--ignored", "--", VENDOR],
        cwd=repo, capture_output=True, text=True, check=True,
    ).stdout
    assert before == after
