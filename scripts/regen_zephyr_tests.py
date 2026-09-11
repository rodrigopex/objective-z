#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Regenerate transpiled C files for Zephyr integration tests.

Transpiles a fixed set of behavior-test .m files into tests/zephyr/generated/.
All sources are AST-dumped individually then transpiled together so the output
contains a unified class hierarchy.  Output is committed to the repo so the
Zephyr build has no Python dependency.
"""

from __future__ import annotations

import re
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "tests" / "tools"))
import oz_static_build  # noqa: E402  (path set up above)

sys.path.insert(0, str(Path(__file__).resolve().parent))
import objz_clang  # noqa: E402  (path set up above)

REPO_ROOT = Path(__file__).resolve().parent.parent
CASES_DIR = REPO_ROOT / "tests" / "behavior" / "cases"
STUBS_DIR = REPO_ROOT / "include" / "oz_sdk"
TEST_INC = REPO_ROOT / "tests" / "behavior" / "include"
OZ_SRC = REPO_ROOT / "src"
ZEPHYR_STUBS = REPO_ROOT / "tests" / "behavior" / "include" / "zephyr_stubs"
OUT_DIR = REPO_ROOT / "tests" / "zephyr" / "generated"


SOURCES = [
    "lifecycle/alloc_returns_valid.m",
    "dispatch/super_calls_parent.m",
    "memory/retain_increments.m",
    "protocol/switch_routes_correct.m",
    "edge/deep_inheritance.m",
    "edge/boxed_expression.m",
]


def _find_llvm_clang() -> str:
    """Find LLVM clang for the AST dump.

    Shared with `tests/tools/compile_and_run.py` and the Rust harness via
    `objz_clang`, so the committed `tests/zephyr/generated/` C is produced
    by the same clang every other path uses -- the SDK's, ahead of Homebrew
    and the system.
    """
    return objz_clang.find_clang_or_exit()


#: `Widget_alloc()` / `Widget_oz_alloc()` in a hand-written ztest driver --
#: an allocation oz2c cannot see. See `_collect_pool_sizes`.
DRIVER_ALLOC_RE = re.compile(r"\b(\w+?)(?:_oz)?_alloc\s*\(")


def _collect_pool_sizes(m_paths: list[Path], driver_text: str) -> str:
    """Pool sizes for classes the *script* can see allocated and oz2c cannot.

    Every class the cases declare (`@interface X : ...`) at 4 blocks, plus
    every `X_alloc()` in the ztest drivers under `tests/zephyr/src/`. The
    drivers are hand-written C that oz2c never reads, so their allocations
    contribute nothing to the counted size -- and since #419 a class with
    no allocation site in the program text reserves no slab at all, where
    it used to get a floor of one that silently covered this.

    Every class the drivers allocate today is also declared by a case, so
    this adds nothing right now. It is here because the *next* driver to
    allocate an SDK class directly would otherwise regenerate C that traps
    at runtime, and `generated-freshness` would pass on it: the same
    mechanism, and the same fix, as `tests/tools/compile_and_run.py`.
    """
    classes: list[str] = []
    for m_path in m_paths:
        text = m_path.read_text()
        classes.extend(re.findall(r"@interface\s+(\w+)\s*:", text))
    classes.extend(DRIVER_ALLOC_RE.findall(driver_text))
    if not classes:
        return ""
    return ",".join(f"{c}=4" for c in sorted(set(classes)))


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

    driver_text = "\n".join(
        p.read_text() for p in sorted((REPO_ROOT / "tests" / "zephyr" / "src").glob("*.c"))
    )
    pool_sizes = _collect_pool_sizes(m_paths, driver_text)

    with tempfile.TemporaryDirectory(prefix="oz_regen_") as tmpdir:
        tmpdir = Path(tmpdir)

        ast_files: list[Path] = []
        for m_path in m_paths:
            print(f"AST dump: {m_path.name} ...")
            ast_json = tmpdir / f"{m_path.stem}.ast.json"
            _ast_dump(clang, m_path, ast_json)
            ast_files.append(ast_json)

        print("Transpiling all sources together (oz2c) ...")
        oz2c = REPO_ROOT / "tools" / "oz_static" / "target" / "debug" / "oz2c"
        if not oz2c.is_file():
            print(f"error: oz2c not built at {oz2c}\n"
                  f"       cargo build --manifest-path tools/oz_static/Cargo.toml",
                  file=sys.stderr)
            return 1
        cmd = [str(oz2c),
               "-I", str(STUBS_DIR),
               "-I", str(TEST_INC),
               "--impl-dir", str(OZ_SRC),
               # Mirrors CONFIG_OBJZ_INTROSPECTION and
               # CONFIG_OBJZ_REFLECTION, both of which default to y, so a
               # ztest source may use those constructs. Passing them
               # changes nothing for the current sources -- neither
               # introspects, and the tables are emitted per construct
               # used, not per option set -- but without them a future one
               # would be refused by a script that has no Kconfig to point
               # anyone at.
               "--introspection",
               "--reflection"]
        for f in ast_files:
            cmd += ["--ast", str(f)]
        if pool_sizes:
            cmd += ["--pool-sizes", pool_sizes]
        cmd += [str(m) for m in m_paths] + [str(tmpdir)]

        result = subprocess.run(cmd, capture_output=True, text=True)
        if result.returncode != 0:
            print("error: transpile failed:", file=sys.stderr)
            print(result.stderr, file=sys.stderr)
            return 1

        # The ztest drivers under tests/zephyr/src/ were written against the
        # Python pipeline's generated ABI -- `<Class>_ozh.h` headers,
        # `Class_alloc`, `OZObject_release`, `OZ_CLASS_X`. oz_static emits one
        # header per *origin file* and its own spellings, so the same shim the
        # behaviour corpus uses bridges the difference and the drivers stay
        # unmodified. See tests/tools/oz_static_build.py.
        print("Writing the ABI shim the ztest drivers include ...")
        classes = oz_static_build.discover_classes(tmpdir)
        if not classes:
            print("error: no classes found in oz2c output", file=sys.stderr)
            return 1
        root = "OZObject" if "OZObject" in classes else classes[0]
        oz_static_build.write_abi_shim(tmpdir, classes, root, driver_text)

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
