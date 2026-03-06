# SPDX-License-Identifier: Apache-2.0
#
# __main__.py - CLI entry point for oz_transpile.

from __future__ import annotations

import argparse
import json
import sys

from .collect import collect
from .emit import emit
from .resolve import resolve


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        prog="oz_transpile",
        description="Transpile Objective-C (Clang JSON AST) to plain C",
    )
    p.add_argument("--input", required=True,
                   help="Path to Clang JSON AST file")
    p.add_argument("--outdir", required=True,
                   help="Output directory for generated C files")
    p.add_argument("--root-class", default="OZObject",
                   help="Name of the root class (default: OZObject)")
    p.add_argument("--pool-sizes", default="",
                   help="Comma-separated ClassName=N pairs (e.g. OZLed=4,OZRgbLed=2)")
    p.add_argument("--verbose", action="store_true",
                   help="Print diagnostic messages")
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


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)

    with open(args.input) as f:
        ast_root = json.load(f)

    module = collect(ast_root)
    resolve(module)

    if args.verbose:
        for d in module.diagnostics:
            print(f"oz_transpile: warning: {d}", file=sys.stderr)

    if args.strict and module.diagnostics:
        for d in module.diagnostics:
            print(f"oz_transpile: error: {d}", file=sys.stderr)
        return 1

    pool_sizes = parse_pool_sizes(args.pool_sizes)
    files = emit(module, args.outdir, pool_sizes=pool_sizes,
                 root_class=args.root_class)

    for f in files:
        print(f"oz_transpile: generated {f}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    sys.exit(main())
