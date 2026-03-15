# SPDX-License-Identifier: Apache-2.0
"""Build Jinja2 context dict for extracted templates.

Maps tree-sitter node positions (_n_L_C keys) to rendered C strings
produced by the existing AST emitters in emit.py.
"""

from __future__ import annotations

from io import StringIO
from pathlib import Path

import tree_sitter_objc as tsobjc
from tree_sitter import Language, Parser

from oz_transpile.extract import (
    _OBJC_IMPL_TYPES,
    _extract_class_name,
    _extract_selector,
    _impl_loc_key,
    _is_class_method,
    _loc_key,
)
from oz_transpile.model import OZClass, OZModule

_TS_LANG = Language(tsobjc.language())


def build_source_context(
    source_path: Path,
    module: OZModule,
    classes: list[OZClass],
    stem: str,
    root_class: str,
    has_item_pool: bool,
    pool_count_fn=None,
    extra_static_names: set[str] | None = None,
) -> dict[str, str]:
    """Build context dict mapping _n_L_C keys to rendered C strings.

    Args:
        source_path: Path to original .m file.
        module: Resolved OZModule.
        classes: List of OZClass objects for this stem.
        stem: Header stem (e.g., "OZLed").
        root_class: Name of root class (e.g., "OZObject").
        has_item_pool: Whether item pool is needed.
        pool_count_fn: Callable(cls_name) -> pool count.
        extra_static_names: Additional static variable names to blank.

    Returns:
        Dict mapping placeholder keys to rendered C strings.
    """
    from oz_transpile.emit import (
        _emit_include_replacement,
        _emit_transpiled_function,
        _extract_decl_name,
        _extract_func_name,
        _is_func_prototype,
    )

    source = source_path.read_bytes()
    parser = Parser(_TS_LANG)
    tree = parser.parse(source)

    context: dict[str, str] = {}
    class_map = {c.name: c for c in classes}
    func_map = {}
    for cls in classes:
        for f in cls.functions:
            func_map[f.name] = f
    for f in module.functions:
        func_map[f.name] = f

    static_names = set(extra_static_names) if extra_static_names else set()
    for cls in classes:
        for sv in cls.statics:
            static_names.add(sv.name)

    source_dir = source_path.parent

    for child in tree.root_node.children:
        text = source[child.start_byte:child.end_byte].decode()

        if child.type in _OBJC_IMPL_TYPES:
            _build_impl_context(
                child, source, context, class_map, module,
                root_class, has_item_pool,
            )

        elif child.type == "preproc_include":
            key = _loc_key(child)
            buf = StringIO()
            _emit_include_replacement(text, buf, source_dir)
            inc_text = buf.getvalue().strip()
            context[key] = inc_text if inc_text else ""

        elif child.type == "function_definition":
            key = _loc_key(child)
            name = _extract_func_name(child)
            func = func_map.get(name) if name else None
            if func and func.body_ast:
                buf = StringIO()
                _emit_transpiled_function(func, module, buf, root_class,
                                          has_item_pool)
                context[key] = buf.getvalue()
            else:
                context[key] = text

        elif child.type == "declaration":
            key = _loc_key(child)
            if _is_func_prototype(child):
                context[key] = ""
                continue
            name = _extract_decl_name(child)
            if name and name in static_names:
                context[key] = ""
                continue
            context[key] = text

    return context


def _build_impl_context(
    node, source: bytes, context: dict[str, str],
    class_map: dict[str, OZClass], module: OZModule,
    root_class: str, has_item_pool: bool,
) -> None:
    """Build context entries for methods inside @implementation."""
    from oz_transpile.emit import (
        _EmitCtx,
        _emit_auto_dealloc,
        _emit_compound_stmt,
        _emit_root_introspection,
        _emit_root_retain_release,
        _emit_synthesized_accessor,
        _emit_user_dealloc,
        _method_prototype,
        _object_params,
    )

    class_name = _extract_class_name(node)
    cls = class_map.get(class_name) if class_name else None
    if not cls:
        if class_name:
            module.diagnostics.append(
                f"warning: @implementation {class_name} not found in module"
            )
        context[_impl_loc_key(node)] = ""
        return

    ctx = _EmitCtx(cls=cls, module=module, root_class=root_class,
                   has_item_pool=has_item_pool)
    is_root = cls.name == root_class
    _root_skip_sels = {"retain", "release", "retainCount",
                       "isEqual:", "cDescription:maxLength:"}

    # Collect all method_definition nodes
    method_nodes = []
    for child in node.children:
        if child.type == "implementation_definition":
            md = child.children[0] if child.children else None
            if md and md.type == "method_definition":
                method_nodes.append(md)

    # Build preamble: static vars, root retain/release, root introspection
    preamble = StringIO()
    for sv in cls.statics:
        decl_str = sv.oz_type.c_param_decl(sv.name)
        init = f" = {sv.init_value}" if sv.init_value is not None else ""
        preamble.write(f"static {decl_str}{init};\n")

    if is_root:
        _emit_root_retain_release(cls, module, preamble)
        _emit_root_introspection(cls, preamble)

    # Render each method body
    has_user_dealloc = False

    for md in method_nodes:
        sel = _extract_selector(md, source)
        is_cls_method = _is_class_method(md)
        key = _loc_key(md)

        m = None
        for candidate in cls.methods:
            if (candidate.selector == sel
                    and candidate.is_class_method == is_cls_method):
                m = candidate
                break

        if not m:
            context[key] = ""
            continue

        if m.selector in _root_skip_sels and is_root:
            context[key] = ""
            continue

        if m.selector == "dealloc":
            has_user_dealloc = True
            buf = StringIO()
            _emit_user_dealloc(ctx, m, buf)
            context[key] = buf.getvalue()
            continue

        ctx.method = m
        ctx.scope_vars = []
        ctx.consumed_vars = set()
        ctx.loop_scope_depth = []
        ctx.pre_stmts = []
        ctx._tmp_counter = 0
        ctx._sync_counter = 0

        buf = StringIO()
        buf.write(f"{_method_prototype(cls, m)}\n")
        if m.synthesized_property:
            _emit_synthesized_accessor(cls, m, buf, root_class)
        elif m.body_ast:
            _emit_compound_stmt(m.body_ast, buf, ctx, indent=0,
                                param_retains=_object_params(m))
        else:
            buf.write("{\n}\n")
        context[key] = buf.getvalue()

    # Emit synthesized methods not found in tree-sitter nodes
    emitted_sels = set()
    for md in method_nodes:
        sel = _extract_selector(md, source)
        is_cm = _is_class_method(md)
        emitted_sels.add((sel, is_cm))

    synth_buf = StringIO()
    for m in cls.methods:
        key = (m.selector, m.is_class_method)
        if key in emitted_sels:
            continue
        if m.selector in _root_skip_sels and is_root:
            continue
        if m.selector == "dealloc":
            continue
        if not m.synthesized_property and not m.body_ast:
            continue
        ctx.method = m
        ctx.scope_vars = []
        ctx.consumed_vars = set()
        ctx.loop_scope_depth = []
        ctx.pre_stmts = []
        ctx._tmp_counter = 0
        ctx._sync_counter = 0
        buf = StringIO()
        buf.write(f"{_method_prototype(cls, m)}\n")
        if m.synthesized_property:
            _emit_synthesized_accessor(cls, m, buf, root_class)
        elif m.body_ast:
            _emit_compound_stmt(m.body_ast, buf, ctx, indent=0,
                                param_retains=_object_params(m))
        synth_buf.write(buf.getvalue())
        synth_buf.write("\n")

    # Auto-dealloc appended to last method's value
    dealloc_text = ""
    if not has_user_dealloc and cls.name != "OZLock":
        buf = StringIO()
        _emit_auto_dealloc(ctx, buf)
        dealloc_text = buf.getvalue()
        if dealloc_text and method_nodes:
            last_key = _loc_key(method_nodes[-1])
            if last_key in context:
                context[last_key] += "\n" + dealloc_text
            dealloc_text = ""

    # Build preamble text in dependency order:
    # 1. Static variables
    # 2. Root retain/release/introspection (already in preamble StringIO)
    # 3. String constants (referenced by block functions)
    # 4. Block functions (may reference string constants)
    # 5. Synthesized property accessors
    # 6. Auto-dealloc (if no method to attach to)
    preamble_text = preamble.getvalue()
    for sc in ctx.string_constants:
        preamble_text += sc + "\n"
    for bf in ctx.block_functions:
        preamble_text += bf + "\n\n"
    preamble_text += synth_buf.getvalue()
    if dealloc_text:
        preamble_text += dealloc_text

    # Store in the _impl_ key (always present in template)
    context[_impl_loc_key(node)] = preamble_text
