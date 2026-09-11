#!/usr/bin/env python3
#
# SPDX-License-Identifier: Apache-2.0
#
# objz_check_cycles.py - Detect retain cycles in ObjC classes via tree-sitter.
#
# Builds a directed graph of strong object-pointer references between classes
# and reports cycles. Exits non-zero if any cycle is detected.
#
# Usage:
#   python3 objz_check_cycles.py [--include-dir=DIR ...] file1.m file2.m ...

import argparse
import os
import re
import sys
from collections import defaultdict

import tree_sitter_objc as tsobjc
from tree_sitter import Language, Parser, Query, QueryCursor

OBJC_LANGUAGE = Language(tsobjc.language())

Q_IFACE = Query(OBJC_LANGUAGE, "(class_interface) @iface")
Q_IMPL = Query(OBJC_LANGUAGE, "(class_implementation) @impl")
Q_PROP = Query(OBJC_LANGUAGE, "(property_declaration) @prop")

FOUNDATION_CLASSES = frozenset({
    "Object",
    "Protocol",
    "OZString",
    "OZMutableString",
    "OZAutoreleasePool",
    "OZNumber",
    "OZArray",
    "OZDictionary",
    "OZLog",
})

NON_OBJECT_TYPES = frozenset({
    "id",
    "Class",
    "SEL",
    "IMP",
    "void",
})


def parse_args():
    p = argparse.ArgumentParser(
        description="Detect retain cycles in ObjC classes (tree-sitter)")
    p.add_argument("--include-dir", action="append", default=[],
                   help="Additional include directories for #import resolution")
    p.add_argument("sources", nargs="+",
                   help=".m and .h source files to analyze")
    return p.parse_args()


def _resolve_imports(source_path, include_dirs):
    """Extract #import/#include "..." paths and resolve them."""
    resolved = []
    src_dir = os.path.dirname(os.path.abspath(source_path))

    try:
        with open(source_path, "r") as f:
            for line in f:
                m = re.match(r'#(?:import|include)\s+"([^"]+)"', line)
                if not m:
                    continue
                rel = m.group(1)
                for base in [src_dir] + include_dirs:
                    candidate = os.path.join(base, rel)
                    if os.path.isfile(candidate):
                        resolved.append(os.path.abspath(candidate))
                        break
    except OSError:
        pass

    return resolved


def _collect_all_files(source_paths, include_dirs):
    """Collect all .m and resolved .h files to analyze."""
    all_files = set()
    queue = list(source_paths)

    while queue:
        path = queue.pop()
        abs_path = os.path.abspath(path)
        if abs_path in all_files:
            continue
        if not os.path.isfile(abs_path):
            continue
        all_files.add(abs_path)
        for imported in _resolve_imports(abs_path, include_dirs):
            if imported not in all_files:
                queue.append(imported)

    return all_files


def _extract_class_name(iface_node):
    """Extract class name from class_interface node."""
    for child in iface_node.children:
        if child.type == "identifier":
            return child.text.decode()
    return None


def _extract_superclass_name(iface_node):
    """Extract superclass name from class_interface node."""
    for child in iface_node.children:
        if child.type == "superclass_reference":
            for sc in child.children:
                if sc.type == "identifier":
                    return sc.text.decode()
    return None


def _has_unsafe_unretained(node):
    """Check if a node or its text contains __unsafe_unretained."""
    text = node.text.decode()
    return "__unsafe_unretained" in text


def _extract_ivar_refs(iface_node, known_classes):
    """Extract strong object-pointer ivars from instance_variables block.

    Returns list of (ivar_name, target_class).
    """
    refs = []
    for child in iface_node.children:
        if child.type != "instance_variables":
            continue
        for field in child.children:
            if field.type != "field_declaration":
                continue
            ref = _parse_field_declaration(field, known_classes)
            if ref:
                refs.append(ref)
    return refs


def _parse_field_declaration(field_node, known_classes):
    """Parse a single ivar field declaration.

    Returns (ivar_name, target_class) if it's a strong object pointer
    to a known class, else None.
    """
    if _has_unsafe_unretained(field_node):
        return None

    type_name = None
    ivar_name = None
    is_pointer = False

    for child in field_node.children:
        if child.type == "type_identifier":
            type_name = child.text.decode()
        elif child.type == "type_qualifier":
            if child.text.decode() == "__unsafe_unretained":
                return None
        elif child.type == "pointer_declarator":
            is_pointer = True
            for sc in child.children:
                if sc.type == "field_identifier":
                    ivar_name = sc.text.decode()
        elif child.type == "field_identifier":
            ivar_name = child.text.decode()

    if (type_name and ivar_name and is_pointer
            and type_name in known_classes
            and type_name not in NON_OBJECT_TYPES):
        return (ivar_name, type_name)

    return None


def _extract_property_ref(prop_node, known_classes):
    """Extract strong object-pointer property reference.

    Returns (prop_name, target_class) if strong reference to known class,
    else None.
    """
    for child in prop_node.children:
        if child.type == "property_attributes_declaration":
            attrs_text = child.text.decode().lower()
            if "assign" in attrs_text or "unsafe_unretained" in attrs_text:
                return None

    if _has_unsafe_unretained(prop_node):
        return None

    type_name = None
    prop_name = None
    is_pointer = False

    for child in prop_node.children:
        if child.type != "struct_declaration":
            continue
        for sc in child.children:
            if sc.type == "type_identifier":
                type_name = sc.text.decode()
            elif sc.type == "type_qualifier":
                if sc.text.decode() == "__unsafe_unretained":
                    return None
            elif sc.type == "struct_declarator":
                for ssc in sc.children:
                    if ssc.type == "identifier":
                        prop_name = ssc.text.decode()
                    elif ssc.type == "pointer_declarator":
                        is_pointer = True
                        for sssc in ssc.children:
                            if sssc.type == "identifier":
                                prop_name = sssc.text.decode()
            elif sc.type == "identifier":
                prop_name = sc.text.decode()
            elif sc.type == "pointer_declarator":
                is_pointer = True
                for ssc in sc.children:
                    if ssc.type == "identifier":
                        prop_name = ssc.text.decode()

    # Fallback: check raw text for pointer
    if not is_pointer and "*" in prop_node.text.decode():
        is_pointer = True

    if (type_name and prop_name and is_pointer
            and type_name in known_classes
            and type_name not in NON_OBJECT_TYPES):
        return (prop_name, type_name)

    return None


def collect_class_info(file_paths):
    """Parse all files and collect class hierarchy and strong references."""
    parser = Parser(OBJC_LANGUAGE)

    # First pass: collect all known class names
    known_classes = set()

    for path in file_paths:
        with open(path, "rb") as f:
            code = f.read()
        tree = parser.parse(code)
        root = tree.root_node

        for node in QueryCursor(Q_IFACE).captures(root).get("iface", []):
            name = _extract_class_name(node)
            if name:
                known_classes.add(name)

        for node in QueryCursor(Q_IMPL).captures(root).get("impl", []):
            for child in node.children:
                if child.type == "identifier":
                    known_classes.add(child.text.decode())
                    break

    # Second pass: extract hierarchy and strong references
    class_hierarchy = {}
    strong_refs = defaultdict(list)

    for path in file_paths:
        with open(path, "rb") as f:
            code = f.read()
        tree = parser.parse(code)
        root = tree.root_node

        for iface_node in QueryCursor(Q_IFACE).captures(root).get("iface", []):
            class_name = _extract_class_name(iface_node)
            if not class_name or class_name in FOUNDATION_CLASSES:
                continue

            super_name = _extract_superclass_name(iface_node)
            if class_name not in class_hierarchy:
                class_hierarchy[class_name] = super_name

            for (ivar_name, target) in _extract_ivar_refs(iface_node, known_classes):
                if target not in FOUNDATION_CLASSES:
                    entry = (ivar_name, target)
                    if entry not in strong_refs[class_name]:
                        strong_refs[class_name].append(entry)

            props = QueryCursor(Q_PROP).captures(iface_node).get("prop", [])
            for prop_node in props:
                ref = _extract_property_ref(prop_node, known_classes)
                if ref and ref[1] not in FOUNDATION_CLASSES:
                    if ref not in strong_refs[class_name]:
                        strong_refs[class_name].append(ref)

    return class_hierarchy, dict(strong_refs), known_classes


def resolve_inheritance(class_hierarchy, strong_refs):
    """Propagate strong references down the inheritance chain."""
    expanded = defaultdict(list, {k: list(v) for k, v in strong_refs.items()})

    def get_ancestors(cls):
        ancestors = []
        current = class_hierarchy.get(cls)
        visited = set()
        while current and current not in visited:
            visited.add(current)
            ancestors.append(current)
            current = class_hierarchy.get(current)
        return ancestors

    for cls in class_hierarchy:
        for ancestor in get_ancestors(cls):
            if ancestor in strong_refs:
                for ref in strong_refs[ancestor]:
                    if ref not in expanded[cls]:
                        expanded[cls].append(ref)

    return dict(expanded)


def build_graph(strong_refs):
    """Build adjacency list from strong references."""
    graph = defaultdict(set)
    for cls, refs in strong_refs.items():
        for (_field, target) in refs:
            graph[cls].add(target)
    return dict(graph)


def detect_cycles(graph):
    """DFS-based cycle detection. Returns list of cycle paths."""
    WHITE, GRAY, BLACK = 0, 1, 2
    color = defaultdict(int)
    cycles = []
    path = []

    def dfs(node):
        color[node] = GRAY
        path.append(node)

        for neighbor in graph.get(node, set()):
            if color[neighbor] == GRAY:
                cycle_start = path.index(neighbor)
                cycle = path[cycle_start:] + [neighbor]
                cycles.append(cycle)
            elif color[neighbor] == WHITE:
                dfs(neighbor)

        path.pop()
        color[node] = BLACK

    for node in graph:
        if color[node] == WHITE:
            dfs(node)

    return cycles


def report_cycles(cycles, strong_refs):
    """Print cycle details to stderr. Returns exit code."""
    if not cycles:
        return 0

    # Deduplicate: normalize cycles by rotating to smallest class name
    seen = set()
    unique_cycles = []
    for cycle in cycles:
        loop = cycle[:-1]
        min_idx = loop.index(min(loop))
        normalized = tuple(loop[min_idx:] + loop[:min_idx])
        if normalized not in seen:
            seen.add(normalized)
            unique_cycles.append(cycle)

    p = lambda *a, **kw: print(*a, file=sys.stderr, **kw)

    p(f"\nerror: {len(unique_cycles)} retain cycle(s) detected\n")

    for cycle in unique_cycles:
        cycle_str = " -> ".join(cycle)
        p(f"  cycle: {cycle_str}")

        for i in range(len(cycle) - 1):
            src = cycle[i]
            dst = cycle[i + 1]
            for (field, target) in strong_refs.get(src, []):
                if target == dst:
                    p(f"    {src}.{field} ({target} *) \u2014 strong")
                    break
        p()

    p("hint: add __unsafe_unretained to one reference in each cycle")
    p("      (__weak is not supported and panics at runtime)\n")

    return 1


def main():
    args = parse_args()

    all_files = _collect_all_files(args.sources, args.include_dir)

    class_hierarchy, strong_refs, known_classes = collect_class_info(all_files)

    if not strong_refs:
        return 0

    expanded_refs = resolve_inheritance(class_hierarchy, strong_refs)
    graph = build_graph(expanded_refs)
    cycles = detect_cycles(graph)

    exit_code = report_cycles(cycles, expanded_refs)

    if exit_code == 0:
        n_classes = len(known_classes - FOUNDATION_CLASSES)
        n_edges = sum(len(v) for v in expanded_refs.values())
        print(f"objz_check_cycles: {n_classes} classes, "
              f"{n_edges} strong refs, no cycles", file=sys.stderr)

    return exit_code


if __name__ == "__main__":
    sys.exit(main())
