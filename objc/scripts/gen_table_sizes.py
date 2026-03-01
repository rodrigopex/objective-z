#!/usr/bin/env python3
#
# SPDX-License-Identifier: Apache-2.0
#
# gen_table_sizes.py - Generate runtime table_sizes.h from tree-sitter
#                      analysis of Objective-C source files.
#
# Parses .m files directly (no Clang AST dumps needed) to count:
#   - @implementation declarations  → class table size
#   - @implementation Foo(Cat)      → category table size
#   - @protocol declarations        → protocol table size
#   - method definitions            → hash table + dispatch cache sizes
#
# Usage:
#   python3 gen_table_sizes.py --output=table_sizes.h file1.m file2.m ...

import argparse
import math
import sys
from collections import defaultdict

import tree_sitter_objc as tsobjc
from tree_sitter import Language, Parser, Query, QueryCursor

OBJC_LANGUAGE = Language(tsobjc.language())

Q_IMPL = Query(OBJC_LANGUAGE, "(class_implementation) @impl")
Q_PROTO = Query(OBJC_LANGUAGE, "(protocol_declaration) @proto")
Q_METHOD = Query(OBJC_LANGUAGE, "(method_definition) @m")


def parse_args():
    p = argparse.ArgumentParser(
        description="Generate table_sizes.h from ObjC sources (tree-sitter)")
    p.add_argument("--pointer-size", type=int, default=4, choices=[4, 8])
    p.add_argument("--output", required=True,
                   help="Output path for generated table_sizes.h")
    p.add_argument("--n-pools", type=int, default=0,
                   help="Number of static pools (from gen_pools.py)")
    p.add_argument("sources", nargs="+",
                   help=".m source files to analyze")
    return p.parse_args()


def count_metadata(source_paths):
    """Parse .m files with tree-sitter and count ObjC metadata.

    Returns dict with n_classes, n_categories, n_protocols,
    n_methods, max_methods_per_class.
    """
    parser = Parser(OBJC_LANGUAGE)

    impl_names = set()
    n_categories = 0
    protocol_names = set()
    n_methods = 0
    methods_per_class = defaultdict(int)

    for path in source_paths:
        with open(path, "rb") as f:
            code = f.read()

        tree = parser.parse(code)
        root = tree.root_node

        # Count class/category implementations
        impls = QueryCursor(Q_IMPL).captures(root).get("impl", [])
        for impl_node in impls:
            # First identifier child = class name
            class_name = None
            for child in impl_node.children:
                if child.type == "identifier":
                    class_name = child.text.decode()
                    break

            is_cat = impl_node.child_by_field_name("category") is not None
            if is_cat:
                n_categories += 1
            else:
                impl_names.add(class_name)

            # Count methods inside this implementation
            methods = QueryCursor(Q_METHOD).captures(impl_node).get("m", [])
            n_methods += len(methods)
            if not is_cat:
                methods_per_class[class_name] += len(methods)

        # Count protocol declarations
        protos = QueryCursor(Q_PROTO).captures(root).get("proto", [])
        for proto_node in protos:
            for child in proto_node.children:
                if child.type == "identifier":
                    protocol_names.add(child.text.decode())
                    break

    max_mpc = max(methods_per_class.values()) if methods_per_class else 4

    return {
        "n_classes": len(impl_names),
        "n_categories": n_categories,
        "n_protocols": len(protocol_names),
        "n_methods": n_methods,
        "max_methods_per_class": max_mpc,
    }


def _next_power_of_2(n):
    """Smallest power of 2 >= n."""
    if n <= 0:
        return 1
    return 1 << (n - 1).bit_length()


def compute_table_sizes(counts, n_pools):
    """Compute optimal table sizes from metadata counts."""
    nc = counts["n_classes"]
    ncat = counts["n_categories"]
    nprot = counts["n_protocols"]
    nm = counts["n_methods"]
    max_mpc = counts["max_methods_per_class"]

    # Each class + metaclass = 2 entries, +4 margin
    class_ts = 2 * nc + 4 if nc > 0 else 8

    category_ts = max(ncat + 2, 4)
    protocol_ts = max(nprot + 2, 4)

    # Hash table: open addressing, target load factor ~0.6
    raw_hash = int(math.ceil(nm / 0.6)) if nm > 0 else 64
    hash_ts = max(raw_hash, 64)

    # Dispatch cache entries per class: power-of-2, clamp [8, 32]
    dispatch_ts = _next_power_of_2(max_mpc)
    dispatch_ts = max(8, min(dispatch_ts, 32))

    # Static dispatch table blocks: cover all classes + metaclasses
    dispatch_static = 2 * nc + 2 if nc > 0 else 8

    # Pool table: n_pools + margin
    pool_ts = max(n_pools + 2, 4)

    return {
        "CLASS_TABLE_SIZE": class_ts,
        "CATEGORY_TABLE_SIZE": category_ts,
        "PROTOCOL_TABLE_SIZE": protocol_ts,
        "HASH_TABLE_SIZE": hash_ts,
        "DISPATCH_TABLE_SIZE": dispatch_ts,
        "DISPATCH_CACHE_STATIC_COUNT": dispatch_static,
        "STATIC_POOL_TABLE_SIZE": pool_ts,
    }


def generate_header(sizes, output_path):
    """Write generated table_sizes.h with CONFIG_OBJZ_* overrides."""
    with open(output_path, "w") as f:
        f.write("/* Auto-generated by gen_table_sizes.py — do not edit */\n")
        f.write("#ifndef OBJZ_TABLE_SIZES_H\n")
        f.write("#define OBJZ_TABLE_SIZES_H\n\n")
        for name, value in sorted(sizes.items()):
            macro = f"CONFIG_OBJZ_{name}"
            f.write(f"#if !defined({macro}) || {macro} == 0\n")
            f.write(f"#undef {macro}\n")
            f.write(f"#define {macro} {value}\n")
            f.write(f"#endif\n\n")
        f.write("#endif /* OBJZ_TABLE_SIZES_H */\n")


def main():
    args = parse_args()

    counts = count_metadata(args.sources)
    sizes = compute_table_sizes(counts, args.n_pools)
    generate_header(sizes, args.output)

    print("gen_table_sizes.py: "
          + " ".join(f"{k}={v}" for k, v in sorted(sizes.items())),
          file=sys.stderr)


if __name__ == "__main__":
    main()
