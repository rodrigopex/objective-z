# SPDX-License-Identifier: Apache-2.0
#
# __main__.py - CLI entry point for oz_transpile.

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

from .collect import collect, extract_source_generics, is_stub_source, merge_modules
from .emit import emit
from .model import OrphanSource
from .resolve import resolve


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        prog="oz_transpile",
        description="Transpile Objective-C (Clang JSON AST) to plain C",
    )
    p.add_argument("--input", required=True, nargs="+",
                   help="Path(s) to Clang JSON AST file(s)")
    p.add_argument("--sources", nargs="*", default=None,
                   help="Original .m source paths (same order as --input)")
    p.add_argument("--manifest", default="",
                   help="Write list of generated file paths to this file")
    p.add_argument("--outdir", required=True,
                   help="Output directory for generated C files")
    p.add_argument("--root-class", default="OZObject",
                   help="Name of the root class (default: OZObject)")
    p.add_argument("--pool-sizes", default="",
                   help="Comma-separated ClassName=N pairs (e.g. OZLed=4,OZRgbLed=2)")
    p.add_argument("--item-pool-size", type=int, default=None,
                   help="Override auto-computed sys_mem_blocks pool size for "
                        "array/dict item slots")
    p.add_argument("--verbose", action="store_true",
                   help="Print diagnostic messages")
    p.add_argument("--heap-support", action="store_true",
                   help="Enable allocWithHeap: and heap-aware free")
    p.add_argument("--strict", action="store_true",
                   help="Treat diagnostics as errors")
    return p.parse_args(argv)


def parse_pool_sizes(raw: str) -> dict[str, int]:
    if not raw:
        return {}
    result = {}
    for pair in raw.split(","):
        pair = pair.strip()
        if "=" in pair:
            name, count = pair.split("=", 1)
            result[name.strip()] = int(count.strip())
    return result


def _source_stem(path: str) -> str:
    """Extract the stem from a source path: '/a/b/Producer.m' -> 'Producer'."""
    base = os.path.basename(path)
    stem, _ = os.path.splitext(base)
    return stem


def _associate_module_items_with_class(module):
    """Move module-level functions/statics/verbatim/includes into the primary class.

    Each .m file typically defines one class alongside free functions.
    This associates those items with the class for per-file emission.
    If no class exists, creates an OrphanSource for standalone emission.
    """
    has_items = (module.functions or module.statics
                 or module.verbatim_lines or module.user_includes)

    if not module.classes:
        if has_items and module.source_stem:
            module.orphan_sources.append(OrphanSource(
                stem=module.source_stem,
                functions=list(module.functions),
                statics=list(module.statics),
                verbatim_lines=list(module.verbatim_lines),
                user_includes=list(module.user_includes),
                source_path=module.source_paths.get(module.source_stem),
            ))
            module.functions = []
            module.statics = []
            module.verbatim_lines = []
            module.user_includes = []
        return

    if not has_items:
        return

    primary = None
    for cls in module.classes.values():
        if any(m.body_ast for m in cls.methods):
            primary = cls
            break

    # No class with implementation — treat as orphan source
    if primary is None:
        if has_items and module.source_stem:
            module.orphan_sources.append(OrphanSource(
                stem=module.source_stem,
                functions=list(module.functions),
                statics=list(module.statics),
                verbatim_lines=list(module.verbatim_lines),
                user_includes=list(module.user_includes),
                source_path=module.source_paths.get(module.source_stem),
            ))
            module.functions = []
            module.statics = []
            module.verbatim_lines = []
            module.user_includes = []
        return

    primary.functions.extend(module.functions)
    primary.statics.extend(module.statics)
    for line in module.verbatim_lines:
        if line not in primary.verbatim_lines:
            primary.verbatim_lines.append(line)
    for inc in module.user_includes:
        if inc not in primary.user_includes:
            primary.user_includes.append(inc)
    module.functions = []
    module.statics = []
    module.verbatim_lines = []
    module.user_includes = []


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)

    sources = args.sources or []

    modules = []
    for i, path in enumerate(args.input):
        with open(path) as f:
            ast_root = json.load(f)
        m = collect(ast_root)
        if i < len(sources):
            src_path = sources[i]
            m.source_stem = _source_stem(src_path)
            stub = is_stub_source(src_path)
            if not stub:
                src_file = Path(src_path)
                if src_file.is_file():
                    m.source_path = src_file
                if m.source_path:
                    m.source_paths[m.source_stem] = m.source_path
            else:
                # Even for stubs, track source path for generic extraction
                src_file = Path(src_path)
                if src_file.is_file():
                    m.source_paths[m.source_stem] = src_file
            for cls in m.classes.values():
                if stub:
                    cls.is_foundation = True
                has_impl = any(meth.body_ast for meth in cls.methods)
                if has_impl and m.source_stem != cls.name:
                    cls.source_stem = m.source_stem
        modules.append(m)

    for m in modules:
        _associate_module_items_with_class(m)

    module = merge_modules(modules) if len(modules) > 1 else modules[0]

    # Extract generic type annotations from source (Clang strips them from AST)
    for src_path in module.source_paths.values():
        module.generic_types.update(extract_source_generics(src_path))

    try:
        resolve(module)
    except ValueError as e:
        print(f"oz_transpile: error: {e}", file=sys.stderr)
        return 1

    if module.errors:
        for e in module.errors:
            print(f"oz_transpile: error: {e}", file=sys.stderr)
        return 1

    if args.verbose:
        for d in module.diagnostics:
            print(f"oz_transpile: warning: {d}", file=sys.stderr)

    if args.strict and module.diagnostics:
        for d in module.diagnostics:
            print(f"oz_transpile: error: {d}", file=sys.stderr)
        return 1

    pool_sizes = parse_pool_sizes(args.pool_sizes)
    pre_emit_diag_count = len(module.diagnostics)
    files = emit(module, args.outdir, pool_sizes=pool_sizes,
                 root_class=args.root_class,
                 item_pool_size=args.item_pool_size,
                 heap_support=args.heap_support)

    # Check for errors added during emit (e.g., unsupported boxed expr, capturing block)
    if module.errors:
        for e in module.errors:
            print(f"oz_transpile: error: {e}", file=sys.stderr)
        return 1

    # Surface diagnostics added during emit (e.g., class-not-found in context.py)
    new_diags = module.diagnostics[pre_emit_diag_count:]
    if new_diags:
        for d in new_diags:
            print(f"oz_transpile: warning: {d}", file=sys.stderr)
        if args.strict:
            return 1

    total = len(files)
    is_tty = sys.stderr.isatty()
    for i, f in enumerate(files, 1):
        name = os.path.basename(f)
        if is_tty:
            sys.stderr.write(f"\r\033[Koz_transpile: [{i}/{total}] {name}")
            sys.stderr.flush()
        else:
            if args.verbose:
                print(f"oz_transpile: [{i}/{total}] {name}", file=sys.stderr)
    summary = f"oz_transpile: {total} files generated in {args.outdir}"
    if is_tty:
        sys.stderr.write(f"\r\033[K{summary}\n")
        sys.stderr.flush()
    else:
        print(summary, file=sys.stderr)

    if args.manifest:
        with open(args.manifest, "w") as mf:
            for f in files:
                mf.write(f + "\n")

    return 0


if __name__ == "__main__":
    sys.exit(main())
