#!/usr/bin/env python3
"""Run one behavior case through BOTH backends and compare what it does.

The 73 cases under tests/behavior/cases/ are the Python pipeline's own
behavior suite: each `<case>.m` has a companion `<case>_test.c` Unity
driver.  `compile_and_run.py` drives the Python backend; this drives the
Rust one (`oz2c`) over the *same* driver and diffs the Unity results, so
"the same features are implemented" rests on observed behavior rather than
on both backends merely compiling.

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

`_write_abi_shim` bridges exactly those three with a generated header, so
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

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
UNITY_DIR = REPO_ROOT / "tests" / "lib" / "unity"
GEN_MAIN = REPO_ROOT / "tests" / "tools" / "gen_test_main.py"
OZ2C = REPO_ROOT / "tools" / "oz_static" / "target" / "debug" / "oz2c"

#: `struct Foo *Foo_oz_alloc(void)` -- how oz_static names the allocator it
#: synthesizes for each class, and the only reliable list of the classes a
#: given run actually emitted.
ALLOC_RE = re.compile(r"\bstruct\s+(\w+)\s*\*\s*(\w+)_oz_alloc\s*\(")

#: `struct X *Class_sel_cls(` -- oz_static's spelling of a class method,
#: which the drivers reach as `Class_cls_sel`.
CLASS_METHOD_RE = re.compile(r"\b(\w+)_(\w+)_cls\s*\(")

#: `OZ_PROTOCOL_SEND_foo` as referenced by a driver.
SEND_RE = re.compile(r"\bOZ_PROTOCOL_SEND_(\w+)\b")

#: An instance-method prototype in oz_static's output:
#: `<ret> Class_sel(struct Class *self`.
INSTANCE_METHOD_RE = re.compile(r"\b(\w+)_(\w+)\s*\(\s*struct\s+(\w+)\s*\*\s*self")

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


def _discover_classes(outdir: Path) -> list[str]:
    """Classes oz_static emitted an allocator for, from its own output."""
    names: set[str] = set()
    for header in outdir.rglob("*.h"):
        for struct_name, alloc_name in ALLOC_RE.findall(header.read_text()):
            if struct_name == alloc_name:
                names.add(struct_name)
    return sorted(names)


def _write_abi_shim(outdir: Path, classes: list[str], root: str,
                    driver_text: str) -> None:
    """Write the `<Class>_ozh.h` / `oz_dispatch.h` headers drivers include.

    Every one of them gets the same body: oz_static's own generated
    headers, then the name bridges.  Writing identical content under each
    expected filename is deliberate -- which header a driver includes says
    nothing about which classes it touches, and oz_static's split is by
    origin file, not by class, so there is no per-class header to map onto.
    """
    body = ["#pragma once", '#include "oz_static_dispatch.h"']
    # Per-origin headers after the companion: a driver may need a complete
    # struct (`struct OZDefer d;` by value), which only the origin header
    # has. The companion alone covers pointer use and prototypes.
    for header in sorted(outdir.rglob("*.h")):
        rel = header.relative_to(outdir)
        if rel.name in ("oz_static_dispatch.h",) or rel.name.endswith("_ozh.h"):
            continue
        body.append(f'#include "{rel.as_posix()}"')
    body.append("")
    body.append("/* Python-backend ABI names, bridged to oz_static's. Naming only:")
    body.append(" * see this file's generator in tests/tools/cross_backend.py. */")
    for cls in classes:
        body.append(f"#define {cls}_alloc {cls}_oz_alloc")
        body.append(f"#define {cls}_free {cls}_oz_free")
        body.append(f"#define OZ_CLASS_{cls} OZ_STATIC_CLASS_{cls}")
    body.append(f"#define {root}_retain oz_static_retain")
    body.append(f"#define {root}_release oz_static_release")
    body.append(f"#define {root}_retainCount oz_static_retain_count")
    body.append(f"#define __objc_refcount_get(o) oz_static_retain_count((struct {root} *)(o))")

    generated = _generated_text(outdir)
    for cls, sel in sorted(set(CLASS_METHOD_RE.findall(generated))):
        if cls in classes:
            body.append(f"#define {cls}_cls_{sel} {cls}_{sel}_cls")

    # A driver may send through `OZ_PROTOCOL_SEND_<sel>` where oz_static
    # emitted no dispatch function, precisely because its hierarchy
    # analysis proved the selector is not polymorphic. If exactly one class
    # implements it, the direct call *is* what a dispatch through it would
    # resolve to, so the macro is defined to that -- with the receiver cast,
    # since the driver passes a root pointer. More than one implementor
    # would mean oz_static should have emitted a dispatcher, so that case is
    # deliberately left undefined and surfaces as a build failure.
    for sel in sorted(set(SEND_RE.findall(driver_text))):
        if f"OZ_PROTOCOL_SEND_{sel}(" in generated:
            continue
        implementors = {c for c, s_, own in INSTANCE_METHOD_RE.findall(generated)
                        if s_ == sel and c == own}
        if len(implementors) == 1:
            only = implementors.pop()
            body.append(
                f"#define OZ_PROTOCOL_SEND_{sel}(o) "
                f"{only}_{sel}((struct {only} *)(o))")
    text = "\n".join(body) + "\n"

    for cls in classes:
        (outdir / f"{cls}_ozh.h").write_text(text)
        # Some drivers spell the include `Foundation/<Class>_ozh.h`, mirroring
        # where the Python backend puts SDK classes.
        foundation = outdir / "Foundation"
        foundation.mkdir(exist_ok=True)
        (foundation / f"{cls}_ozh.h").write_text(text)
    (outdir / "oz_dispatch.h").write_text(text)


def _generated_text(outdir: Path) -> str:
    """All of oz_static's generated headers, concatenated.

    Read once per case; the shim needs to ask several questions of it.
    Shim headers themselves are skipped so a rerun cannot feed on its own
    output.
    """
    parts = []
    for header in sorted(outdir.rglob("*.h")):
        if header.name.endswith("_ozh.h") or header.name == "oz_dispatch.h":
            continue
        parts.append(header.read_text())
    return "\n".join(parts)


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
        transpile = subprocess.run(
            [str(OZ2C),
             "-I", str(REPO_ROOT / "include" / "oz_sdk"),
             "-I", str(REPO_ROOT / "tests" / "behavior" / "include"),
             "--impl-dir", str(REPO_ROOT / "src"),
             str(case), str(tmpdir)],
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
