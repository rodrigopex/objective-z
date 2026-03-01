#!/usr/bin/env python3
#
# SPDX-License-Identifier: Apache-2.0
#
# objz_gen_table_sizes.py - Generate runtime table_sizes.h and dtable_pool.c
#                           from tree-sitter analysis of Objective-C sources.
#
# Parses .m files directly (no Clang AST dumps needed) to count:
#   - @implementation declarations  → class table size
#   - @implementation Foo(Cat)      → category table size
#   - @protocol declarations        → protocol table size
#   - method definitions            → hash table + dispatch cache sizes
#   - instance vs class methods     → per-class dtable sizing
#
# Usage:
#   python3 objz_gen_table_sizes.py --output=table_sizes.h file1.m file2.m ...
#   python3 objz_gen_table_sizes.py --output=table_sizes.h \
#       --dtable-output=dtable_pool.c file1.m file2.m ...

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
    p.add_argument("--dtable-output", default=None,
                   help="Output path for generated dtable_pool.c")
    p.add_argument("--n-pools", type=int, default=0,
                   help="Number of static pools (from objz_gen_pools.py)")
    p.add_argument("sources", nargs="+",
                   help=".m source files to analyze")
    return p.parse_args()


def _is_class_method(method_node):
    """Check if a method_definition is a class method (+) vs instance (-)."""
    text = method_node.text.decode()
    stripped = text.lstrip()
    return stripped.startswith("+")


def count_metadata(source_paths):
    """Parse .m files with tree-sitter and count ObjC metadata.

    Returns dict with n_classes, n_categories, n_protocols,
    n_methods, max_methods_per_class, per_class_methods.
    """
    parser = Parser(OBJC_LANGUAGE)

    impl_names = set()
    n_categories = 0
    protocol_names = set()
    n_methods = 0
    methods_per_class = defaultdict(int)
    instance_methods_per_class = defaultdict(int)
    class_methods_per_class = defaultdict(int)

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
                for m in methods:
                    if _is_class_method(m):
                        class_methods_per_class[class_name] += 1
                    else:
                        instance_methods_per_class[class_name] += 1

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
        "impl_names": impl_names,
        "instance_methods_per_class": dict(instance_methods_per_class),
        "class_methods_per_class": dict(class_methods_per_class),
    }


def _next_power_of_2(n):
    """Smallest power of 2 >= n."""
    if n <= 0:
        return 1
    return 1 << (n - 1).bit_length()


def compute_table_sizes(counts, n_pools):
    """Compute optimal table sizes from metadata counts.

    Returns dict mapping name -> (value, formula_str).
    """
    nc = counts["n_classes"]
    ncat = counts["n_categories"]
    nprot = counts["n_protocols"]
    nm = counts["n_methods"]
    max_mpc = counts["max_methods_per_class"]

    # Each class + metaclass = 2 entries, +4 margin
    class_ts = 2 * nc + 4 if nc > 0 else 8
    class_f = f"2*{nc} + 4" if nc > 0 else "default(8)"

    category_ts = max(ncat + 2, 4)
    category_f = f"max({ncat} + 2, 4)"

    protocol_ts = max(nprot + 2, 4)
    protocol_f = f"max({nprot} + 2, 4)"

    # Hash table: open addressing, target load factor ~0.6
    raw_hash = int(math.ceil(nm / 0.6)) if nm > 0 else 64
    hash_ts = max(raw_hash, 64)
    hash_f = f"max(ceil({nm}/0.6), 64)" if nm > 0 else "default(64)"

    # Dispatch table max cap: power-of-2, clamp [8, 32]
    dispatch_ts = _next_power_of_2(max_mpc)
    dispatch_ts = max(8, min(dispatch_ts, 32))
    dispatch_f = f"clamp(p2({max_mpc}), 8, 32)"

    # Dispatch cache registry: cover all classes + margin
    registry_ts = max(nc + 2, 4)
    registry_f = f"max({nc} + 2, 4)"

    # Pool table: n_pools + margin
    pool_ts = max(n_pools + 2, 4)
    pool_f = f"max({n_pools} + 2, 4)"

    return {
        "CLASS_TABLE_SIZE": (class_ts, class_f),
        "CATEGORY_TABLE_SIZE": (category_ts, category_f),
        "PROTOCOL_TABLE_SIZE": (protocol_ts, protocol_f),
        "HASH_TABLE_SIZE": (hash_ts, hash_f),
        "DISPATCH_TABLE_SIZE": (dispatch_ts, dispatch_f),
        "DISPATCH_CACHE_REGISTRY_SIZE": (registry_ts, registry_f),
        "STATIC_POOL_TABLE_SIZE": (pool_ts, pool_f),
    }


def generate_header(sizes, output_path):
    """Write generated table_sizes.h with CONFIG_OBJZ_* overrides."""
    with open(output_path, "w") as f:
        f.write("/* Auto-generated by objz_gen_table_sizes.py — do not edit */\n")
        f.write("#ifndef OBJZ_TABLE_SIZES_H\n")
        f.write("#define OBJZ_TABLE_SIZES_H\n\n")
        for name, (value, _formula) in sorted(sizes.items()):
            macro = f"CONFIG_OBJZ_{name}"
            f.write(f"#if !defined({macro}) || {macro} == 0\n")
            f.write(f"#undef {macro}\n")
            f.write(f"#define {macro} {value}\n")
            f.write(f"#endif\n\n")
        f.write("#endif /* OBJZ_TABLE_SIZES_H */\n")


def compute_per_class_dtable_sizes(counts, max_cap):
    """Compute per-class dtable sizes for instance and class methods.

    Returns list of (class_name, cls_size, meta_size) tuples.
    """
    instance_mpc = counts["instance_methods_per_class"]
    class_mpc = counts["class_methods_per_class"]
    all_classes = counts["impl_names"]

    result = []
    for name in sorted(all_classes):
        n_inst = instance_mpc.get(name, 0)
        n_cls = class_mpc.get(name, 0)

        cls_size = _next_power_of_2(n_inst) if n_inst > 0 else 8
        cls_size = max(8, min(cls_size, max_cap))

        meta_size = _next_power_of_2(n_cls) if n_cls > 0 else 8
        meta_size = max(8, min(meta_size, max_cap))

        result.append((name, cls_size, meta_size))
    return result


def generate_dtable_pool(counts, max_cap, output_path):
    """Write generated dtable_pool.c with OZ_DEFINE_DTABLE calls."""
    entries = compute_per_class_dtable_sizes(counts, max_cap)

    with open(output_path, "w") as f:
        f.write("/* Auto-generated by objz_gen_table_sizes.py — do not edit */\n")
        f.write("#include <objc/dtable.h>\n\n")
        for name, cls_size, meta_size in entries:
            f.write(f"OZ_DEFINE_DTABLE({name}, {cls_size}, {meta_size});\n")

    # Print per-class sizing table to stderr
    p = lambda *a, **kw: print(*a, file=sys.stderr, **kw)
    p(f"objz_gen_table_sizes: per-class dtable sizing ({len(entries)} classes)")

    if entries:
        name_w = max(len(e[0]) for e in entries)
        p(f"  {'CLASS':<{name_w}}  {'INST':>4}  {'META':>4}  INST_BSS  META_BSS")
        p(f"  {'-' * (name_w + 30)}")
        total_bss = 0
        for name, cls_size, meta_size in entries:
            inst_bss = cls_size * 8  # sizeof(objc_dtable_entry) on ARM32
            meta_bss = meta_size * 8
            total_bss += inst_bss + meta_bss
            p(f"  {name:<{name_w}}  {cls_size:>4}  {meta_size:>4}  "
              f"{inst_bss:>6} B  {meta_bss:>6} B")
        p(f"  {'TOTAL':<{name_w}}  {'':>4}  {'':>4}  {total_bss:>6} B (+ mask overhead)")


def _print_table(counts, sizes):
    """Print aligned table of computed sizes with formulas to stderr."""
    nc = counts["n_classes"]
    ncat = counts["n_categories"]
    nprot = counts["n_protocols"]
    nm = counts["n_methods"]
    max_mpc = counts["max_methods_per_class"]

    p = lambda *a, **kw: print(*a, file=sys.stderr, **kw)

    p(f"objz_gen_table_sizes: {nc} classes, {ncat} categories, "
      f"{nprot} protocols, {nm} methods (max {max_mpc}/class)")

    name_w = max(len(n) for n in sizes)
    val_w = max(len(str(v)) for v, _f in sizes.values())

    p(f"  {'TABLE':<{name_w}}  {'SIZE':>{val_w}}  FORMULA")
    p(f"  {'-' * (name_w + val_w + 10)}")
    for name, (value, formula) in sorted(sizes.items()):
        p(f"  {name:<{name_w}}  {value:>{val_w}}  {formula}")


def main():
    args = parse_args()

    counts = count_metadata(args.sources)
    sizes = compute_table_sizes(counts, args.n_pools)
    generate_header(sizes, args.output)
    _print_table(counts, sizes)

    if args.dtable_output:
        # Use DISPATCH_TABLE_SIZE as max cap
        max_cap = sizes["DISPATCH_TABLE_SIZE"][0]
        generate_dtable_pool(counts, max_cap, args.dtable_output)


if __name__ == "__main__":
    main()
