#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Behavior test orchestrator: transpile .m → compile C → run with Unity."""

from __future__ import annotations

import argparse
import glob
import hashlib
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
PAL_INC = REPO_ROOT / "include"
UNITY_DIR = REPO_ROOT / "tests" / "lib" / "unity"
GEN_MAIN = REPO_ROOT / "tests" / "tools" / "gen_test_main.py"

POOL_RE = re.compile(r"/\*\s*oz-pool:\s*(.+?)\s*\*/")
HEAP_RE = re.compile(r"/\*\s*oz-heap\s*\*/")
#: `Widget_alloc()` / `Widget_oz_alloc()` in a hand-written Unity driver --
#: an allocation oz2c cannot see. See `_default_pool_sizes`.
DRIVER_ALLOC_RE = re.compile(r"\b(\w+?)(?:_oz)?_alloc\s*\(")


sys.path.insert(0, str(Path(__file__).resolve().parent))
import oz_static_build  # noqa: E402  (path set up above)

sys.path.insert(0, str(REPO_ROOT / "scripts"))
import objz_clang  # noqa: E402  (path set up above)


def _find_llvm_clang() -> str:
    """Find LLVM clang for the AST dump.

    Delegated to `scripts/objz_clang.py`, which searches the Zephyr SDK's
    LLVM before Homebrew and the system -- the order
    `cmake/ObjcClang.cmake` has always used and this harness did not, so on
    a machine with the SDK installed the corpus dumped its ASTs with a
    different clang from every CMake build (#269's shape, one layer down).
    """
    return objz_clang.find_clang_or_exit()


def _parse_pool_sizes(m_path: Path) -> str:
    """Extract pool sizes from /* oz-pool: Class=N,... */ comment in .m file."""
    text = m_path.read_text()
    match = POOL_RE.search(text)
    if match:
        return match.group(1).strip()
    return ""


def _merge_pool_sizes(defaults: str, directive: str) -> str:
    """`defaults` filled in where `directive` is silent; the directive wins.

    Both are `Class=N,...` strings. The result goes to `--pool-sizes`,
    which beats the source directive `pools.rs` reads for itself -- so
    passing a class the directive names with the directive's own number is
    a no-op, and passing one it does not name is the point.
    """
    merged: dict[str, str] = {}
    for spec in (defaults, directive):
        for entry in spec.split(","):
            entry = entry.strip()
            if not entry or "=" not in entry:
                continue
            name, _, count = entry.partition("=")
            merged[name.strip()] = count.strip()
    return ",".join(f"{k}={v}" for k, v in sorted(merged.items()))


def _needs_heap_support(m_path: Path) -> bool:
    """Check for /* oz-heap */ marker in .m file."""
    return bool(HEAP_RE.search(m_path.read_text()))



def _default_pool_sizes(m_path: Path, driver_text: str = "") -> str:
    """Pool sizes for classes the *harness* can see allocated and oz2c cannot.

    Two sources, both of them things outside the transpiled `.m`:

      - every class the case declares (`@interface X : ...`), at 4 blocks.
        This is the long-standing default.
      - every `X_alloc()` in the companion `_test.c`. A Unity driver is
        hand-written C that oz2c never reads, so an allocation there
        contributes nothing to the counted size -- and since #419 a class
        with no allocation site in the program text reserves no slab at all,
        where it used to get a floor of one that silently covered this. The
        floor was the wrong place to fix it: it cost a `k_mem_slab` plus an
        instance of static storage in *every* program, for every class it
        never allocates, to serve the handful of drivers that do this. The
        right place is here, where the driver is actually visible --
        `--pool-sizes` exists precisely for a bound the static count cannot
        see, and `tests/behavior/cases/memory/heap_alloc_test.c`'s
        `OZHeap_alloc()` and `foundation/defer_block_ivar_test.c`'s
        `OZDefer_alloc()` are exactly that.

    `X_alloc` rather than `X_oz_alloc` because the driver spells the retired
    Python pipeline's ABI and `oz_static_build.write_abi_shim` bridges it;
    both spellings are matched so neither form is missed.
    """
    text = m_path.read_text()
    classes = set(re.findall(r"@interface\s+(\w+)\s*:", text))
    classes |= set(DRIVER_ALLOC_RE.findall(driver_text))
    if not classes:
        return ""
    return ",".join(f"{c}=4" for c in sorted(classes))


def _find_test_file(m_path: Path) -> Path | None:
    """Find companion _test.c file for a .m file."""
    test_c = m_path.with_name(m_path.stem + "_test.c")
    return test_c if test_c.exists() else None


def run_pipeline(m_path: Path, opt: str = "O0", sanitize: str | None = None,
                 compiler: str = "gcc", cflags: str = "",
                 backend: str = "static",
                 ldflags: str = "",
                 keep_tmp: bool = False,
                 check_leaks: bool = False) -> subprocess.CompletedProcess:
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
        return _run_pipeline_inner(m_path, test_file, tmpdir, opt, sanitize,
                                   compiler, cflags, ldflags,
                                   check_leaks=check_leaks, backend=backend)
    finally:
        if not keep_tmp:
            shutil.rmtree(tmpdir, ignore_errors=True)


def _run_pipeline_inner(m_path: Path, test_file: Path, tmpdir: Path,
                        opt: str, sanitize: str | None,
                        compiler: str = "gcc", cflags: str = "",
                        ldflags: str = "",
                        check_leaks: bool = False,
                        backend: str = "static") -> subprocess.CompletedProcess:
    llvm_clang = _find_llvm_clang()
    ast_json = tmpdir / "input.ast.json"

    # Step 1: Clang AST dump
    # Use -fobjc-runtime=macosx because Clang 18-20 on Linux segfault in
    # MangleContext::mangleObjCMethodName with gnustep-2.0 when JSON-dumping
    # @protocol method declarations.  The AST structure is identical between
    # runtimes for syntax-only parsing; only pointer IDs differ.
    inc_dir = m_path.parent.parent / "include"
    if not inc_dir.is_dir():
        inc_dir = REPO_ROOT / "tests" / "behavior" / "include"

    oz_hdr = REPO_ROOT / "include" / "oz_sdk"
    oz_src = REPO_ROOT / "src"
    stubs_dir = REPO_ROOT / "tests" / "behavior" / "include" / "stubs"
    zephyr_stubs = REPO_ROOT / "tests" / "behavior" / "include" / "zephyr_stubs"

    result = subprocess.run(
        [llvm_clang, "-Xclang", "-ast-dump=json", "-fsyntax-only",
         "-fobjc-runtime=macosx", "-fobjc-arc",
         "--target=x86_64-unknown-linux-gnu",
         "-fblocks",
         "-isystem", str(stubs_dir),
         "-isystem", str(zephyr_stubs),
         "-I", str(inc_dir),
         "-I", str(oz_hdr),
         "-I", str(oz_src),
         str(m_path)],
        capture_output=True, text=True)
    if result.returncode != 0:
        return subprocess.CompletedProcess(
            args=result.args, returncode=1,
            stdout=result.stdout,
            stderr=f"AST dump failed:\n{result.stderr}")

    ast_json.write_text(result.stdout)

    # Step 2: Transpile. This was the one step that differed between the two
    # backends, which is why a single switch here handed the whole harness --
    # compiler, -O level, sanitizers, leak detection, gcov -- to either one.
    # The Python pipeline is retired, so there is one arm; the shape is kept
    # because everything around it is still backend-agnostic and a future
    # second producer would slot in here and nowhere else.
    #
    # oz2c reads the AST dump step 1 already made rather than making its own,
    # so the dump and the transpile cannot disagree about flags.
    # Merged, not "directive or defaults". A case whose directive names one
    # class -- `/* oz-pool: OZObject=1 */` -- meant to size *that* class,
    # not to drop the defaults for every other class in the file. It did,
    # and seven `foundation/*` cases relied on the floor of one that #419
    # removed to cover for it. The directive still wins per class, which is
    # what it is for; it just no longer silences the rest.
    pool_sizes = _merge_pool_sizes(
        _default_pool_sizes(m_path, test_file.read_text() if test_file else ""),
        _parse_pool_sizes(m_path),
    )
    heap_support = _needs_heap_support(m_path)

    if backend != "static":
        return subprocess.CompletedProcess(
            args=["oz2c", str(m_path)], returncode=1, stdout="",
            stderr=f"unknown backend {backend!r}: the Python pipeline was "
                   f"retired (see the `python-backend-final` tag)")
    err = oz_static_build.transpile(m_path, tmpdir, pool_sizes,
                                    heap_support, ast_json)
    if err is not None:
        return subprocess.CompletedProcess(
            args=["oz2c", str(m_path)], returncode=1,
            stdout="", stderr=f"Transpile failed:\n{err}")

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
    c_files = sorted(glob.glob(str(tmpdir / "*.c")) +
                      glob.glob(str(tmpdir / "Foundation" / "*.c")))
    all_sources = c_files + [str(test_file), str(UNITY_DIR / "unity.c")]
    test_bin = tmpdir / "test_bin"

    zephyr_stubs = REPO_ROOT / "tests" / "behavior" / "include" / "zephyr_stubs"
    cc_flags = [compiler, "-std=c11", f"-{opt}",
                "-Wall", "-Werror", "-Wno-unused-function",
                "-DOZ_PLATFORM_HOST",
                "-I", str(tmpdir),
                "-I", str(tmpdir / "Foundation"),
                "-I", str(PAL_INC),
                "-I", str(zephyr_stubs),
                "-I", str(UNITY_DIR)]
    if heap_support:
        cc_flags.append("-DOZ_HEAP_SUPPORT")
    if check_leaks and not sanitize:
        cc_flags.extend(["-fsanitize=leak", "-fno-omit-frame-pointer"])
    if sanitize:
        cc_flags.extend([f"-fsanitize={sanitize}",
                         "-fno-omit-frame-pointer"])
    if cflags:
        cc_flags.extend(shlex.split(cflags))
    cc_flags.extend(all_sources)
    if ldflags:
        cc_flags.extend(shlex.split(ldflags))
    cc_flags.extend(["-o", str(test_bin)])

    result = subprocess.run(cc_flags, capture_output=True, text=True)
    if result.returncode != 0:
        return subprocess.CompletedProcess(
            args=result.args, returncode=1,
            stdout=result.stdout,
            stderr=f"Compilation failed:\n{result.stderr}")

    # Step 5: Run
    env = dict(os.environ)
    if check_leaks and sanitize:
        env["ASAN_OPTIONS"] = "detect_leaks=1"
    elif sanitize:
        env["ASAN_OPTIONS"] = "detect_leaks=0"

    return subprocess.run(
        [str(test_bin)],
        capture_output=True, text=True, timeout=30, env=env)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description="Behavior test: transpile → compile → run")
    p.add_argument("m_file", help="Path to the .m test file")
    p.add_argument("--backend", default="static", choices=["static"],
                   help="transpiler backend (only 'static' / oz2c)")
    p.add_argument("--opt", default="O0", choices=["O0", "O2"],
                   help="Optimization level (default: O0)")
    p.add_argument("--compiler", default="gcc", choices=["gcc", "clang"],
                   help="C compiler for generated code (default: gcc)")
    p.add_argument("--sanitize", default=None,
                   help="Sanitizers to enable (e.g. address,undefined)")
    p.add_argument("--cflags", default="",
                   help="Extra compiler flags (space-separated)")
    p.add_argument("--ldflags", default="",
                   help="Extra linker flags (space-separated)")
    p.add_argument("--check-leaks", action="store_true",
                   help="Enable leak detection (LSan or ASan detect_leaks)")
    p.add_argument("--keep-tmp", action="store_true",
                   help="Keep temporary build directory")
    args = p.parse_args(argv)

    check_leaks = args.check_leaks or os.environ.get("OZ_TEST_CHECK_LEAKS") == "1"
    result = run_pipeline(Path(args.m_file), opt=args.opt,
                          sanitize=args.sanitize, compiler=args.compiler,
                          cflags=args.cflags, ldflags=args.ldflags,
                          keep_tmp=args.keep_tmp,
                          check_leaks=check_leaks, backend=args.backend)
    if result.stdout:
        print(result.stdout, end="")
    if result.stderr:
        print(result.stderr, end="", file=sys.stderr)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
