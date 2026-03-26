#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Regenerate transpiled C files for Zephyr integration tests.

Transpiles a fixed set of behavior-test .m files into tests/zephyr/generated/.
All sources are AST-dumped individually then transpiled together so the output
contains a unified class hierarchy.  Output is committed to the repo so the
Zephyr build has no Python dependency.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CASES_DIR = REPO_ROOT / "tests" / "behavior" / "cases"
STUBS_DIR = REPO_ROOT / "include" / "oz_sdk"
TEST_INC = REPO_ROOT / "tests" / "behavior" / "include"
OZ_SRC = REPO_ROOT / "src"
ZEPHYR_STUBS = REPO_ROOT / "tests" / "behavior" / "include" / "zephyr_stubs"
OUT_DIR = REPO_ROOT / "tests" / "zephyr" / "generated"

LLVM_SEARCH_PATHS = [
    Path("/opt/homebrew/opt/llvm/bin"),
    Path("/usr/local/opt/llvm/bin"),
    Path("/usr/bin"),
]

SOURCES = [
    "lifecycle/alloc_returns_valid.m",
    "dispatch/super_calls_parent.m",
    "memory/retain_increments.m",
    "protocol/switch_routes_correct.m",
    "edge/deep_inheritance.m",
    "edge/boxed_expression.m",
    "foundation/timer_zephyr.m",
]


def _find_llvm_clang() -> str:
    """Find LLVM clang for AST dump."""
    env_clang = os.environ.get("OZ_CLANG")
    if env_clang and shutil.which(env_clang):
        return env_clang
    versioned = [f"clang-{v}" for v in range(23, 18, -1)]
    for p in LLVM_SEARCH_PATHS:
        for name in versioned + ["clang"]:
            candidate = p / name
            if candidate.exists():
                return str(candidate)
    for name in versioned + ["clang"]:
        if shutil.which(name):
            return name
    print("error: cannot find LLVM clang for AST dump", file=sys.stderr)
    sys.exit(1)


def _collect_pool_sizes(m_paths: list[Path]) -> str:
    """Auto-generate pool sizes (4 blocks per class) from all @interface decls."""
    classes: list[str] = []
    for m_path in m_paths:
        text = m_path.read_text()
        classes.extend(re.findall(r"@interface\s+(\w+)\s*:", text))
    if not classes:
        return ""
    return ",".join(f"{c}=4" for c in classes)


def _ast_dump(clang: str, m_path: Path, out_json: Path) -> None:
    """Run Clang JSON AST dump on a .m file."""
    result = subprocess.run(
        [clang, "-Xclang", "-ast-dump=json", "-fsyntax-only",
         "-fobjc-runtime=macosx", "-fobjc-arc", "-fblocks",
         "-I", str(STUBS_DIR),
         "-I", str(TEST_INC),
         "-I", str(OZ_SRC),
         "-isystem", str(ZEPHYR_STUBS),
         str(m_path)],
        capture_output=True, text=True)
    if result.returncode != 0:
        print(f"error: AST dump failed for {m_path.name}:", file=sys.stderr)
        print(result.stderr, file=sys.stderr)
        sys.exit(1)
    out_json.write_text(result.stdout)


def main() -> int:
    clang = _find_llvm_clang()
    print(f"Using clang: {clang}")

    m_paths: list[Path] = []
    for src_rel in SOURCES:
        m_path = CASES_DIR / src_rel
        if not m_path.exists():
            print(f"error: source not found: {m_path}", file=sys.stderr)
            return 1
        m_paths.append(m_path)

    pool_sizes = _collect_pool_sizes(m_paths)

    with tempfile.TemporaryDirectory(prefix="oz_regen_") as tmpdir:
        tmpdir = Path(tmpdir)

        ast_files: list[Path] = []
        for m_path in m_paths:
            print(f"AST dump: {m_path.name} ...")
            ast_json = tmpdir / f"{m_path.stem}.ast.json"
            _ast_dump(clang, m_path, ast_json)
            ast_files.append(ast_json)

        print("Transpiling all sources together ...")
        cmd = [sys.executable, "-m", "oz_transpile",
               "--input"] + [str(f) for f in ast_files] + [
               "--outdir", str(tmpdir)]
        if pool_sizes:
            cmd.extend(["--pool-sizes", pool_sizes])

        result = subprocess.run(
            cmd, capture_output=True, text=True,
            env={**os.environ, "PYTHONPATH": str(REPO_ROOT / "tools")})
        if result.returncode != 0:
            print("error: transpile failed:", file=sys.stderr)
            print(result.stderr, file=sys.stderr)
            return 1

        generated: dict[str, str] = {}
        for f in sorted(tmpdir.rglob("*")):
            if f.suffix in (".h", ".c"):
                rel = f.relative_to(tmpdir)
                generated[str(rel)] = f.read_text()

    if OUT_DIR.exists():
        for old in OUT_DIR.rglob("*"):
            if old.suffix in (".h", ".c"):
                old.unlink()
    else:
        OUT_DIR.mkdir(parents=True, exist_ok=True)

    for rel in sorted(generated):
        dst = OUT_DIR / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        dst.write_text(generated[rel])
        print(f"  -> {dst.relative_to(REPO_ROOT)}")

    print(f"\nGenerated {len(generated)} files in {OUT_DIR.relative_to(REPO_ROOT)}/")
    return 0


if __name__ == "__main__":
    sys.exit(main())
