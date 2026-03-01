#!/usr/bin/env python3
#
# SPDX-License-Identifier: Apache-2.0
#
# gen_pools.py - Generate OZ_DEFINE_POOL() calls from Clang AST analysis.
#
# Parses Clang JSON AST dumps to determine:
#   - Instance sizes from ObjCInterfaceDecl ivar layouts
#   - Max concurrent object counts from alloc patterns + ARC scoping
#
# Usage:
#   python3 gen_pools.py --pointer-size=4 --output=pools.c f1.ast.json ...

import argparse
import json
import sys
from collections import defaultdict

SKIP_CLASSES = frozenset({
    "Object", "Protocol", "OZString", "OZMutableString",
    "OZAutoreleasePool", "OZNumber", "OZArray", "OZDictionary",
    "OZLog",
})


def parse_args():
    p = argparse.ArgumentParser(
        description="Generate pools.c from Clang AST")
    p.add_argument("--pointer-size", type=int, default=4, choices=[4, 8])
    p.add_argument("--output", required=True,
                   help="Output path for generated pools.c")
    p.add_argument("ast_files", nargs="+")
    return p.parse_args()


def type_size(qual_type, ptr_size):
    """Resolve a qualType string to a byte size."""
    qt = qual_type
    for qual in ("__strong", "__weak", "__unsafe_unretained",
                 "__autoreleasing", "_Nonnull", "_Nullable"):
        qt = qt.replace(qual, "")
    qt = qt.strip()

    if "*" in qt or qt in ("id", "Class", "SEL", "IMP"):
        return ptr_size

    sizes = {
        "int": 4, "unsigned int": 4, "uint32_t": 4, "int32_t": 4,
        "short": 2, "unsigned short": 2, "uint16_t": 2, "int16_t": 2,
        "char": 1, "unsigned char": 1, "uint8_t": 1, "int8_t": 1,
        "BOOL": 1, "_Bool": 1, "bool": 1,
        "float": 4, "double": 8,
        "atomic_t": 4,
        "long": ptr_size, "unsigned long": ptr_size,
        "long long": 8, "unsigned long long": 8,
        "uint64_t": 8, "int64_t": 8,
        "size_t": ptr_size, "ptrdiff_t": ptr_size, "uintptr_t": ptr_size,
    }
    if qt in sizes:
        return sizes[qt]

    return ptr_size


def align_up(size, align):
    return (size + align - 1) & ~(align - 1)


# -------------------------------------------------------------------
# Phase A: class sizes
# -------------------------------------------------------------------

def parse_classes(ast_roots):
    """Extract {name: {super, ivars}} from ObjCInterfaceDecl nodes."""
    classes = {}

    def walk(node):
        if node.get("kind") == "ObjCInterfaceDecl":
            name = node.get("name", "")
            if not name:
                return
            super_name = node.get("super", {}).get("name", "")
            ivars = []
            for child in node.get("inner", []):
                if child.get("kind") == "ObjCIvarDecl":
                    ivars.append(child.get("type", {}).get("qualType", ""))
            if name not in classes or ivars:
                classes[name] = {"super": super_name, "ivars": ivars}
        for child in node.get("inner", []):
            walk(child)

    for root in ast_roots:
        walk(root)
    return classes


def compute_class_sizes(classes, ptr_size):
    """Compute instance sizes including inherited ivars."""
    cache = {}

    def get_size(name):
        if name in cache:
            return cache[name]
        cls = classes.get(name)
        if cls is None:
            cache[name] = ptr_size + 4
            return cache[name]

        sup = cls["super"]
        if sup and sup in classes:
            base = get_size(sup)
        elif sup:
            base = ptr_size + 4
        else:
            base = 0

        own = sum(type_size(qt, ptr_size) for qt in cls["ivars"])
        total = align_up(base + own, ptr_size)
        cache[name] = total
        return total

    for name in classes:
        get_size(name)
    return cache


# -------------------------------------------------------------------
# Phase B: alloc counting + call graph
# -------------------------------------------------------------------

def extract_class_from_type(qual_type):
    """'Driver *__strong' -> 'Driver', 'Sensor *' -> 'Sensor'."""
    qt = qual_type
    for qual in ("__strong", "__weak", "__unsafe_unretained",
                 "__autoreleasing", "_Nonnull", "_Nullable"):
        qt = qt.replace(qual, "")
    qt = qt.strip()
    if qt.endswith("*"):
        qt = qt[:-1].strip()
    if qt and qt[0].isupper() and " " not in qt:
        return qt
    return None


def _callee_name(call_node):
    """Extract function name from CallExpr."""
    for child in call_node.get("inner", []):
        kind = child.get("kind", "")
        if kind == "DeclRefExpr":
            return child.get("referencedDecl", {}).get("name", "")
        if kind == "ImplicitCastExpr":
            return _callee_name(child)
    return None


def analyze_ast(ast_roots):
    """
    Return (allocs, call_graph, thread_entries).
      allocs:     func_key -> {class_name: count}
      call_graph: func_key -> {callee_key: count}
      thread_entries: set of func_keys with (void*,void*,void*) signature
    """
    allocs = defaultdict(lambda: defaultdict(int))
    call_graph = defaultdict(lambda: defaultdict(int))
    thread_entries = set()

    def walk(node, func_key=None, impl_name=None):
        kind = node.get("kind", "")

        if kind == "ObjCImplementationDecl":
            impl_name = node.get("name", "")

        new_key = func_key
        if kind == "FunctionDecl" and node.get("name"):
            new_key = node["name"]
            _detect_thread_entry(node, thread_entries)
        elif kind == "ObjCMethodDecl" and impl_name:
            sel = node.get("name", "")
            new_key = f"{impl_name}.{sel}"

        if kind == "ObjCMessageExpr" and new_key:
            sel = node.get("selector", "")
            if sel == "alloc" and node.get("receiverKind") == "class":
                cls = node.get("classType", {}).get("qualType", "")
                if cls:
                    allocs[new_key][cls] += 1
            elif node.get("receiverKind") == "instance" and sel:
                inner = node.get("inner", [])
                if inner:
                    recv_type = inner[0].get("type", {}).get("qualType", "")
                    recv_cls = extract_class_from_type(recv_type)
                    if recv_cls:
                        callee_key = f"{recv_cls}.{sel}"
                        call_graph[new_key][callee_key] += 1

        if kind == "CallExpr" and new_key:
            callee = _callee_name(node)
            if callee:
                call_graph[new_key][callee] += 1

        for child in node.get("inner", []):
            walk(child, new_key, impl_name)

    for root in ast_roots:
        walk(root)

    return allocs, call_graph, thread_entries


def _detect_thread_entry(func_node, entries):
    """Detect Zephyr thread entries: static void f(void*,void*,void*)."""
    name = func_node.get("name", "")
    if not name or name == "main":
        return
    params = [c for c in func_node.get("inner", [])
              if c.get("kind") == "ParmVarDecl"]
    if len(params) != 3:
        return
    if all("void *" in p.get("type", {}).get("qualType", "")
           for p in params):
        entries.add(name)


def effective_allocs(func_key, allocs, call_graph, visited=None):
    """Compute allocs reachable from func_key through call graph."""
    if visited is None:
        visited = set()
    if func_key in visited:
        return defaultdict(int)
    visited.add(func_key)

    result = defaultdict(int)
    for cls, cnt in allocs.get(func_key, {}).items():
        result[cls] += cnt

    for callee, call_cnt in call_graph.get(func_key, {}).items():
        sub = effective_allocs(callee, allocs, call_graph, visited.copy())
        for cls, cnt in sub.items():
            result[cls] += cnt * call_cnt

    return result


def compute_pool_counts(allocs, call_graph, thread_entries):
    """Max concurrent counts: main + thread entries (additive).

    When main() is not in the analyzed sources (e.g. test helpers where
    main is in a separate .c file), fall back to treating every function
    that allocates objects as a potential entry point and take the max
    count per class across all functions.
    """
    counts = defaultdict(int)

    main_a = effective_allocs("main", allocs, call_graph)
    for cls, cnt in main_a.items():
        counts[cls] += cnt

    for entry in thread_entries:
        entry_a = effective_allocs(entry, allocs, call_graph)
        for cls, cnt in entry_a.items():
            counts[cls] += cnt

    if not counts:
        for func_key in allocs:
            ea = effective_allocs(func_key, allocs, call_graph)
            for cls, cnt in ea.items():
                if cnt > counts[cls]:
                    counts[cls] = cnt
        counts = {cls: cnt for cls, cnt in counts.items()
                  if "pool" in cls.lower()
                  and "unpool" not in cls.lower()}

    return counts


# -------------------------------------------------------------------
# Generate pools.c
# -------------------------------------------------------------------

def generate(pools, output_path):
    with open(output_path, "w") as f:
        f.write("/* Auto-generated by tools/gen_pools.py — do not edit */\n")
        f.write("#include <objc/pool.h>\n\n")
        for cls, (bsz, cnt, align) in sorted(pools.items()):
            f.write(f"OZ_DEFINE_POOL({cls}, {bsz}, {cnt}, {align});\n")


# -------------------------------------------------------------------
# main
# -------------------------------------------------------------------

def main():
    args = parse_args()
    ptr = args.pointer_size

    ast_roots = []
    for path in args.ast_files:
        with open(path) as f:
            ast_roots.append(json.load(f))

    classes = parse_classes(ast_roots)
    sizes = compute_class_sizes(classes, ptr)
    allocs, call_graph, thread_entries = analyze_ast(ast_roots)
    pool_counts = compute_pool_counts(allocs, call_graph, thread_entries)

    pools = {}
    for cls, cnt in pool_counts.items():
        if cls in SKIP_CLASSES or cnt <= 0:
            continue
        bsz = align_up(sizes.get(cls, ptr + 4), ptr)
        pools[cls] = (bsz, cnt, ptr)

    generate(pools, args.output)

    if pools:
        for cls, (bsz, cnt, align) in sorted(pools.items()):
            print(f"gen_pools.py: {cls} block_size={bsz} count={cnt} "
                  f"align={align}", file=sys.stderr)
    else:
        print("gen_pools.py: no user classes with alloc calls found",
              file=sys.stderr)


if __name__ == "__main__":
    main()
