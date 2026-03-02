#!/usr/bin/env python3
#
# SPDX-License-Identifier: Apache-2.0
#
# objz_gen_table_sizes.py - Generate runtime table_sizes.h and dispatch_init.c
#                           from tree-sitter analysis of Objective-C sources.
#
# Parses .m files directly (no Clang AST dumps needed) to count:
#   - @implementation declarations  → class table size
#   - @implementation Foo(Cat)      → category table size
#   - @protocol declarations        → protocol table size
#   - method definitions            → hash table size
#   - unique selector names         → flat dispatch table sizing
#
# Usage:
#   python3 objz_gen_table_sizes.py --output=table_sizes.h file1.m file2.m ...
#   python3 objz_gen_table_sizes.py --output=table_sizes.h \
#       --dispatch-init-output=dispatch_init.c file1.m file2.m ...

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
Q_IFACE = Query(OBJC_LANGUAGE, "(class_interface) @iface")
Q_PROP = Query(OBJC_LANGUAGE, "(property_declaration) @prop")


def parse_args():
    p = argparse.ArgumentParser(
        description="Generate table_sizes.h from ObjC sources (tree-sitter)")
    p.add_argument("--pointer-size", type=int, default=4, choices=[4, 8])
    p.add_argument("--output", required=True,
                   help="Output path for generated table_sizes.h")
    p.add_argument("--dispatch-init-output", default=None,
                   help="Output path for generated dispatch_init.c")
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


def _extract_selector_name(method_node):
    """Extract the full selector name from a method_definition node.

    tree-sitter-objc structure:
      Unary:   method_definition -> ... method_type identifier compound_statement
      Keyword: method_definition -> ... method_type identifier method_parameter
                                                   identifier method_parameter ...

    Unary selectors: single identifier after method_type (e.g. 'init').
    Keyword selectors: interleaved identifier + method_parameter nodes.
    Full selector = join(keyword + ':' for each identifier/method_parameter pair).
    """
    parts = []
    saw_method_type = False
    for child in method_node.children:
        if child.type == "method_type":
            saw_method_type = True
            continue
        if not saw_method_type:
            continue
        if child.type == "compound_statement":
            break
        if child.type == "identifier":
            parts.append(child.text.decode())
        elif child.type == "method_parameter":
            if parts:
                parts[-1] += ":"
            else:
                parts.append(":")
    return "".join(parts) if parts else None


def _extract_property_selectors(prop_node):
    """Extract getter/setter selector names from a property_declaration.

    Returns a list of selector names (getter, and setter if not readonly).
    Property structure: @property (attrs) type name;
    The name is inside a struct_declaration child.
    """
    is_readonly = False
    custom_getter = None
    custom_setter = None

    # Check property attributes
    for child in prop_node.children:
        if child.type == "property_attributes_declaration":
            text = child.text.decode()
            if "readonly" in text:
                is_readonly = True
            # Parse custom getter=name or setter=name:
            for part in text.strip("()").split(","):
                part = part.strip()
                if part.startswith("getter="):
                    custom_getter = part[len("getter="):]
                elif part.startswith("setter="):
                    custom_setter = part[len("setter="):]

    # Extract property name from struct_declaration
    prop_name = None
    for child in prop_node.children:
        if child.type == "struct_declaration":
            # Last identifier in the struct_declaration is the property name
            for sc in reversed(child.children):
                if sc.type == "identifier":
                    prop_name = sc.text.decode()
                    break
                elif sc.type == "struct_declarator":
                    for ssc in reversed(sc.children):
                        if ssc.type == "identifier":
                            prop_name = ssc.text.decode()
                            break
                    if prop_name:
                        break
            break

    if not prop_name:
        return []

    selectors = []
    # Getter
    getter = custom_getter if custom_getter else prop_name
    selectors.append(getter)

    # Setter (unless readonly)
    if not is_readonly:
        if custom_setter:
            setter = custom_setter
        else:
            setter = "set" + prop_name[0].upper() + prop_name[1:] + ":"
        selectors.append(setter)

    return selectors


def count_metadata(source_paths):
    """Parse .m files with tree-sitter and count ObjC metadata.

    Returns dict with n_classes, n_categories, n_protocols,
    n_methods, max_methods_per_class, per_class_methods,
    selector_names.
    """
    parser = Parser(OBJC_LANGUAGE)

    impl_names = set()
    n_categories = 0
    protocol_names = set()
    n_methods = 0
    methods_per_class = defaultdict(int)
    instance_methods_per_class = defaultdict(int)
    class_methods_per_class = defaultdict(int)
    selector_names = set()

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

            for m in methods:
                sel_name = _extract_selector_name(m)
                if sel_name:
                    selector_names.add(sel_name)

                if not is_cat:
                    if _is_class_method(m):
                        class_methods_per_class[class_name] += 1
                    else:
                        instance_methods_per_class[class_name] += 1

            if not is_cat:
                methods_per_class[class_name] += len(methods)

        # Extract property declarations from @interface blocks
        ifaces = QueryCursor(Q_IFACE).captures(root).get("iface", [])
        for iface_node in ifaces:
            props = QueryCursor(Q_PROP).captures(iface_node).get("prop", [])
            for prop_node in props:
                for sel in _extract_property_selectors(prop_node):
                    selector_names.add(sel)

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
        "selector_names": selector_names,
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
    n_sels = len(counts["selector_names"])

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

    # Pool table: n_pools + margin
    pool_ts = max(n_pools + 2, 4)
    pool_f = f"max({n_pools} + 2, 4)"

    # Flat dispatch: selector enumeration
    sel_bits = max(1, (n_sels - 1).bit_length()) if n_sels > 0 else 1
    sel_count = 1 << sel_bits
    flat_ts = class_ts * sel_count
    sel_bits_f = f"ceil(log2({n_sels}))" if n_sels > 1 else "1"

    sizes = {
        "CLASS_TABLE_SIZE": (class_ts, class_f),
        "CATEGORY_TABLE_SIZE": (category_ts, category_f),
        "PROTOCOL_TABLE_SIZE": (protocol_ts, protocol_f),
        "HASH_TABLE_SIZE": (hash_ts, hash_f),
        "STATIC_POOL_TABLE_SIZE": (pool_ts, pool_f),
        "NUM_SELECTORS": (n_sels, f"{n_sels} unique selectors"),
        "SEL_BITS": (sel_bits, sel_bits_f),
        "SEL_COUNT": (sel_count, f"1 << {sel_bits}"),
        "FLAT_DISPATCH_TABLE_SIZE": (flat_ts, f"{class_ts} * {sel_count}"),
    }

    return sizes


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


def generate_dispatch_init(counts, output_path):
    """Write generated dispatch_init.c with selector init table."""
    selectors = sorted(counts["selector_names"])

    with open(output_path, "w") as f:
        f.write("/* Auto-generated by objz_gen_table_sizes.py — do not edit */\n")
        f.write("#include <objc/dispatch.h>\n\n")
        f.write("const struct objz_sel_init_entry __objz_sel_init_table[] = {\n")
        for i, sel in enumerate(selectors):
            f.write(f'\t{{"{sel}", {i}}},\n')
        f.write("};\n\n")
        f.write(f"const uint16_t __objz_sel_init_count = {len(selectors)};\n")

    # Print selector table to stderr
    p = lambda *a, **kw: print(*a, file=sys.stderr, **kw)
    p(f"objz_gen_table_sizes: flat dispatch ({len(selectors)} selectors)")
    if selectors:
        id_w = len(str(len(selectors) - 1))
        for i, sel in enumerate(selectors):
            p(f"  {i:>{id_w}}  {sel}")


def _print_table(counts, sizes):
    """Print aligned table of computed sizes with formulas to stderr."""
    nc = counts["n_classes"]
    ncat = counts["n_categories"]
    nprot = counts["n_protocols"]
    nm = counts["n_methods"]
    max_mpc = counts["max_methods_per_class"]
    n_sels = len(counts["selector_names"])

    p = lambda *a, **kw: print(*a, file=sys.stderr, **kw)

    p(f"objz_gen_table_sizes: {nc} classes, {ncat} categories, "
      f"{nprot} protocols, {nm} methods (max {max_mpc}/class), "
      f"{n_sels} unique selectors")

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

    if args.dispatch_init_output:
        generate_dispatch_init(counts, args.dispatch_init_output)


if __name__ == "__main__":
    main()
