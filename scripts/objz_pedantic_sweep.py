#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Count ISO C constraint violations in generated C, on target (#266).

`corpus_parity.rs` compiles the behavior corpus with `-std=c17
-pedantic-errors` on host, and that is a gate: the count there is zero.
This asks the same question of the *samples*, with the real ARM
toolchain, and it is a report rather than a gate because a handful of
diagnostics remain -- each listed in `KNOWN_PEDANTIC` below with the
reason it is still there.

Two things make the naive version of this measurement useless, and both
make it read *clean* rather than fail, which is worse:

  1. **`-Wpedantic` cannot go in `EXTRA_CFLAGS`.** That applies to
     Zephyr's own sources too, where it breaks the build outright
     (`subsys/mem_mgmt/mem_attr.c`: `error: zero or negative size
     array`). So each sample is built entirely normally and only the
     files under `oz_static_generated/` are recompiled afterwards, from
     the exact command `compile_commands.json` records.

  2. **CMSIS switches `-Wpedantic` off for the rest of the translation
     unit.** `modules/hal/cmsis_6/CMSIS/Core/Include/core_cm3.h` opens
     with `#pragma GCC diagnostic ignored "-Wpedantic"` -- no `push`, no
     `pop` -- so once anything pulls in `zephyr/kernel.h`, which every
     generated TU does, pedantic diagnostics stop being reported for
     everything that follows. Measured: injecting a bare `;` and an
     empty struct into generated files produced **zero** diagnostics,
     while `-Wall`'s `-Wunused-variable` still fired from the same file.

     So every TU is compiled through a wrapper that pulls Zephyr in
     first, re-enables `-Wpedantic`, and only then includes the real TU.
     That covers generated *headers* too, which inserting the pragma at
     the top of the `.c` would not.

Usage:
    scripts/objz_pedantic_sweep.py                 # every sample, mps2/an385
    scripts/objz_pedantic_sweep.py --board qemu_riscv32
    scripts/objz_pedantic_sweep.py --sample arc_demo --sample heap_alloc
    scripts/objz_pedantic_sweep.py --report        # print, never fail

Exit status is 0 when what is found matches `KNOWN_PEDANTIC` exactly.
Anything new fails, and so does a baseline entry that no longer occurs
-- the same discipline as `corpus_parity.rs`'s `KNOWN_CC_FAILURES`, so
the list cannot decay into a set of silently-tolerated diagnostics.
"""

import argparse
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Samples this sweep does not build, and why. Not a skip of convenience:
# excluded by the board, not by anything in oz_static.
SKIP = {
    "smp_shared": "needs CONFIG_SMP; only qemu_cortex_a53/smp selects it",
}

# The diagnostics that remain, keyed by (sample, path under
# oz_static_generated/, message) and valued (sites, reason). `*` as the
# sample means every sample that reaches that file, with `sites` the
# count expected in each one.
#
# A site is one (file, line, column). Messages are matched with the
# trailing `[-Woption]` tag stripped, so an unrelated edit above a site
# does not churn this table, while the site count is matched exactly.
KNOWN_PEDANTIC = {
    (
        "*",
        "Foundation/OZTimer.c",
        "ISO C forbids conversion of function pointer to object pointer type",
    ): (
        2,
        "oz_static lowers `(__bridge void *)block` to `(void*)(block)`, and "
        "a block is a function pointer. __oz_timer_setup takes `void *` "
        "because ARC forbids a direct block-to-function-pointer cast, so "
        "this is a signature decision rather than a codegen one -- filed "
        "as #267. Corrected by #272: the helper is not on \"both PAL "
        "backends\" -- it is in oz_platform_zephyr.h and in the behaviour "
        "tests' Zephyr stand-in, while the host PAL has no timer at all.",
    ),
    (
        "gpio_demo",
        "main.c",
        'ISO C99 requires at least one argument for the "..." in a variadic macro',
    ): (
        2,
        "GPIO_DT_SPEC_GET(DT_ALIAS(led0), gpios) in the sample's own "
        "passthrough C. The violation is inside Zephyr's macro rather than "
        "in anything oz_static emits, and no spelling of that call avoids "
        "it.",
    ),
    (
        "zbus_service",
        "Foundation/TemperatureService.c",
        'ISO C99 requires at least one argument for the "..." in a variadic macro',
    ): (
        4,
        "ZBUS_CHAN_DEFINE in the sample's own passthrough C -- same as "
        "gpio_demo's, inside Zephyr's macro.",
    ),
    (
        "zbus_service",
        "Foundation/TemperatureService.c",
        "ISO C does not allow extra ';' outside of a function",
    ): (
        2,
        "The `;` after ZBUS_CHAN_DEFINE(...), which is Zephyr's own "
        "documented idiom -- every Zephyr zbus sample writes it. It is "
        "redundant only because these channels have observers: the macro "
        "ends in FOR_EACH_FIXED_ARG_NONEMPTY_TERM, which emits nothing for "
        "ZBUS_OBSERVERS_EMPTY, and then the `;` is required. Removing it "
        "would make the source depend on the observer list staying "
        "non-empty.",
    ),
}

DIAG_RE = re.compile(
    r"^(?P<file>\S*oz_static_generated\S*):(?P<line>\d+):(?P<col>\d+): "
    r"(?:warning|error): (?P<msg>.*)$"
)


def sample_names(named):
    """The samples to sweep: those named, else every buildable one."""
    if named:
        return list(named)
    root = os.path.join(REPO, "samples")
    return sorted(
        d for d in os.listdir(root)
        if os.path.isdir(os.path.join(root, d)) and d not in SKIP
    )


def build(sample, board, build_dir):
    """Build one sample exactly as an ordinary `west build` would."""
    cmd = ["west", "build", "-p", "-b", board,
           os.path.join(REPO, "samples", sample), "-d", build_dir]
    p = subprocess.run(cmd, cwd=REPO, capture_output=True, text=True)
    return p.returncode == 0, p.stdout + p.stderr


def generated_tus(build_dir):
    with open(os.path.join(build_dir, "compile_commands.json")) as f:
        return [e for e in json.load(f) if "oz_static_generated" in e["file"]]


def strip_option(msg):
    return re.sub(r"\s*\[-W[a-z0-9-]+\]$", "", msg).strip()


def recompile(entry, build_dir, wrapdir):
    """Recompile one generated TU with -Wpedantic actually in effect.

    The wrapper is what makes this measure anything: see the CMSIS note
    in this file's docstring. Returns a set of (path, line, col, message)
    sites, paths relative to oz_static_generated/.
    """
    wrap = os.path.join(wrapdir, os.path.basename(entry["file"]))
    with open(wrap, "w") as f:
        f.write("/* generated by scripts/objz_pedantic_sweep.py -- see that file */\n")
        f.write("#include <zephyr/kernel.h>\n")
        f.write('#pragma GCC diagnostic warning "-Wpedantic"\n')
        f.write('#include "%s"\n' % entry["file"])

    argv, out, skip = shlex.split(entry["command"]), [], False
    for arg in argv:
        if skip:
            skip = False
            continue
        if arg == "-o":
            skip = True
            continue
        if arg in ("-c", "-fdiagnostics-color=always"):
            continue
        out.append(wrap if arg == entry["file"] else arg)
    out += ["-Wpedantic", "-fdiagnostics-color=never", "-fsyntax-only"]

    p = subprocess.run(out, cwd=entry.get("directory", build_dir),
                       capture_output=True, text=True)
    sites = set()
    for line in p.stderr.splitlines():
        m = DIAG_RE.match(line)
        if m:
            rel = m.group("file").split("oz_static_generated/", 1)[1]
            sites.add((rel, int(m.group("line")), int(m.group("col")),
                       strip_option(m.group("msg"))))
    return sites


def sweep_sample(sample, board, keep):
    """Build and measure one sample.

    Returns {(path, message): site count}, or None if the build failed.
    Sites are deduplicated first: a diagnostic in a generated *header* is
    reported once per TU that includes it, so 9 reports of one line are
    one site, not nine.
    """
    build_dir = os.path.join(tempfile.gettempdir(), "objz_pedantic_" + sample)
    shutil.rmtree(build_dir, ignore_errors=True)
    ok, log = build(sample, board, build_dir)
    if not ok:
        sys.stderr.write("%s: build failed\n%s\n" % (sample, log[-2000:]))
        if not keep:
            shutil.rmtree(build_dir, ignore_errors=True)
        return None

    wrapdir = os.path.join(build_dir, "_pedantic_wrappers")
    os.makedirs(wrapdir, exist_ok=True)
    sites = set()
    for entry in generated_tus(build_dir):
        sites |= recompile(entry, build_dir, wrapdir)
    if not keep:
        shutil.rmtree(build_dir, ignore_errors=True)

    counts = {}
    for path, _line, _col, msg in sites:
        counts[(path, msg)] = counts.get((path, msg), 0) + 1
    return counts


def baseline_key(sample, path, msg):
    """The KNOWN_PEDANTIC key covering this finding, if any."""
    for key in KNOWN_PEDANTIC:
        known_sample, known_path, known_msg = key
        if known_path == path and known_msg == msg \
                and known_sample in ("*", sample):
            return key
    return None


def main():
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--board", default="mps2/an385")
    ap.add_argument("--sample", action="append", default=[],
                    help="sweep only this sample (repeatable)")
    ap.add_argument("--report", action="store_true",
                    help="print what was found and exit 0, whatever it is")
    ap.add_argument("--keep-builds", action="store_true",
                    help="leave each build directory in place for inspection")
    args = ap.parse_args()

    observed = {}   # baseline key -> {sample: sites}
    unexpected = []
    build_failures = []
    total = 0

    for sample in sample_names(args.sample):
        counts = sweep_sample(sample, args.board, args.keep_builds)
        if counts is None:
            build_failures.append(sample)
            continue
        total += sum(counts.values())
        print("%-22s %d site(s)" % (sample, sum(counts.values())))
        for (path, msg), sites in sorted(counts.items()):
            print("    %-40s %d  %s" % (path, sites, msg))
            key = baseline_key(sample, path, msg)
            if key is None:
                unexpected.append((sample, path, msg, sites))
            else:
                observed.setdefault(key, {})[sample] = sites

    print("\n%d site(s) in total across the generated C" % total)

    problems = []
    for sample in build_failures:
        problems.append("build failed: %s" % sample)
    for sample, path, msg, sites in unexpected:
        problems.append("NEW, not in KNOWN_PEDANTIC: %s %s (%d site(s)) -- %s"
                        % (sample, path, sites, msg))
    for key, (expected, _reason) in sorted(KNOWN_PEDANTIC.items()):
        known_sample, path, msg = key
        per_sample = observed.get(key)
        if not per_sample:
            # Only meaningful when that sample was in this run at all.
            if known_sample == "*" or known_sample in sample_names(args.sample):
                problems.append(
                    "no longer occurs, remove from KNOWN_PEDANTIC: %s %s -- %s"
                    % (known_sample, path, msg))
            continue
        for sample, sites in sorted(per_sample.items()):
            if sites != expected:
                problems.append(
                    "count changed: %s %s -- baseline %d site(s), found %d"
                    % (sample, path, expected, sites))

    for line in problems:
        print(line)
    if args.report:
        return 0
    if problems:
        print("\nfailed: see the lines above")
        return 1
    print("as expected: every site found is in KNOWN_PEDANTIC")
    return 0


if __name__ == "__main__":
    sys.exit(main())
