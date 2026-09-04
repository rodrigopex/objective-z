#!/usr/bin/env python3
"""Run one behavior case through BOTH backends and compare what it does.

The 73 cases under tests/behavior/cases/ are the Python pipeline's own
behavior suite: each `<case>.m` has a companion `<case>_test.c` Unity
driver.  `compile_and_run.py` drives the Python backend; this drives the
Rust one (`oz2c`) over the *same* driver and diffs the Unity results, so
"the same features are implemented" rests on observed behavior rather than
on both backends merely compiling.

Both backends are given the same pool sizes, since a slab that is too
small fails a test on a null receiver rather than on behavior.

What is compared is the Unity report -- one `test_name:PASS|FAIL` line per
test plus the totals -- not generated C.  The two backends deliberately
generate different C (in-place substitution vs. template emission), so a
textual diff would be noise; what has to agree is what the code *does*.

The drivers are written against the Python backend's generated ABI, which
differs from oz_static's in three ways, none of them behavioral:

  * headers are named `<Class>_ozh.h` (plus `oz_dispatch.h`), where
    oz_static emits one header per *origin file*;
  * `+alloc` is reached as `<Class>_alloc`, where oz_static synthesizes
    `<Class>_oz_alloc`;
  * root retain/release/retainCount are `OZObject_*`, where oz_static
    emits backend-wide `oz_static_*` functions;
  * class methods are `<Class>_cls_<sel>`, where oz_static emits
    `<Class>_<sel>_cls`;
  * every dynamically-dispatched selector has an `OZ_PROTOCOL_SEND_<sel>`,
    where oz_static only emits one when a selector really is polymorphic
    (its class hierarchy analysis proves the rest are direct calls).

`_write_abi_shim` bridges exactly those with a generated header, so
the driver is compiled unmodified against both backends.  Anything beyond
naming -- a different result, a crash, a missing symbol -- is a real
difference and is reported as one.

Usage:
    cross_backend.py <case.m>          # one case
    cross_backend.py --all             # the whole corpus, with a summary
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import compile_and_run  # noqa: E402  (path set up above)
import oz_static_build  # noqa: E402  (path set up above)

# The ABI shim and the oz2c invocation live in `oz_static_build` now: the
# pytest harnesses need them too, and they outlive this file, which exists
# only to compare the two backends.
ALLOC_RE = oz_static_build.ALLOC_RE
_discover_classes = oz_static_build.discover_classes
_write_abi_shim = oz_static_build.write_abi_shim
_generated_text = oz_static_build.generated_text

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
UNITY_DIR = REPO_ROOT / "tests" / "lib" / "unity"
GEN_MAIN = REPO_ROOT / "tests" / "tools" / "gen_test_main.py"
OZ2C = REPO_ROOT / "tools" / "oz_static" / "target" / "debug" / "oz2c"

#: A Unity result line: `<path>:<line>:<test name>:PASS` (or `:FAIL:...`).
#: The leading path and line number are build-location noise and are
#: dropped; the test name and outcome are the payload.
UNITY_RESULT_RE = re.compile(r"^.*?:\d+:(\w+):(PASS|FAIL|IGNORE)(.*)$")


def _unity_summary(output: str) -> list[str]:
    """Reduce a Unity run to the lines that describe behavior.

    Keeps `name:PASS` / `name:FAIL:<reason>` and the `N Tests M Failures`
    total, discarding paths, line numbers and the decorative rule -- all of
    which differ between two builds of the same program.
    """
    kept: list[str] = []
    for line in output.splitlines():
        line = line.strip()
        match = UNITY_RESULT_RE.match(line)
        if match:
            name, outcome, detail = match.groups()
            # A FAIL carries its assertion message; keep it, it is the most
            # useful thing in a mismatch report.
            kept.append(f"{name}:{outcome}{detail}" if outcome == "FAIL"
                        else f"{name}:{outcome}")
        elif re.match(r"^\d+ Tests \d+ Failures \d+ Ignored", line):
            kept.append(line)
    return kept


def _compile_failure(stderr: str) -> str:
    """One line describing why a build failed.

    A link failure is reported by the driver as an unhelpful "linker command
    failed", with the symbol names on separate lines above; those names are
    the whole diagnosis, so they are pulled out and preferred over the
    generic message.
    """
    undefined = re.findall(r'"_([A-Za-z_]\w*)", referenced from', stderr)
    if undefined:
        shown = ", ".join(sorted(set(undefined))[:6])
        more = "" if len(set(undefined)) <= 6 else f" (+{len(set(undefined)) - 6} more)"
        return f"undefined: {shown}{more}"
    for line in stderr.splitlines():
        if "error:" in line:
            return line.strip()
    return stderr.strip()[:200]


def _dump_ast(case: Path, outdir: Path) -> Path | None:
    """Run the same clang AST dump the oracle uses, for oz2c to read.

    Reusing `compile_and_run`'s own flags is the point: the two backends must
    be reasoning about the same translation unit, and `-fobjc-arc` is what
    puts the ARC ownership qualifiers into the dump that oz_static needs (see
    tools/oz_static/src/astinfo.rs). Returns None if clang is unavailable or
    the dump fails -- oz_static then falls back to its own narrower rule
    rather than the run failing outright.
    """
    inc_dir = case.parent.parent / "include"
    if not inc_dir.is_dir():
        inc_dir = REPO_ROOT / "tests" / "behavior" / "include"
    ast_path = outdir / "input.ast.json"
    try:
        clang = compile_and_run._find_llvm_clang()
    except SystemExit:
        return None
    result = subprocess.run(
        [clang, "-Xclang", "-ast-dump=json", "-fsyntax-only",
         "-fobjc-runtime=macosx", "-fobjc-arc",
         "--target=x86_64-unknown-linux-gnu", "-fblocks",
         "-isystem", str(REPO_ROOT / "tests" / "behavior" / "include" / "stubs"),
         "-isystem", str(REPO_ROOT / "tests" / "behavior" / "include" / "zephyr_stubs"),
         "-I", str(inc_dir),
         "-I", str(REPO_ROOT / "include" / "oz_sdk"),
         "-I", str(REPO_ROOT / "src"),
         str(case)],
        capture_output=True, text=True)
    if result.returncode != 0 or not result.stdout:
        return None
    ast_path.write_text(result.stdout)
    return ast_path


def run_static(case: Path) -> tuple[bool, str, list[str]]:
    """oz2c -> shim -> Unity driver -> compile -> run.

    Returns (ok, detail, unity_summary).  `detail` explains a failure and is
    empty on success.
    """
    driver = case.with_name(case.stem + "_test.c")
    if not driver.exists():
        return False, f"no companion {driver.name}", []
    if not OZ2C.is_file():
        return False, f"oz2c not built at {OZ2C}", []

    tmpdir = Path(tempfile.mkdtemp(prefix="oz_xback_"))
    try:
        # The two backends must be given the *same* pool sizes or the
        # comparison measures configuration rather than behavior. The Python
        # harness uses the case's `oz-pool` directive if it has one and
        # otherwise defaults to 4 blocks per class; oz_static would
        # otherwise size from allocation sites it can see, and a case whose
        # allocations all live in the `_test.c` driver has none -- its slab
        # would hold one object, the second `alloc` would return NULL, and
        # the test would fail on a null receiver rather than on behavior.
        pool_sizes = (compile_and_run._parse_pool_sizes(case)
                      or compile_and_run._default_pool_sizes(case))
        # `/* oz-heap */` in the case turns on `+allocWithHeap:` -- the same
        # directive and the same pair of switches the Python harness reads
        # (`--heap-support` to the transpiler, `-DOZ_HEAP_SUPPORT` to the
        # compiler), so both backends are given the identical configuration.
        heap_support = compile_and_run._needs_heap_support(case)
        args = [str(OZ2C),
                "-I", str(REPO_ROOT / "include" / "oz_sdk"),
                "-I", str(REPO_ROOT / "tests" / "behavior" / "include"),
                "--impl-dir", str(REPO_ROOT / "src")]
        if heap_support:
            args.append("--heap-support")
        # Clang resolves types and states ARC ownership; tree-sitter does
        # neither. Handing oz2c the same dump the oracle parses is what lets
        # it classify id-typed ivars and spot methods that are declared but
        # never defined.
        ast_dir = Path(tempfile.mkdtemp(prefix="oz_xback_ast_"))
        ast_path = _dump_ast(case, ast_dir)
        if ast_path is not None:
            args += ["--ast", str(ast_path)]
        transpile = subprocess.run(
            args + [str(case), str(tmpdir)], capture_output=True, text=True)
        if transpile.returncode != 0:
            return False, f"transpile: {transpile.stderr.strip().splitlines()[0]}", []

        # Re-run with the sizes, now that the first pass has revealed which
        # classes exist: oz_static rejects `--pool-sizes` naming a class it
        # has no record of, and a directive written for the oracle can name
        # one (`OZSpinLock`, which oz_static never creates).
        known = set(_discover_classes(tmpdir))
        wanted = [entry for entry in pool_sizes.split(",")
                  if entry and entry.split("=")[0] in known]
        if wanted:
            shutil.rmtree(tmpdir, ignore_errors=True)
            tmpdir.mkdir(parents=True, exist_ok=True)
            transpile = subprocess.run(
                args + ["--pool-sizes", ",".join(wanted), str(case), str(tmpdir)],
                capture_output=True, text=True)
            if transpile.returncode != 0:
                return False, f"transpile: {transpile.stderr.strip().splitlines()[0]}", []

        classes = _discover_classes(tmpdir)
        if not classes:
            return False, "no classes found in oz2c output", []
        # The root is whichever class has no `struct <X> base;` field; in
        # this corpus that is always OZObject, and assuming it keeps the
        # shim simple. A program with a different root would need this
        # derived from the output instead.
        root = "OZObject" if "OZObject" in classes else classes[0]
        _write_abi_shim(tmpdir, classes, root, driver.read_text())

        test_main = tmpdir / "test_main.c"
        gen = subprocess.run(
            [sys.executable, str(GEN_MAIN), "--scan", str(driver),
             "--output", str(test_main)],
            capture_output=True, text=True)
        if gen.returncode != 0:
            return False, f"gen_test_main: {gen.stderr.strip()}", []

        sources = sorted(str(p) for p in tmpdir.rglob("*.c"))
        sources += [str(driver), str(UNITY_DIR / "unity.c")]
        binary = tmpdir / "test_bin"
        compile_cmd = [
            "cc", "-std=c11", "-O0", "-DOZ_PLATFORM_HOST",
            *(["-DOZ_HEAP_SUPPORT"] if heap_support else []),
            "-I", str(tmpdir),
            "-I", str(tmpdir / "Foundation"),
            "-I", str(REPO_ROOT / "include"),
            "-I", str(REPO_ROOT / "tests" / "behavior" / "include" / "zephyr_stubs"),
            "-I", str(UNITY_DIR),
            *sources, "-o", str(binary),
        ]
        built = subprocess.run(compile_cmd, capture_output=True, text=True)
        if built.returncode != 0:
            return False, f"compile: {_compile_failure(built.stderr)}", []

        try:
            run = subprocess.run([str(binary)], capture_output=True, text=True,
                                 timeout=30)
        except subprocess.TimeoutExpired:
            return False, "run: timed out", []
        summary = _unity_summary(run.stdout)
        if not summary:
            return False, f"run: no Unity output (exit {run.returncode})", []
        return True, "", summary
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)
        shutil.rmtree(ast_dir, ignore_errors=True) if "ast_dir" in dir() else None


def run_python(case: Path) -> tuple[bool, str, list[str]]:
    """The oracle, via its own existing harness."""
    result = compile_and_run.run_pipeline(case)
    summary = _unity_summary(result.stdout)
    if not summary:
        detail = (result.stderr or result.stdout).strip().splitlines()
        return False, (detail[0] if detail else "no Unity output"), []
    return True, "", summary


def compare(case: Path) -> tuple[str, str]:
    """Run both backends. Returns (verdict, detail).

    Verdicts: MATCH, MISMATCH, STATIC-FAILED, PYTHON-FAILED, BOTH-FAILED.
    """
    py_ok, py_detail, py_summary = run_python(case)
    st_ok, st_detail, st_summary = run_static(case)

    if not py_ok and not st_ok:
        return "BOTH-FAILED", f"python: {py_detail} | static: {st_detail}"
    if not py_ok:
        return "PYTHON-FAILED", py_detail
    if not st_ok:
        return "STATIC-FAILED", st_detail
    if py_summary == st_summary:
        return "MATCH", f"{len(py_summary) - 1} test(s)"
    return "MISMATCH", f"python={py_summary} static={st_summary}"


def corpus_cases() -> list[Path]:
    root = REPO_ROOT / "tests" / "behavior" / "cases"
    return sorted(p for p in root.glob("*/*.m"))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Compare behavior between the Python and Rust backends")
    parser.add_argument("case", nargs="?", help="path to a single <case>.m")
    parser.add_argument("--all", action="store_true",
                        help="run the whole behavior corpus")
    args = parser.parse_args(argv)

    if not args.all and not args.case:
        parser.error("give a case, or --all")

    cases = corpus_cases() if args.all else [Path(args.case).resolve()]
    tally: dict[str, int] = {}
    for case in cases:
        verdict, detail = compare(case)
        tally[verdict] = tally.get(verdict, 0) + 1
        case_id = f"{case.parent.name}/{case.name}"
        if verdict == "MATCH":
            print(f"  {verdict:14} {case_id}  ({detail})")
        else:
            print(f"  {verdict:14} {case_id}\n      {detail}")

    if args.all:
        print()
        print(f"  {len(cases)} cases: " +
              ", ".join(f"{n} {v}" for v, n in sorted(tally.items())))
    # Only a genuine behavioral disagreement is a failure of *this* tool's
    # question; a backend that cannot build a case is reported but is a
    # known-gap question, tracked separately.
    return 1 if tally.get("MISMATCH") else 0


if __name__ == "__main__":
    sys.exit(main())
