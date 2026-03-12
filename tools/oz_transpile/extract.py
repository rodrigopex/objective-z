# SPDX-License-Identifier: Apache-2.0
"""Tree-sitter CST → Jinja2 template string.

Walks an Objective-C .m source file via tree-sitter, replacing ObjC
constructs with {{ _n_L_C }} Jinja placeholders and annotating comments.
C code, comments, and whitespace are preserved verbatim.
"""

from __future__ import annotations

from io import StringIO
from pathlib import Path

import tree_sitter_objc as tsobjc
from tree_sitter import Language, Parser

_TS_LANG = Language(tsobjc.language())

_OBJC_INTERFACE_TYPES = frozenset({"class_interface", "category_interface"})
_OBJC_IMPL_TYPES = frozenset({"class_implementation", "category_implementation"})
_OBJC_SKIP_TYPES = frozenset({"protocol_declaration"})

_OBJC_IMPL_SKIP_CHILDREN = frozenset({
    "@implementation", "@end", "identifier", "superclass_reference",
    "instance_variables", "(", ")",
})


def _loc_key(node) -> str:
    """Build a Jinja variable name from node source position."""
    line = node.start_point[0] + 1
    col = node.start_point[1] + 1
    return f"_n_{line}_{col}"


def _extract_class_name(node) -> str | None:
    """Extract class name from class_implementation/class_interface node."""
    for child in node.children:
        if child.type == "identifier":
            return child.text.decode()
    return None


def _extract_protocol_name(node) -> str | None:
    """Extract protocol name from protocol_declaration node."""
    for child in node.children:
        if child.type == "identifier":
            return child.text.decode()
    return None


def _extract_selector(node, source: bytes) -> str:
    """Extract ObjC selector from a method_definition node."""
    parts = []
    for child in node.children:
        if child.type == "identifier":
            parts.append(source[child.start_byte:child.end_byte].decode())
        elif child.type == "method_parameter":
            for sub in child.children:
                if sub.type == ":":
                    parts.append(":")
    return "".join(parts)


def _is_class_method(node) -> bool:
    """Check if method_definition is a class method (+) vs instance (-)."""
    return len(node.children) > 0 and node.children[0].type == "+"


def _impl_loc_key(node) -> str:
    """Build a Jinja variable name for the @implementation preamble."""
    line = node.start_point[0] + 1
    col = node.start_point[1] + 1
    return f"_impl_{line}_{col}"


def _emit_impl_block(node, source: bytes, out: StringIO) -> None:
    """Handle @implementation block: annotate methods, preserve verbatim."""
    class_name = _extract_class_name(node)
    if not class_name:
        out.write(source[node.start_byte:node.end_byte].decode())
        return

    out.write(f"/* @implementation {class_name} */\n")
    out.write("{{ " + _impl_loc_key(node) + " }}")
    out.write("\n")

    for child in node.children:
        if child.type in _OBJC_IMPL_SKIP_CHILDREN:
            if child.type == "@end":
                out.write(f"/* @end {class_name} */\n")
            continue

        if child.type == "implementation_definition":
            inner = child.children[0] if child.children else None
            if inner and inner.type == "method_definition":
                sel = _extract_selector(inner, source)
                sign = "+" if _is_class_method(inner) else "-"
                out.write(f"\n/* {sign}[{class_name} {sel}] */\n")
                out.write("{{ " + _loc_key(inner) + " }}\n")
            elif inner and inner.type == "property_implementation":
                pass  # handled by resolve phase
            else:
                out.write(source[child.start_byte:child.end_byte].decode())
                out.write("\n")

        elif child.type == "comment":
            out.write(source[child.start_byte:child.end_byte].decode())
            out.write("\n")

        elif child.type.startswith("preproc_"):
            out.write(source[child.start_byte:child.end_byte].decode())
            out.write("\n")

        else:
            text = source[child.start_byte:child.end_byte].decode().strip()
            if text:
                out.write(text)
                out.write("\n")


def extract_template(source: bytes, tree=None) -> str:
    """Convert ObjC .m source to a Jinja2 template string.

    ObjC constructs become {{ _n_L_C }} placeholders with annotating
    comments.  C code, comments, and whitespace are preserved verbatim.

    Args:
        source: Raw bytes of the .m file.
        tree: Pre-parsed tree-sitter tree (optional; parsed if None).

    Returns:
        Jinja2 template string.
    """
    if tree is None:
        parser = Parser(_TS_LANG)
        tree = parser.parse(source)

    out = StringIO()

    for child in tree.root_node.children:
        text = source[child.start_byte:child.end_byte].decode()

        if child.type in _OBJC_INTERFACE_TYPES:
            name = _extract_class_name(child)
            if name:
                out.write(f"/* @interface {name} — see {name}_ozh.h */\n")
            else:
                out.write(f"/* @interface — see _ozh.h */\n")

        elif child.type in _OBJC_IMPL_TYPES:
            _emit_impl_block(child, source, out)

        elif child.type in _OBJC_SKIP_TYPES:
            name = _extract_protocol_name(child)
            if name:
                out.write(f"/* @protocol {name} — see oz_dispatch.h */\n")
            else:
                out.write(f"/* @protocol — see oz_dispatch.h */\n")

        elif child.type == "preproc_include":
            out.write("{{ " + _loc_key(child) + " }}")
            out.write("\n")

        elif child.type == "function_definition":
            out.write("{{ " + _loc_key(child) + " }}")
            out.write("\n")

        elif child.type == "declaration":
            out.write("{{ " + _loc_key(child) + " }}")
            out.write("\n")

        else:
            out.write(text)
            out.write("\n")

    return out.getvalue()
