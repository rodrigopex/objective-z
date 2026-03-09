#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Behavior test orchestrator: transpile .m → compile C → run with Unity."""

from __future__ import annotations

import argparse
import glob
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
PAL_INC = REPO_ROOT / "include"
UNITY_DIR = REPO_ROOT / "test" / "lib" / "unity"
GEN_MAIN = REPO_ROOT / "test" / "tools" / "gen_test_main.py"

POOL_RE = re.compile(r"/\*\s*oz-pool:\s*(.+?)\s*\*/")

LLVM_SEARCH_PATHS = [
    Path("/opt/homebrew/opt/llvm/bin"),
    Path("/usr/local/opt/llvm/bin"),
]


def _find_llvm_clang() -> str:
    """Find Homebrew LLVM clang for AST dump (gnustep-2.0 needs ELF target)."""
    for p in LLVM_SEARCH_PATHS:
        candidate = p / "clang"
        if candidate.exists():
            return str(candidate)
    # Fallback: try PATH
    if shutil.which("clang"):
        return "clang"
    print("error: cannot find LLVM clang for AST dump", file=sys.stderr)
    sys.exit(1)


def _parse_pool_sizes(m_path: Path) -> str:
    """Extract pool sizes from /* oz-pool: Class=N,... */ comment in .m file."""
    text = m_path.read_text()
    match = POOL_RE.search(text)
    if match:
        return match.group(1).strip()
    return ""


def _default_pool_sizes(m_path: Path) -> str:
    """Auto-generate default pool sizes (4 blocks per class) from @interface decls."""
    text = m_path.read_text()
    classes = re.findall(r"@interface\s+(\w+)\s*:", text)
    if not classes:
        return ""
    return ",".join(f"{c}=4" for c in classes)


def _find_test_file(m_path: Path) -> Path | None:
    """Find companion _test.c file for a .m file."""
    test_c = m_path.with_name(m_path.stem + "_test.c")
    return test_c if test_c.exists() else None


def run_pipeline(m_path: Path, opt: str = "O0", sanitize: bool = False,
                 keep_tmp: bool = False) -> subprocess.CompletedProcess:
    """Run the full transpile → compile → execute pipeline."""
    m_path = m_path.resolve()
    test_file = _find_test_file(m_path)
    if test_file is None:
        return subprocess.CompletedProcess(
            args=[], returncode=1,
            stdout="", stderr=f"error: no companion _test.c for {m_path.name}\n")

    h = hashlib.md5(str(m_path).encode()).hexdigest()[:8]
    tmpdir = Path(tempfile.mkdtemp(prefix=f"oz_btest_{h}_"))

    try:
        return _run_pipeline_inner(m_path, test_file, tmpdir, opt, sanitize)
    finally:
        if not keep_tmp:
            shutil.rmtree(tmpdir, ignore_errors=True)


def _run_pipeline_inner(m_path: Path, test_file: Path, tmpdir: Path,
                        opt: str, sanitize: bool) -> subprocess.CompletedProcess:
    llvm_clang = _find_llvm_clang()
    ast_json = tmpdir / "input.ast.json"

    # Step 1: Clang AST dump
    inc_dir = m_path.parent.parent / "include"
    if not inc_dir.is_dir():
        inc_dir = REPO_ROOT / "test" / "behavior" / "include"

    result = subprocess.run(
        [llvm_clang, "-Xclang", "-ast-dump=json", "-fsyntax-only",
         "-fobjc-runtime=gnustep-2.0", "--target=arm-none-eabi",
         "-I", str(inc_dir),
         str(m_path)],
        capture_output=True, text=True)
    if result.returncode != 0:
        return subprocess.CompletedProcess(
            args=result.args, returncode=1,
            stdout=result.stdout,
            stderr=f"AST dump failed:\n{result.stderr}")

    ast_json.write_text(result.stdout)

    # Step 2: Transpile
    pool_sizes = _parse_pool_sizes(m_path) or _default_pool_sizes(m_path)
    transpile_cmd = [
        sys.executable, "-m", "oz_transpile",
        "--input", str(ast_json),
        "--outdir", str(tmpdir)]
    if pool_sizes:
        transpile_cmd.extend(["--pool-sizes", pool_sizes])

    result = subprocess.run(
        transpile_cmd,
        capture_output=True, text=True,
        env={**os.environ, "PYTHONPATH": str(REPO_ROOT / "tools")})
    if result.returncode != 0:
        return subprocess.CompletedProcess(
            args=result.args, returncode=1,
            stdout=result.stdout,
            stderr=f"Transpile failed:\n{result.stderr}")

    # Step 3: Generate test_main.c
    test_main = tmpdir / "test_main.c"
    result = subprocess.run(
        [sys.executable, str(GEN_MAIN),
         "--scan", str(test_file),
         "--output", str(test_main)],
        capture_output=True, text=True)
    if result.returncode != 0:
        return subprocess.CompletedProcess(
            args=result.args, returncode=1,
            stdout=result.stdout,
            stderr=f"gen_test_main failed:\n{result.stderr}")

    # Step 4: Compile
    c_files = sorted(glob.glob(str(tmpdir / "*.c")))
    all_sources = c_files + [str(test_file), str(UNITY_DIR / "unity.c")]
    test_bin = tmpdir / "test_bin"

    cc_flags = ["clang", "-std=c11", f"-{opt}",
                "-Wall", "-Werror", "-Wno-unused-function",
                "-DOZ_PLATFORM_HOST",
                "-I", str(tmpdir),
                "-I", str(PAL_INC),
                "-I", str(UNITY_DIR)]
    if sanitize:
        cc_flags.extend(["-fsanitize=address,undefined",
                         "-fno-omit-frame-pointer"])
    cc_flags.extend(all_sources)
    cc_flags.extend(["-o", str(test_bin)])

    result = subprocess.run(cc_flags, capture_output=True, text=True)
    if result.returncode != 0:
        return subprocess.CompletedProcess(
            args=result.args, returncode=1,
            stdout=result.stdout,
            stderr=f"Compilation failed:\n{result.stderr}")

    # Step 5: Run
    env = dict(os.environ)
    if sanitize:
        env["ASAN_OPTIONS"] = "detect_leaks=0"

    return subprocess.run(
        [str(test_bin)],
        capture_output=True, text=True, timeout=30, env=env)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description="Behavior test: transpile → compile → run")
    p.add_argument("m_file", help="Path to the .m test file")
    p.add_argument("--opt", default="O0", choices=["O0", "O2"],
                   help="Optimization level (default: O0)")
    p.add_argument("--sanitize", action="store_true",
                   help="Enable ASan + UBSan")
    p.add_argument("--keep-tmp", action="store_true",
                   help="Keep temporary build directory")
    args = p.parse_args(argv)

    result = run_pipeline(Path(args.m_file), opt=args.opt,
                          sanitize=args.sanitize, keep_tmp=args.keep_tmp)
    if result.stdout:
        print(result.stdout, end="")
    if result.stderr:
        print(result.stderr, end="", file=sys.stderr)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
