# SPDX-License-Identifier: Apache-2.0
#
# emit.py - Pass 3: OZModule -> .h/.c files

from __future__ import annotations

import os
from dataclasses import dataclass, field
from io import StringIO

from jinja2 import Environment, FileSystemLoader

from .model import (DispatchKind, OZClass, OZFunction, OZIvar, OZMethod,
                     OZModule, OZParam, OZType, OrphanSource)


@dataclass
class _EmitCtx:
    cls: OZClass
    module: OZModule
    root_class: str
    method: OZMethod | None = None
    scope_vars: list[dict[str, OZType]] = field(default_factory=list)
    consumed_vars: set[str] = field(default_factory=set)
    loop_scope_depth: list[int] = field(default_factory=list)
    pre_stmts: list[str] = field(default_factory=list)
    string_constants: list[str] = field(default_factory=list)
    block_functions: list[str] = field(default_factory=list)
    has_item_pool: bool = False
    _tmp_counter: int = 0
    _sync_counter: int = 0
    _string_dedup: dict[str, str] = field(default_factory=dict)


def _create_env() -> Environment:
    """Create Jinja2 environment loading templates from the templates/ directory."""
    tmpl_dir = os.path.join(os.path.dirname(__file__), "templates")
    return Environment(
        loader=FileSystemLoader(tmpl_dir),
        autoescape=False,
        keep_trailing_newline=True,
        trim_blocks=True,
    )


def _render(env: Environment, template_name: str, context: dict,
            outdir: str, filename: str) -> str:
    """Render a Jinja2 template to a file. Returns the output path."""
    tmpl = env.get_template(template_name)
    content = tmpl.render(**context)
    path = os.path.join(outdir, filename)
    _write_file(path, content)
    return path


def _inject_oz_lock(module: OZModule, root_class: str) -> None:
    """Add synthetic OZLock class if any @synchronized is used."""
    counts = _count_alloc_calls(module)
    if counts.get("OZLock", 0) == 0:
        return
    if "OZLock" in module.classes:
        return
    max_id = max((c.class_id for c in module.classes.values()), default=-1)
    module.classes["OZLock"] = OZClass(
        name="OZLock",
        superclass=root_class,
        ivars=[
            OZIvar("_lock", OZType("oz_spinlock_t")),
            OZIvar("_key", OZType("oz_spinlock_key_t")),
            OZIvar("_obj", OZType("OZObject *")),
        ],
        class_id=max_id + 1,
        base_depth=1,
    )


def _header_stem(cls: OZClass) -> str:
    """Return the file stem for a class: source_stem if set, else class name."""
    return cls.source_stem or cls.name


def _associate_module_items(module: OZModule) -> None:
    """Move module-level functions/statics/verbatim/includes into the primary class.

    Each source file typically defines one class alongside free functions.
    This associates those items with the class for per-file emission.
    """
    if not module.classes:
        return
    if not (module.functions or module.statics
            or module.verbatim_lines or module.user_includes):
        return
    primary = None
    for cls in module.classes.values():
        if any(m.body_ast for m in cls.methods):
            primary = cls
            break
    if primary is None:
        for cls in module.classes.values():
            if cls.methods or cls.ivars:
                primary = cls
                break
    if primary is None:
        primary = next(iter(module.classes.values()))
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


def emit(module: OZModule, outdir: str, pool_sizes: dict[str, int] | None = None,
         root_class: str = "OZObject",
         item_pool_size: int | None = None) -> list[str]:
    """Generate C files from OZModule. Returns list of generated file paths."""
    os.makedirs(outdir, exist_ok=True)
    env = _create_env()
    files = []

    _associate_module_items(module)
    _inject_oz_lock(module, root_class)

    files.append(_render(env, "oz_dispatch.h.j2",
                         _dispatch_header_ctx(module), outdir, "oz_dispatch.h"))
    files.append(_render(env, "oz_dispatch.c.j2",
                         _dispatch_source_ctx(module), outdir, "oz_dispatch.c"))
    slabs_ctx = _mem_slabs_ctx(module, pool_sizes or {}, root_class,
                               item_pool_size)
    files.append(_render(env, "oz_mem_slabs.h.j2", slabs_ctx, outdir, "oz_mem_slabs.h"))
    files.append(_render(env, "oz_mem_slabs.c.j2", slabs_ctx, outdir, "oz_mem_slabs.c"))
    _has_item_pool = slabs_ctx["item_pool_count"] > 0

    # Group classes by source stem for per-file emission
    stem_groups: dict[str, list[OZClass]] = {}
    for cls in module.classes.values():
        if cls.name == "OZLock":
            continue
        stem = _header_stem(cls)
        stem_groups.setdefault(stem, []).append(cls)

    for stem, classes in stem_groups.items():
        header_tmpl = env.get_template("class_header.h.j2")
        source_tmpl = env.get_template("class_source.c.j2")
        has_source_stem = any(cls.source_stem for cls in classes)
        header_parts = []
        source_parts = []
        for cls in classes:
            ctx = _EmitCtx(cls=cls, module=module, root_class=root_class,
                           has_item_pool=_has_item_pool)
            header_parts.append(header_tmpl.render(**_class_header_ctx(ctx, stem)))
            source_parts.append(source_tmpl.render(**_class_source_ctx(ctx, stem)))
        if has_source_stem:
            # Inline: header content goes into the _ozm.c file directly
            source_path = os.path.join(outdir, f"{stem}_ozm.c")
            _write_file(source_path, "\n".join(header_parts) + "\n"
                        + "\n".join(source_parts))
            files.append(source_path)
        else:
            header_path = os.path.join(outdir, f"{stem}_ozh.h")
            source_path = os.path.join(outdir, f"{stem}_ozm.c")
            _write_file(header_path, "\n".join(header_parts))
            _write_file(source_path, "\n".join(source_parts))
            files.append(header_path)
            files.append(source_path)

    # Emit orphan sources (class-less .m files)
    for orphan in module.orphan_sources:
        ctx_dict = _orphan_source_ctx(orphan, module, root_class)
        files.append(_render(env, "orphan_source.c.j2",
                             ctx_dict, outdir, f"{orphan.stem}_ozm.c"))

    return files


# ---------------------------------------------------------------------------
# oz_dispatch.h
# ---------------------------------------------------------------------------

def _dispatch_header_ctx(module: OZModule) -> dict:
    """Build template context for oz_dispatch.h."""
    classes = sorted(module.classes.values(), key=lambda c: c.class_id)

    proto_sels_map: dict[str, OZMethod] = {}
    for cls in module.classes.values():
        for m in cls.methods:
            if (m.dispatch == DispatchKind.PROTOCOL
                    and not m.is_class_method
                    and m.selector not in proto_sels_map):
                proto_sels_map[m.selector] = m

    proto_sels = []
    for sel, m in sorted(proto_sels_map.items()):
        c_sel = _selector_to_c(sel)
        ret = m.return_type.c_type
        param_types = ", ".join(
            ["struct OZObject *"] + [p.oz_type.c_type for p in m.params]
        )
        call_args = ", ".join(
            ["(struct OZObject *)(obj)"] + [f"({p.name})" for p in m.params]
        )
        macro_params = ", ".join(["obj"] + [p.name for p in m.params])
        proto_sels.append({
            "c_sel": c_sel, "ret": ret, "param_types": param_types,
            "call_args": call_args, "macro_params": macro_params,
        })

    return {
        "classes": classes,
        "class_count": len(module.classes),
        "proto_sels": proto_sels,
    }


def _dispatch_source_ctx(module: OZModule) -> dict:
    """Build template context for oz_dispatch.c."""
    sorted_classes = sorted(module.classes.values(), key=lambda c: c.class_id)

    classes = []
    for cls in sorted_classes:
        super_id = (f"OZ_CLASS_{cls.superclass}"
                    if cls.superclass and cls.superclass in module.classes
                    else "OZ_CLASS_COUNT")
        classes.append({"name": cls.name, "super_id_expr": super_id,
                        "header_stem": _header_stem(cls),
                        "has_source_stem": bool(cls.source_stem)})

    # Collect unique protocol selectors (instance methods only)
    proto_sels_map: dict[str, OZMethod] = {}
    for cls in module.classes.values():
        for m in cls.methods:
            if (m.dispatch == DispatchKind.PROTOCOL
                    and not m.is_class_method
                    and m.selector not in proto_sels_map):
                proto_sels_map[m.selector] = m

    proto_sels = [{"c_sel": _selector_to_c(sel)}
                  for sel in sorted(proto_sels_map.keys())]

    # Build vtable entries and collect forward declarations for user classes
    vtable_entries = []
    fwd_decl_set: set[str] = set()
    fwd_decls: list[str] = []
    for sel_name in sorted(proto_sels_map.keys()):
        c_sel = _selector_to_c(sel_name)
        m = proto_sels_map[sel_name]
        for cls in sorted_classes:
            impl_cls = _find_implementing_class(cls, sel_name, module)
            if impl_cls:
                vtable_entries.append({
                    "c_sel": c_sel,
                    "cls_name": cls.name,
                    "impl_name": impl_cls.name,
                })
                if impl_cls.source_stem:
                    proto = _method_prototype(impl_cls, m)
                    if proto not in fwd_decl_set:
                        fwd_decl_set.add(proto)
                        fwd_decls.append(proto + ";")

    return {
        "classes": classes,
        "proto_sels": proto_sels,
        "vtable_entries": vtable_entries,
        "fwd_decls": fwd_decls,
        "has_oz_lock": "OZLock" in module.classes,
    }


def _flattened_sizeof(cls: OZClass, module: OZModule) -> str:
    """Build a sizeof(struct { ... }) expression with all fields flattened.

    Walks the inheritance chain from root to leaf, collecting every ivar.
    The anonymous struct mirrors the memory layout without requiring the
    actual struct definition, so oz_mem_slabs.h needs no class headers.
    """
    chain: list[OZClass] = []
    cur: OZClass | None = cls
    while cur:
        chain.append(cur)
        if cur.superclass and cur.superclass in module.classes:
            cur = module.classes[cur.superclass]
        else:
            break
    chain.reverse()
    fields: list[str] = []
    for c in chain:
        for iv in c.ivars:
            fields.append(f"{iv.oz_type.c_type} _{len(fields)}")
    if not fields:
        return "sizeof(int)"
    return "sizeof(struct { " + "; ".join(fields) + "; })"


def _has_auto_dealloc(cls: OZClass, module: OZModule) -> bool:
    """Check if a class will get an auto-generated dealloc method."""
    is_root = not cls.superclass or cls.superclass not in module.classes
    obj_ivars = [iv for iv in cls.ivars if iv.oz_type.is_object]
    return bool(obj_ivars) or not is_root


def _find_implementing_class(cls: OZClass, selector: str,
                             module: OZModule) -> OZClass | None:
    """Walk up the class hierarchy to find which class implements the selector."""
    # Auto-generated dealloc: class gets its own dealloc even without explicit one
    if selector == "dealloc" and _has_auto_dealloc(cls, module):
        return cls
    cur: OZClass | None = cls
    while cur:
        for m in cur.methods:
            if m.selector == selector:
                return cur
        if cur.superclass and cur.superclass in module.classes:
            cur = module.classes[cur.superclass]
        else:
            break
    return None


# ---------------------------------------------------------------------------
# oz_mem_slabs.h
# ---------------------------------------------------------------------------

def _mem_slabs_ctx(module: OZModule, pool_sizes: dict[str, int],
                   root_class: str,
                   item_pool_size: int | None = None) -> dict:
    """Build template context for oz_mem_slabs.h and oz_mem_slabs.c."""
    sorted_classes = sorted(module.classes.values(), key=lambda c: c.class_id)
    auto_counts = _count_alloc_calls(module)

    classes = []
    for cls in sorted_classes:
        classes.append({
            "name": cls.name,
            "base_chain": _base_chain(cls.name, module),
            "pool_count": pool_sizes.get(cls.name,
                                         max(auto_counts.get(cls.name, 0), 1)),
            "header_stem": _header_stem(cls),
            "sizeof_expr": _flattened_sizeof(cls, module),
            "has_source_stem": bool(cls.source_stem),
        })

    if item_pool_size is not None:
        ipc = item_pool_size
    else:
        ipc = _count_item_slots(module)

    number_inits = [
        {"suffix": "Int32", "c_type": "int32_t", "tag": "OZ_NUM_INT32",
         "field": "i32"},
        {"suffix": "Float", "c_type": "float", "tag": "OZ_NUM_FLOAT",
         "field": "f32"},
        {"suffix": "Int8", "c_type": "int8_t", "tag": "OZ_NUM_INT8",
         "field": "i8"},
    ]

    return {"classes": classes, "root_class": root_class,
            "item_pool_count": ipc, "number_inits": number_inits}


# ---------------------------------------------------------------------------
# Per-class .h
# ---------------------------------------------------------------------------

def _class_header_ctx(ctx: _EmitCtx, stem: str | None = None) -> dict:
    """Build template context for a class header file."""
    cls = ctx.cls
    module = ctx.module
    _root_builtins = {"_refcount", "oz_class_id"}
    is_root = not cls.superclass or cls.superclass not in module.classes

    user_ivars = []
    for ivar in cls.ivars:
        if is_root and ivar.name in _root_builtins:
            continue
        user_ivars.append({"c_type": ivar.oz_type.c_type, "name": ivar.name})

    _root_skip_sels = {"retain", "release", "retainCount",
                       "isEqual:", "cDescription:maxLength:"}
    method_prototypes = []
    has_dealloc = False
    for m in cls.methods:
        if is_root and m.selector in _root_skip_sels:
            continue
        if m.selector == "dealloc":
            has_dealloc = True
        method_prototypes.append(_method_prototype(cls, m))

    auto_dealloc_proto = False
    if not has_dealloc:
        obj_ivars = [iv for iv in cls.ivars if iv.oz_type.is_object]
        if obj_ivars or not is_root:
            auto_dealloc_proto = True

    # Collect type definitions needed by ivars (enum/union from stubs)
    ivar_type_defs = []
    for ivar in cls.ivars:
        qt = ivar.oz_type.raw_qual_type
        if qt in module.type_defs and module.type_defs[qt] not in ivar_type_defs:
            ivar_type_defs.append(module.type_defs[qt])

    function_protos = []
    for func in cls.functions:
        ret = func.return_type.c_type
        parts = [p.oz_type.c_param_decl(p.name) for p in func.params]
        params_str = ", ".join(parts) if parts else "void"
        function_protos.append(f"{ret} {func.name}({params_str});")

    extern_decls = []
    for sv in cls.statics:
        decl_str = sv.oz_type.c_param_decl(sv.name)
        extern_decls.append(f"extern {decl_str};")

    superclass_stem = None
    if cls.superclass and cls.superclass in module.classes:
        superclass_stem = _header_stem(module.classes[cls.superclass])

    return {
        "name": cls.name,
        "stem": stem or _header_stem(cls),
        "is_root": is_root,
        "inline_header": bool(cls.source_stem),
        "superclass": cls.superclass,
        "superclass_header": superclass_stem,
        "user_ivars": user_ivars,
        "ivar_type_defs": ivar_type_defs,
        "method_prototypes": method_prototypes,
        "auto_dealloc_proto": auto_dealloc_proto,
        "has_any_methods": bool(cls.methods) or is_root,
        "function_protos": function_protos,
        "extern_decls": extern_decls,
    }


# ---------------------------------------------------------------------------
# Synthesized property accessors
# ---------------------------------------------------------------------------

def _emit_synthesized_accessor(cls: OZClass, m: OZMethod,
                                out: StringIO,
                                root_class: str = "OZObject") -> None:
    """Emit a synthesized getter or setter for a property."""
    prop = m.synthesized_property
    ivar = prop.ivar_name
    is_getter = len(m.params) == 0
    is_atomic = not prop.is_nonatomic
    c_type = prop.oz_type.c_type
    is_strong_obj = prop.oz_type.is_object and prop.ownership == "strong"
    root = root_class

    if is_getter:
        if is_atomic:
            out.write("{\n")
            out.write("\toz_spinlock_t lck;\n")
            out.write(f"\t{c_type} val;\n")
            out.write("\tOZ_SPINLOCK(&lck) {\n")
            out.write(f"\t\tval = self->{ivar};\n")
            out.write("\t}\n")
            out.write("\treturn val;\n")
            out.write("}\n")
        else:
            out.write("{\n")
            out.write(f"\treturn self->{ivar};\n")
            out.write("}\n")
    else:
        param_name = m.params[0].name
        if is_strong_obj:
            if is_atomic:
                out.write("{\n")
                out.write("\toz_spinlock_t lck;\n")
                out.write(f"\t{c_type} old;\n")
                out.write(f"\t{root}_retain((struct {root} *){param_name});\n")
                out.write("\tOZ_SPINLOCK(&lck) {\n")
                out.write(f"\t\told = self->{ivar};\n")
                out.write(f"\t\tself->{ivar} = {param_name};\n")
                out.write("\t}\n")
                out.write(f"\t{root}_release((struct {root} *)old);\n")
                out.write("}\n")
            else:
                out.write("{\n")
                out.write(f"\t{c_type} old = self->{ivar};\n")
                out.write(f"\tself->{ivar} = {param_name};\n")
                out.write(f"\t{root}_retain((struct {root} *){param_name});\n")
                out.write(f"\t{root}_release((struct {root} *)old);\n")
                out.write("}\n")
        else:
            if is_atomic:
                out.write("{\n")
                out.write("\toz_spinlock_t lck;\n")
                out.write("\tOZ_SPINLOCK(&lck) {\n")
                out.write(f"\t\tself->{ivar} = {param_name};\n")
                out.write("\t}\n")
                out.write("}\n")
            else:
                out.write("{\n")
                out.write(f"\tself->{ivar} = {param_name};\n")
                out.write("}\n")


# ---------------------------------------------------------------------------
# Per-class .c
# ---------------------------------------------------------------------------

def _class_source_ctx(ctx: _EmitCtx, stem: str | None = None) -> dict:
    """Build template context for a class source file. Method bodies are pre-rendered."""
    cls = ctx.cls
    root_class = ctx.root_class
    is_root = cls.name == root_class

    root_retain_release = ""
    root_introspection = ""
    if is_root:
        buf = StringIO()
        _emit_root_retain_release(cls, ctx.module, buf)
        root_retain_release = buf.getvalue().rstrip("\n")
        buf = StringIO()
        _emit_root_introspection(cls, buf)
        root_introspection = buf.getvalue().rstrip("\n")

    _root_skip_sels = {"retain", "release", "retainCount",
                       "isEqual:", "cDescription:maxLength:"}
    method_bodies = []
    dealloc_body = ""
    has_user_dealloc = False
    for m in cls.methods:
        if m.selector in _root_skip_sels and is_root:
            continue
        if m.selector == "dealloc":
            has_user_dealloc = True
            buf = StringIO()
            _emit_user_dealloc(ctx, m, buf)
            dealloc_body = buf.getvalue().rstrip("\n")
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
        method_bodies.append(buf.getvalue().rstrip("\n"))

    if not has_user_dealloc:
        buf = StringIO()
        _emit_auto_dealloc(ctx, buf)
        val = buf.getvalue()
        if val:
            dealloc_body = val.rstrip("\n")

    function_bodies = []
    for func in cls.functions:
        ret = func.return_type.c_type
        parts = [p.oz_type.c_param_decl(p.name) for p in func.params]
        params_str = ", ".join(parts) if parts else "void"
        ctx.method = None
        ctx.scope_vars = []
        ctx.consumed_vars = set()
        ctx.loop_scope_depth = []
        ctx.pre_stmts = []
        ctx._tmp_counter = 0
        buf = StringIO()
        buf.write(f"{ret} {func.name}({params_str})\n")
        if func.body_ast:
            _emit_compound_stmt(func.body_ast, buf, ctx, indent=0,
                                param_retains=_object_params(func))
        else:
            buf.write("{\n}\n")
        function_bodies.append(buf.getvalue().rstrip("\n"))

    static_decls = []
    for sv in cls.statics:
        decl_str = sv.oz_type.c_param_decl(sv.name)
        init = f" = {sv.init_value}" if sv.init_value is not None else ""
        static_decls.append(f"{decl_str}{init};")

    return {
        "name": cls.name,
        "stem": stem or _header_stem(cls),
        "is_root": is_root,
        "inline_header": bool(cls.source_stem),
        "root_retain_release": root_retain_release,
        "root_introspection": root_introspection,
        "method_bodies": method_bodies,
        "dealloc_body": dealloc_body,
        "function_bodies": function_bodies,
        "static_decls": static_decls,
        "string_constants": ctx.string_constants,
        "block_functions": ctx.block_functions,
        "user_includes": cls.user_includes,
        "verbatim_lines": cls.verbatim_lines,
    }


def _orphan_source_ctx(orphan: OrphanSource, module: OZModule,
                       root_class: str) -> dict:
    """Build template context for an orphan source file (no class)."""
    function_bodies = []
    for func in orphan.functions:
        ret = func.return_type.c_type
        parts = [p.oz_type.c_param_decl(p.name) for p in func.params]
        params_str = ", ".join(parts) if parts else "void"
        buf = StringIO()
        buf.write(f"{ret} {func.name}({params_str})\n")
        if func.body_ast:
            # Create a minimal _EmitCtx for body emission
            dummy_cls = OZClass(name="__orphan__")
            ctx = _EmitCtx(cls=dummy_cls, module=module,
                           root_class=root_class)
            ctx.method = None
            ctx.scope_vars = []
            ctx.consumed_vars = set()
            ctx.loop_scope_depth = []
            ctx.pre_stmts = []
            ctx._tmp_counter = 0
            _emit_compound_stmt(func.body_ast, buf, ctx, indent=0,
                                param_retains=_object_params(func))
        else:
            buf.write("{\n}\n")
        function_bodies.append(buf.getvalue().rstrip("\n"))

    static_decls = []
    for sv in orphan.statics:
        decl_str = sv.oz_type.c_param_decl(sv.name)
        init = f" = {sv.init_value}" if sv.init_value is not None else ""
        static_decls.append(f"{decl_str}{init};")

    return {
        "stem": orphan.stem,
        "function_bodies": function_bodies,
        "static_decls": static_decls,
        "user_includes": orphan.user_includes,
        "verbatim_lines": orphan.verbatim_lines,
    }


# ---------------------------------------------------------------------------
# Root class retain/release
# ---------------------------------------------------------------------------

def _emit_root_retain_release(cls: OZClass, module: OZModule,
                              out: StringIO) -> None:
    out.write(f"struct {cls.name} *{cls.name}_retain(struct {cls.name} *self)\n")
    out.write("{\n")
    out.write("\tif (self) {\n")
    out.write(f"\t\toz_atomic_inc(&self->_refcount);\n")
    out.write("\t}\n")
    out.write("\treturn self;\n")
    out.write("}\n\n")

    out.write(f"void {cls.name}_release(struct {cls.name} *self)\n")
    out.write("{\n")
    out.write("\tif (!self) {\n")
    out.write("\t\treturn;\n")
    out.write("\t}\n")
    out.write(f"\tif (oz_atomic_dec_and_test(&self->_refcount)) {{\n")
    out.write(f"\t\tOZ_SEND_dealloc((struct {cls.name} *)self);\n")
    out.write("\t}\n")
    out.write("}\n\n")

    out.write(f"uint32_t {cls.name}_retainCount(struct {cls.name} *self)\n")
    out.write("{\n")
    out.write("\tif (!self) {\n")
    out.write("\t\treturn 0;\n")
    out.write("\t}\n")
    out.write(f"\treturn (uint32_t)oz_atomic_get(&self->_refcount);\n")
    out.write("}\n\n")


def _emit_root_introspection(cls: OZClass, out: StringIO) -> None:
    """Emit isEqual: and cDescription:maxLength: for root class."""
    out.write(f"BOOL {cls.name}_isEqual_("
              f"struct {cls.name} *self, struct {cls.name} *anObject)\n")
    out.write("{\n")
    out.write("\treturn self == anObject;\n")
    out.write("}\n\n")

    out.write(f"int {cls.name}_cDescription_maxLength_("
              f"struct {cls.name} *self, char *buf, int maxLen)\n")
    out.write("{\n")
    out.write("\treturn oz_platform_snprint(buf, (size_t)maxLen, \"<%s: %p>\",\n")
    out.write("\t\toz_class_names[self->oz_class_id], (void *)self);\n")
    out.write("}\n\n")


# ---------------------------------------------------------------------------
# Body AST -> C
# ---------------------------------------------------------------------------

def _emit_compound_stmt(node: dict, out: StringIO, ctx: _EmitCtx,
                        indent: int = 0, inline: bool = False,
                        param_retains: list[OZParam] | None = None) -> None:
    if inline:
        out.write("{\n")
    else:
        out.write("\t" * indent + "{\n")

    # Push scope frame
    ctx.scope_vars.append({})

    # Retain and track object params at method/function entry
    if param_retains:
        tabs_inner = "\t" * (indent + 1)
        for p in param_retains:
            out.write(f"{tabs_inner}{ctx.root_class}_retain("
                      f"(struct {ctx.root_class} *){p.name});\n")
            ctx.scope_vars[-1][p.name] = p.oz_type

    children = node.get("inner", [])
    for i, child in enumerate(children):
        _emit_stmt(child, out, ctx, indent + 1)

    # Release locals at scope exit (skip if last stmt is a return — it handles cleanup)
    last_kind = children[-1].get("kind", "") if children else ""
    if last_kind != "ReturnStmt":
        _emit_scope_releases(out, ctx, indent + 1)

    # Pop scope frame
    ctx.scope_vars.pop()

    out.write("\t" * indent + "}\n")


def _emit_synchronized_stmt(node: dict, out: StringIO, ctx: _EmitCtx,
                            indent: int) -> None:
    """Emit @synchronized(obj) { ... } as OZLock RAII block."""
    inner = node.get("inner", [])
    if len(inner) < 2:
        return

    obj_expr = inner[0]
    body = inner[1]

    ctx._sync_counter += 1
    sync_name = "_sync" if ctx._sync_counter == 1 else f"_sync{ctx._sync_counter}"
    tabs = "\t" * indent
    tabs1 = "\t" * (indent + 1)

    obj_buf = StringIO()
    _emit_expr(obj_expr, obj_buf, ctx)
    _flush_pre_stmts(out, ctx, indent)

    out.write(f"{tabs}{{\n")

    ctx.scope_vars.append({})

    out.write(f"{tabs1}struct OZLock *{sync_name} = OZLock_initWithObject("
              f"OZLock_alloc(), (struct {ctx.root_class} *){obj_buf.getvalue()});\n")
    ctx.scope_vars[-1][sync_name] = OZType("OZLock *")

    if body.get("kind") == "CompoundStmt":
        for child in body.get("inner", []):
            _emit_stmt(child, out, ctx, indent + 1)

    last_kind = ""
    body_inner = body.get("inner", [])
    if body_inner:
        last_kind = body_inner[-1].get("kind", "")
    if last_kind != "ReturnStmt":
        _emit_scope_releases(out, ctx, indent + 1)

    ctx.scope_vars.pop()

    out.write(f"{tabs}}}\n")


def _flush_pre_stmts(out: StringIO, ctx: _EmitCtx, indent: int) -> None:
    """Flush any pre-statement temp var declarations."""
    tabs = "\t" * indent
    for stmt in ctx.pre_stmts:
        out.write(f"{tabs}{stmt}")
    ctx.pre_stmts.clear()


def _emit_stmt(node: dict, out: StringIO, ctx: _EmitCtx,
               indent: int) -> None:
    tabs = "\t" * indent
    kind = node.get("kind", "")

    if kind == "ReturnStmt":
        _emit_return_stmt(node, out, ctx, indent)

    elif kind == "DeclStmt":
        for decl in node.get("inner", []):
            if decl.get("kind") == "VarDecl":
                _emit_var_decl(decl, out, ctx, indent)

    elif kind == "IfStmt":
        _emit_if_stmt(node, out, ctx, indent)

    elif kind == "ForStmt":
        _emit_for_stmt(node, out, ctx, indent)

    elif kind == "WhileStmt":
        inner = node.get("inner", [])
        if len(inner) >= 2:
            cond_buf = StringIO()
            _emit_expr(inner[0], cond_buf, ctx)
            _flush_pre_stmts(out, ctx, indent)
            out.write(f"{tabs}while ({cond_buf.getvalue()}) ")
            ctx.loop_scope_depth.append(len(ctx.scope_vars))
            if inner[1].get("kind") == "CompoundStmt":
                _emit_compound_stmt(inner[1], out, ctx, indent, inline=True)
            else:
                out.write("{\n")
                _emit_stmt(inner[1], out, ctx, indent + 1)
                out.write(f"{tabs}}}\n")
            ctx.loop_scope_depth.pop()

    elif kind == "DoStmt":
        inner = node.get("inner", [])
        if len(inner) >= 2:
            out.write(f"{tabs}do ")
            ctx.loop_scope_depth.append(len(ctx.scope_vars))
            if inner[0].get("kind") == "CompoundStmt":
                _emit_compound_stmt(inner[0], out, ctx, indent, inline=True)
            else:
                out.write("{\n")
                _emit_stmt(inner[0], out, ctx, indent + 1)
                out.write(f"{tabs}}}\n")
            ctx.loop_scope_depth.pop()
            out.write(f"{tabs}while (")
            _emit_expr(inner[1], out, ctx)
            out.write(");\n")

    elif kind == "CompoundStmt":
        _emit_compound_stmt(node, out, ctx, indent)

    elif kind == "ObjCAutoreleasePoolStmt":
        inner = node.get("inner", [])
        if inner and inner[0].get("kind") == "CompoundStmt":
            _emit_compound_stmt(inner[0], out, ctx, indent)

    elif kind == "ObjCAtSynchronizedStmt":
        _emit_synchronized_stmt(node, out, ctx, indent)

    elif kind == "ObjCForCollectionStmt":
        _emit_forin_stmt(node, out, ctx, indent)

    elif kind == "NullStmt":
        out.write(f"{tabs};\n")

    elif kind == "BreakStmt":
        _emit_break_continue_releases(out, ctx, indent)
        out.write(f"{tabs}break;\n")

    elif kind == "ContinueStmt":
        _emit_break_continue_releases(out, ctx, indent)
        out.write(f"{tabs}continue;\n")

    elif kind == "CompoundAssignOperator":
        expr_buf = StringIO()
        _emit_expr(node, expr_buf, ctx)
        _flush_pre_stmts(out, ctx, indent)
        out.write(f"{tabs}{expr_buf.getvalue()};\n")

    elif kind == "BinaryOperator" and node.get("opcode") == "=":
        # Check for strong ivar or local assignment
        inner = node.get("inner", [])
        if len(inner) == 2 and _is_object_ivar_assign(inner[0], ctx):
            _emit_strong_ivar_assign(node, out, ctx, indent)
        elif len(inner) == 2 and _is_object_local_assign(inner[0], ctx):
            _emit_strong_local_assign(node, out, ctx, indent)
        else:
            expr_buf = StringIO()
            _emit_expr(node, expr_buf, ctx)
            _flush_pre_stmts(out, ctx, indent)
            out.write(f"{tabs}{expr_buf.getvalue()};\n")

    else:
        # Expression statement
        expr_buf = StringIO()
        _emit_expr(node, expr_buf, ctx)
        _flush_pre_stmts(out, ctx, indent)
        out.write(f"{tabs}{expr_buf.getvalue()};\n")


def _emit_var_decl(node: dict, out: StringIO, ctx: _EmitCtx,
                   indent: int) -> None:
    tabs = "\t" * indent
    qt = node.get("type", {}).get("qualType", "int")
    oz_type = OZType(qt)
    name = node.get("name", "_anon")

    # __block vars are promoted to file-scope statics — skip local emission
    if any(c.get("kind") == "BlocksAttr" for c in node.get("inner", [])):
        return
    c_type = oz_type.c_type

    # Track object locals for scope-exit release (never track self)
    if oz_type.is_object and name != "self" and ctx.scope_vars:
        ctx.scope_vars[-1][name] = oz_type

    inner = node.get("inner", [])
    init_expr = None
    for child in inner:
        if child.get("kind") not in ("FullComment",):
            init_expr = child
            break

    decl_str = oz_type.c_param_decl(name)

    if init_expr:
        expr_buf = StringIO()
        _emit_expr(init_expr, expr_buf, ctx)
        _flush_pre_stmts(out, ctx, indent)
        out.write(f"{tabs}{decl_str} = {expr_buf.getvalue()};\n")
        # Retain borrowed (+0) references tracked as scope vars
        if (oz_type.is_object and name != "self" and ctx.scope_vars
                and name in ctx.scope_vars[-1]
                and _is_borrowed_object_expr(init_expr)):
            root = ctx.root_class
            out.write(f"{tabs}{root}_retain((struct {root} *){name});\n")
    else:
        out.write(f"{tabs}{decl_str};\n")


def _emit_if_stmt(node: dict, out: StringIO, ctx: _EmitCtx,
                  indent: int) -> None:
    tabs = "\t" * indent
    inner = node.get("inner", [])
    if not inner:
        return

    has_else = node.get("hasElse", False)

    # Clang AST: inner[0]=cond, inner[1]=then, [inner[2]=else]
    cond = inner[0]
    then_body = inner[1] if len(inner) > 1 else None
    else_body = inner[2] if has_else and len(inner) > 2 else None

    cond_buf = StringIO()
    _emit_expr(cond, cond_buf, ctx)
    _flush_pre_stmts(out, ctx, indent)
    out.write(f"{tabs}if ({cond_buf.getvalue()}) ")
    if then_body:
        if then_body.get("kind") == "CompoundStmt":
            _emit_compound_stmt(then_body, out, ctx, indent, inline=True)
        else:
            out.write("{\n")
            _emit_stmt(then_body, out, ctx, indent + 1)
            out.write(f"{tabs}}}\n")

    if else_body:
        if else_body.get("kind") == "IfStmt":
            out.write(f"{tabs}else ")
            _emit_if_stmt(else_body, out, ctx, indent)
        elif else_body.get("kind") == "CompoundStmt":
            out.write(f"{tabs}else ")
            _emit_compound_stmt(else_body, out, ctx, indent, inline=True)
        else:
            out.write(f"{tabs}else {{\n")
            _emit_stmt(else_body, out, ctx, indent + 1)
            out.write(f"{tabs}}}\n")


def _emit_for_stmt(node: dict, out: StringIO, ctx: _EmitCtx,
                   indent: int) -> None:
    tabs = "\t" * indent
    inner = node.get("inner", [])
    if len(inner) < 2:
        return

    # Pre-emit init, cond, and inc into buffers to capture pre_stmts
    # (receiver temps for protocol dispatch) before the for statement.
    init_buf = StringIO()
    if len(inner) > 0 and inner[0].get("kind") not in ("NullStmt", "<<<NULL>>>"):
        if inner[0].get("kind") == "DeclStmt":
            decls = inner[0].get("inner", [])
            if decls and decls[0].get("kind") == "VarDecl":
                vd = decls[0]
                qt = vd.get("type", {}).get("qualType", "int")
                vname = vd.get("name", "i")
                c_type = OZType(qt).c_type
                vinit = [c for c in vd.get("inner", []) if c.get("kind") != "FullComment"]
                init_buf.write(f"{c_type} {vname}")
                if vinit:
                    init_buf.write(" = ")
                    _emit_expr(vinit[0], init_buf, ctx)
        else:
            _emit_expr(inner[0], init_buf, ctx)

    cond_idx = 2 if len(inner) > 4 else 1
    cond_buf = StringIO()
    if cond_idx < len(inner) and inner[cond_idx].get("kind") not in ("NullStmt", "<<<NULL>>>"):
        _emit_expr(inner[cond_idx], cond_buf, ctx)

    inc_idx = 3 if len(inner) > 4 else 2
    inc_buf = StringIO()
    if inc_idx < len(inner) and inner[inc_idx].get("kind") not in ("NullStmt", "<<<NULL>>>"):
        _emit_expr(inner[inc_idx], inc_buf, ctx)

    # Flush all receiver temps before the for statement
    _flush_pre_stmts(out, ctx, indent)

    out.write(f"{tabs}for (")
    out.write(init_buf.getvalue())
    out.write("; ")
    out.write(cond_buf.getvalue())
    out.write("; ")
    out.write(inc_buf.getvalue())
    out.write(") ")

    # body
    body_idx = 4 if len(inner) > 4 else len(inner) - 1
    body = inner[body_idx] if body_idx < len(inner) else None
    ctx.loop_scope_depth.append(len(ctx.scope_vars))
    if body and body.get("kind") == "CompoundStmt":
        _emit_compound_stmt(body, out, ctx, indent, inline=True)
    elif body:
        out.write("{\n")
        _emit_stmt(body, out, ctx, indent + 1)
        out.write(f"{tabs}}}\n")
    else:
        out.write("{}\n")
    ctx.loop_scope_depth.pop()


def _emit_forin_stmt(node: dict, out: StringIO, ctx: _EmitCtx,
                     indent: int) -> None:
    """Lower ObjCForCollectionStmt to scoped iterator-based for loop.

    for (id obj in [sensor samples]) { body }
    =>
    {
        struct OZObject *_oz_iterN = (struct OZObject *)OZ_SEND_iter(collection);
        struct OZObject *_oz_recvN = _oz_iterN;
        for (struct OZObject *obj = OZ_SEND_next(_oz_recvN); obj != NULL;
             obj = OZ_SEND_next(_oz_recvN)) { body }
    }
    """
    tabs = "\t" * indent
    inner = node.get("inner", [])
    if len(inner) < 3:
        return

    decl_stmt = inner[0]
    collection = inner[1]
    body = inner[2]

    vd = decl_stmt.get("inner", [{}])[0]
    var_name = vd.get("name", "obj")
    qt = vd.get("type", {}).get("qualType", "id")
    c_type = OZType(qt).c_type

    coll_buf = StringIO()
    _emit_expr(collection, coll_buf, ctx)
    _flush_pre_stmts(out, ctx, indent)

    iter_tmp = f"_oz_iter{ctx._tmp_counter}"
    ctx._tmp_counter += 1
    next_recv = f"_oz_recv{ctx._tmp_counter}"
    ctx._tmp_counter += 1

    itabs = "\t" * (indent + 1)
    out.write(f"{tabs}{{\n")
    out.write(f"{itabs}struct OZObject *{iter_tmp} = "
              f"(struct OZObject *)OZ_SEND_iter("
              f"(struct OZObject *){coll_buf.getvalue()});\n")
    out.write(f"{itabs}struct OZObject *{next_recv} = {iter_tmp};\n")
    next_call = f"OZ_SEND_next({next_recv})"
    if c_type != "struct OZObject *":
        next_call = f"({c_type}){next_call}"
    out.write(f"{itabs}for ({c_type} {var_name} = {next_call}; "
              f"{var_name} != ((void *)0); "
              f"{var_name} = {next_call}) ")

    ctx.scope_vars.append({var_name: OZType(qt)})
    ctx.loop_scope_depth.append(len(ctx.scope_vars))
    if body.get("kind") == "CompoundStmt":
        _emit_compound_stmt(body, out, ctx, indent + 1, inline=True)
    else:
        out.write("{\n")
        _emit_stmt(body, out, ctx, indent + 2)
        out.write(f"{itabs}}}\n")
    ctx.loop_scope_depth.pop()
    ctx.scope_vars.pop()
    out.write(f"{tabs}}}\n")


def _emit_boxed_number(node: dict, out: StringIO, ctx: _EmitCtx) -> None:
    """Emit a dynamically allocated OZNumber via OZNumber_initXxx() helper."""
    inner = node.get("inner", [])
    if not inner:
        out.write("/* TODO: empty ObjCBoxedExpr */")
        return

    child = inner[0]
    child_kind = child.get("kind", "")

    # Unwrap ImplicitCastExpr wrappers
    while child_kind == "ImplicitCastExpr":
        child = child.get("inner", [{}])[0]
        child_kind = child.get("kind", "")

    if child_kind == "IntegerLiteral":
        val = child.get("value", "0")
        out.write(f"OZNumber_initInt32({val})")
    elif child_kind == "FloatingLiteral":
        val = child.get("value", "0.0")
        fval = val if val.endswith("f") or val.endswith("F") else f"{val}f"
        out.write(f"OZNumber_initFloat({fval})")
    elif child_kind == "CharacterLiteral":
        val = str(child.get("value", 0))
        out.write(f"OZNumber_initInt32({val})")
    elif child_kind == "ObjCBoolLiteralExpr":
        raw = child.get("value", False)
        if isinstance(raw, str):
            val = "0" if "no" in raw.lower() else "1"
        else:
            val = "1" if raw else "0"
        out.write(f"OZNumber_initInt8({val})")
    else:
        out.write(f"/* TODO: ObjCBoxedExpr({child_kind}) */")
        return


def _emit_block_expr(node: dict, out: StringIO, ctx: _EmitCtx) -> None:
    """Emit a non-capturing block as a static C function."""
    inner = node.get("inner", [])
    if not inner:
        out.write("/* TODO: empty BlockExpr */")
        return

    block_decl = inner[0]
    if block_decl.get("kind") != "BlockDecl":
        out.write("/* TODO: BlockExpr without BlockDecl */")
        return

    block_inner = block_decl.get("inner", [])

    # Check for captures — allow __block (byref), reject others
    for child in block_inner:
        if child.get("kind") == "Capture":
            var_name = child.get("var", {}).get("name", "?")
            if child.get("byref"):
                # __block var → already promoted to file-scope static
                continue
            ctx.module.diagnostics.append(
                f"error: capturing blocks not supported "
                f"(block captures '{var_name}'). Use a regular loop instead."
            )
            out.write(f"/* ERROR: capturing block (captures '{var_name}') */")
            return

    # Extract params and body
    params = []
    body_ast = None
    for child in block_inner:
        ckind = child.get("kind", "")
        if ckind == "ParmVarDecl":
            pname = child.get("name", "")
            ptype = OZType(child.get("type", {}).get("qualType", ""))
            params.append(OZParam(pname, ptype))
        elif ckind == "CompoundStmt":
            body_ast = child

    # Determine return type (default void)
    block_qt = node.get("type", {}).get("qualType", "")
    ret_type = "void"
    if block_qt:
        # Parse "void (^)(params)" or "int (^)(params)" — return type is before (^)
        paren_idx = block_qt.find("(^)")
        if paren_idx > 0:
            ret_type = OZType(block_qt[:paren_idx].strip()).c_type
        elif paren_idx < 0:
            # Might be just a function type
            paren_idx = block_qt.find("(")
            if paren_idx > 0:
                ret_type = OZType(block_qt[:paren_idx].strip()).c_type

    # Generate function name
    func_name = f"_oz_block_{ctx._tmp_counter}"
    ctx._tmp_counter += 1

    # Build param string
    param_parts = []
    for p in params:
        param_parts.append(f"{p.oz_type.c_type} {p.name}")
    params_str = ", ".join(param_parts) if param_parts else "void"

    # Render the function body
    buf = StringIO()
    buf.write(f"static {ret_type} {func_name}({params_str})\n")
    if body_ast:
        # Create a minimal context for the block body
        block_ctx = _EmitCtx(
            cls=ctx.cls,
            module=ctx.module,
            root_class=ctx.root_class,
            string_constants=ctx.string_constants,
            block_functions=ctx.block_functions,
            has_item_pool=ctx.has_item_pool,
            _tmp_counter=ctx._tmp_counter,
            _string_dedup=ctx._string_dedup,
        )
        _emit_compound_stmt(body_ast, buf, block_ctx, indent=0)
        ctx._tmp_counter = block_ctx._tmp_counter
    else:
        buf.write("{\n}\n")

    ctx.block_functions.append(buf.getvalue().rstrip("\n"))

    # Emit function name as the expression value
    out.write(func_name)


def _emit_expr(node: dict, out: StringIO, ctx: _EmitCtx) -> None:
    kind = node.get("kind", "")

    if kind == "ExprWithCleanups":
        inner = node.get("inner", [])
        if inner:
            _emit_expr(inner[0], out, ctx)
        return

    if kind == "BlockExpr":
        _emit_block_expr(node, out, ctx)
        return

    if kind == "ImplicitCastExpr":
        inner = node.get("inner", [])
        if inner:
            cast_kind = node.get("castKind", "")
            target_qt = node.get("type", {}).get("qualType", "")
            target_oz = OZType(target_qt)
            if cast_kind == "BitCast" and target_oz.is_object:
                out.write(f"({target_oz.c_type})")
            _emit_expr(inner[0], out, ctx)
        return

    if kind == "CStyleCastExpr":
        if node.get("castKind") == "NullToPointer":
            out.write("((void *)0)")
            return
        qt = node.get("type", {}).get("qualType", "")
        out.write(f"({OZType(qt).c_type})")
        inner = node.get("inner", [])
        if inner:
            _emit_expr(inner[0], out, ctx)
        return

    if kind == "ParenExpr":
        out.write("(")
        inner = node.get("inner", [])
        if inner:
            _emit_expr(inner[0], out, ctx)
        out.write(")")
        return

    if kind == "ObjCMessageExpr":
        _emit_msg_expr(node, out, ctx)
        return

    if kind == "ObjCIvarRefExpr":
        ivar_name = node.get("decl", {}).get("name", "")
        inner = node.get("inner", [])
        # Non-free ivar with an explicit base (e.g. other->_length)
        if not node.get("isFreeIvar", False) and inner:
            base = inner[0]
            # Skip LValueToRValue casts to get to the actual DeclRefExpr
            while (base.get("kind") == "ImplicitCastExpr"
                   and base.get("castKind") == "LValueToRValue"):
                base = base.get("inner", [{}])[0]
            ref_name = base.get("referencedDecl", {}).get("name", "")
            if ref_name and ref_name != "self":
                _emit_expr(inner[0], out, ctx)
                out.write(f"->{ivar_name}")
                return
        out.write(f"self->{ivar_name}")
        return

    if kind == "DeclRefExpr":
        name = node.get("referencedDecl", {}).get("name", "")
        out.write(name)
        return

    if kind == "MemberExpr":
        inner = node.get("inner", [])
        member = node.get("name", "")
        is_arrow = node.get("isArrow", False)
        if inner:
            _emit_expr(inner[0], out, ctx)
        out.write(f"->{member}" if is_arrow else f".{member}")
        return

    if kind == "BinaryOperator":
        inner = node.get("inner", [])
        op = node.get("opcode", "")
        if len(inner) == 2:
            _emit_expr(inner[0], out, ctx)
            out.write(f" {op} ")
            _emit_expr(inner[1], out, ctx)
        return

    if kind == "CompoundAssignOperator":
        inner = node.get("inner", [])
        op = node.get("opcode", "")
        if len(inner) == 2:
            _emit_expr(inner[0], out, ctx)
            out.write(f" {op} ")
            _emit_expr(inner[1], out, ctx)
        return

    if kind == "UnaryOperator":
        inner = node.get("inner", [])
        op = node.get("opcode", "")
        is_postfix = node.get("isPostfix", False)
        if inner:
            if is_postfix:
                _emit_expr(inner[0], out, ctx)
                out.write(op)
            else:
                out.write(op)
                _emit_expr(inner[0], out, ctx)
        return

    if kind == "ConditionalOperator":
        inner = node.get("inner", [])
        if len(inner) == 3:
            _emit_expr(inner[0], out, ctx)
            out.write(" ? ")
            _emit_expr(inner[1], out, ctx)
            out.write(" : ")
            _emit_expr(inner[2], out, ctx)
        return

    if kind == "IntegerLiteral":
        out.write(node.get("value", "0"))
        return

    if kind == "FloatingLiteral":
        out.write(node.get("value", "0.0"))
        return

    if kind == "CharacterLiteral":
        val = node.get("value", 0)
        if 32 <= val < 127 and val != ord("'") and val != ord("\\"):
            out.write(f"'{chr(val)}'")
        else:
            out.write(str(val))
        return

    if kind == "StringLiteral":
        out.write(node.get("value", '""'))
        return

    if kind == "ObjCStringLiteral":
        inner = node.get("inner", [])
        val = inner[0].get("value", '""') if inner else '""'
        if val in ctx._string_dedup:
            name = ctx._string_dedup[val]
        else:
            raw = val[1:-1]  # strip surrounding quotes
            name = f"_oz_str_{ctx._tmp_counter}"
            ctx._tmp_counter += 1
            ctx._string_dedup[val] = name
            ctx.string_constants.append(
                f"static struct OZString {name} = {{"
                f"{{OZ_CLASS_OZString, 2147483647}}, "
                f"{len(raw)}, 0, {val}}};"
            )
        out.write(f"(struct OZString *)&{name}")
        return

    if kind == "ObjCBoolLiteralExpr":
        val = node.get("value", False)
        if isinstance(val, str):
            out.write("0" if "no" in val.lower() else "1")
        else:
            out.write("1" if val else "0")
        return

    if kind == "ObjCBoxedExpr":
        _emit_boxed_number(node, out, ctx)
        return

    if kind == "ObjCArrayLiteral":
        inner = node.get("inner", [])
        buf_name = f"_oz_arr_{ctx._tmp_counter}_buf"
        ctx._tmp_counter += 1
        root = ctx.root_class
        elem_refs = []
        for child in inner:
            buf = StringIO()
            _emit_expr(child, buf, ctx)
            ref = buf.getvalue()
            if _is_fresh_alloc(child):
                elem_refs.append(f"(struct {root} *){ref}")
            else:
                elem_refs.append(
                    f"(struct {root} *){root}_retain("
                    f"(struct {root} *){ref})")
        count = len(elem_refs)
        ctx.pre_stmts.append(
            f"struct {root} *{buf_name}[] = {{"
            + ", ".join(elem_refs) + "};\n"
        )
        out.write(f"OZArray_initWithItems({buf_name}, {count})")
        return

    if kind == "ObjCDictionaryLiteral":
        inner = node.get("inner", [])
        buf_name = f"_oz_dict_{ctx._tmp_counter}_kv"
        ctx._tmp_counter += 1
        root = ctx.root_class
        key_refs = []
        val_refs = []
        for i in range(0, len(inner), 2):
            kbuf = StringIO()
            _emit_expr(inner[i], kbuf, ctx)
            kref = kbuf.getvalue()
            if _is_fresh_alloc(inner[i]):
                key_refs.append(f"(struct {root} *){kref}")
            else:
                key_refs.append(
                    f"(struct {root} *){root}_retain("
                    f"(struct {root} *){kref})")
            vbuf = StringIO()
            _emit_expr(inner[i + 1], vbuf, ctx)
            vref = vbuf.getvalue()
            if _is_fresh_alloc(inner[i + 1]):
                val_refs.append(f"(struct {root} *){vref}")
            else:
                val_refs.append(
                    f"(struct {root} *){root}_retain("
                    f"(struct {root} *){vref})")
        count = len(key_refs)
        all_refs = key_refs + val_refs
        ctx.pre_stmts.append(
            f"struct {root} *{buf_name}[] = {{"
            + ", ".join(all_refs) + "};\n"
        )
        out.write(f"OZDictionary_initWithKeysValues({buf_name}, {count})")
        return

    if kind == "GNUNullExpr" or kind == "CXXNullPtrLiteralExpr":
        out.write("((void *)0)")
        return

    if kind == "PseudoObjectExpr":
        # ObjC subscript: inner[0] is ObjCSubscriptRefExpr (syntactic),
        # last ObjCMessageExpr child is the lowered call.
        inner = node.get("inner", [])
        if inner:
            msg = None
            for child in reversed(inner):
                if child.get("kind") == "ObjCMessageExpr":
                    msg = child
                    break
            _emit_expr(msg if msg else inner[0], out, ctx)
        return

    if kind == "OpaqueValueExpr":
        inner = node.get("inner", [])
        if inner:
            _emit_expr(inner[0], out, ctx)
        return

    if kind == "CallExpr":
        inner = node.get("inner", [])
        if inner:
            _emit_expr(inner[0], out, ctx)
            out.write("(")
            args = inner[1:]
            for i, arg in enumerate(args):
                if i > 0:
                    out.write(", ")
                _emit_expr(arg, out, ctx)
            out.write(")")
        return

    if kind == "ArraySubscriptExpr":
        inner = node.get("inner", [])
        if len(inner) == 2:
            _emit_expr(inner[0], out, ctx)
            out.write("[")
            _emit_expr(inner[1], out, ctx)
            out.write("]")
        return

    if kind == "CompoundLiteralExpr":
        qt = node.get("type", {}).get("qualType", "")
        out.write(f"({qt})")
        inner = node.get("inner", [])
        if inner:
            _emit_expr(inner[0], out, ctx)
        return

    if kind == "InitListExpr":
        inner = node.get("inner", [])
        out.write("{")
        for i, child in enumerate(inner):
            if i > 0:
                out.write(", ")
            _emit_expr(child, out, ctx)
        out.write("}")
        return

    # Fallback: try inner children or emit placeholder
    inner = node.get("inner", [])
    if inner:
        _emit_expr(inner[0], out, ctx)
    else:
        out.write(f"/* TODO: {kind} */")


def _emit_msg_expr(node: dict, out: StringIO, ctx: _EmitCtx) -> None:
    cls = ctx.cls
    module = ctx.module
    root_class = ctx.root_class
    selector = node.get("selector", "")
    c_sel = _selector_to_c(selector)
    receiver_kind = node.get("receiverKind", "")
    inner = node.get("inner", [])

    # [super sel] -> ParentClass_sel((struct ParentClass *)self)
    if receiver_kind.startswith("super"):
        parent = cls.superclass or root_class
        ret_type = node.get("type", {}).get("qualType", "void")
        needs_cast = OZType(ret_type).is_object
        if needs_cast:
            out.write(f"(struct {cls.name} *)")
        out.write(f"{parent}_{c_sel}(")
        out.write(f"(struct {parent} *)self")
        for arg in inner:
            out.write(", ")
            _emit_expr(arg, out, ctx)
        out.write(")")
        return

    # [ClassName sel] -> ClassName_sel() or ClassName_cls_sel() for class methods
    if receiver_kind == "class":
        class_type = node.get("classType", {}).get("qualType", "")
        prefix = ""
        cls_obj = module.classes.get(class_type)
        if cls_obj:
            for m in cls_obj.methods:
                if m.selector == selector and m.is_class_method:
                    prefix = "cls_"
                    break
        out.write(f"{class_type}_{prefix}{c_sel}(")
        for i, arg in enumerate(inner):
            if i > 0:
                out.write(", ")
            _emit_expr(arg, out, ctx)
        out.write(")")
        return

    # [obj sel] -> check dispatch kind
    args_exprs = _collect_msg_args(inner)
    receiver = inner[0] if inner else None

    dispatch = _find_dispatch_kind(selector, module)
    if dispatch == DispatchKind.PROTOCOL:
        # Protocol vtable returns root class pointer; cast if the method
        # returns an object type and the receiver is a subclass
        ret_qt = node.get("type", {}).get("qualType", "void")
        ret_oz = OZType(ret_qt)
        recv_class = _infer_receiver_class(receiver, cls, module) if receiver else cls.name
        needs_cast = ret_oz.is_object and recv_class != root_class
        if needs_cast:
            out.write(f"(struct {recv_class} *)")
        # Emit receiver into a temp var to avoid double evaluation
        # in the OZ_SEND macro
        if receiver:
            recv_buf = StringIO()
            _emit_expr(receiver, recv_buf, ctx)
            recv_str = recv_buf.getvalue()
            tmp = f"_oz_recv{ctx._tmp_counter}"
            ctx._tmp_counter += 1
            ctx.pre_stmts.append(
                f"struct {root_class} *{tmp} = "
                f"(struct {root_class} *){recv_str};\n"
            )
            out.write(f"OZ_SEND_{c_sel}({tmp}")
        else:
            out.write(f"OZ_SEND_{c_sel}(")
        for arg in args_exprs:
            out.write(", ")
            _emit_expr(arg, out, ctx)
        out.write(")")
    else:
        recv_class = _infer_receiver_class(receiver, cls, module) if receiver else cls.name
        # Walk up hierarchy to find class that actually defines the selector
        recv_class = _find_defining_class(recv_class, selector, module)
        out.write(f"{recv_class}_{c_sel}(")
        if receiver:
            _emit_expr(receiver, out, ctx)
        for arg in args_exprs:
            out.write(", ")
            _emit_expr(arg, out, ctx)
        out.write(")")


_ROOT_INTROSPECTION_SELS = {"isEqual:", "cDescription:maxLength:",
                            "retain", "release", "retainCount"}


def _find_defining_class(class_name: str, selector: str,
                         module: OZModule) -> str:
    """Find the class that defines a selector, walking up the hierarchy.

    Also checks root introspection methods (isEqual:, cDescription:maxLength:,
    retain, release, retainCount) which are emitted separately from the normal
    method list.
    """
    name = class_name
    while name:
        cls_obj = module.classes.get(name)
        if not cls_obj:
            break
        for m in cls_obj.methods:
            if m.selector == selector:
                return name
        # Root introspection methods are skipped from cls.methods during emit
        # but are defined on the root class
        if not cls_obj.superclass and selector in _ROOT_INTROSPECTION_SELS:
            return name
        name = cls_obj.superclass
    return class_name


def _collect_msg_args(inner: list[dict]) -> list[dict]:
    """Extract argument expressions from ObjCMessageExpr inner nodes.

    inner[0] is the receiver, the rest are arguments.
    """
    if len(inner) <= 1:
        return []
    return inner[1:]


def _find_dispatch_kind(selector: str, module: OZModule) -> DispatchKind:
    for cls in module.classes.values():
        for m in cls.methods:
            if m.selector == selector:
                return m.dispatch
    return DispatchKind.STATIC


def _infer_receiver_class(node: dict, cls: OZClass, module: OZModule | None = None) -> str:
    """Best-effort receiver class from AST type info."""
    qt = node.get("type", {}).get("qualType", "")
    oz = OZType(qt)
    if oz.is_object:
        name = oz._strip_qualifiers().rstrip(" *")
        if name and name not in ("id", "instancetype"):
            return name

    # If the receiver is a message expr, try to infer from classType or receiver chain
    unwrapped = node
    while unwrapped.get("kind") in ("ImplicitCastExpr", "ParenExpr"):
        inner = unwrapped.get("inner", [])
        if inner:
            unwrapped = inner[0]
        else:
            break
    if unwrapped.get("kind") == "ObjCMessageExpr":
        ct = unwrapped.get("classType", {}).get("qualType", "")
        if ct and ct in (module.classes if module else {}):
            return ct

    return cls.name




# ---------------------------------------------------------------------------
# ARC: scope tracking, return cleanup, dealloc, strong ivar assign
# ---------------------------------------------------------------------------

def _object_params(m: OZMethod | OZFunction) -> list[OZParam]:
    """Return object-typed params (excluding self, which is implicit)."""
    return [p for p in m.params if p.oz_type.is_object]


def _emit_break_continue_releases(out: StringIO, ctx: _EmitCtx,
                                   indent: int) -> None:
    """Emit release calls for all object vars from current scope down to loop boundary."""
    if not ctx.loop_scope_depth:
        return
    tabs = "\t" * indent
    loop_depth = ctx.loop_scope_depth[-1]
    for i in range(len(ctx.scope_vars) - 1, loop_depth - 1, -1):
        if i < 0 or i >= len(ctx.scope_vars):
            continue
        frame = ctx.scope_vars[i]
        for name in frame:
            if name not in ctx.consumed_vars:
                out.write(f"{tabs}{ctx.root_class}_release("
                          f"(struct {ctx.root_class} *){name});\n")


def _emit_scope_releases(out: StringIO, ctx: _EmitCtx, indent: int) -> None:
    """Emit release calls for object vars in the current (top) scope frame."""
    if not ctx.scope_vars:
        return
    tabs = "\t" * indent
    frame = ctx.scope_vars[-1]
    for name in frame:
        if name not in ctx.consumed_vars:
            out.write(f"{tabs}{ctx.root_class}_release("
                      f"(struct {ctx.root_class} *){name});\n")


def _emit_return_stmt(node: dict, out: StringIO, ctx: _EmitCtx,
                      indent: int) -> None:
    """Emit return statement with ARC cleanup of all in-scope object vars."""
    tabs = "\t" * indent
    inner = node.get("inner", [])

    # Find the name of the returned variable (if any) to skip its release
    returned_var = _find_returned_var(inner[0]) if inner else None

    # Release all in-scope object vars except returned var and self
    all_vars = _flatten_scope_vars(ctx)
    for name in all_vars:
        if name == returned_var or name in ctx.consumed_vars:
            continue
        out.write(f"{tabs}{ctx.root_class}_release("
                  f"(struct {ctx.root_class} *){name});\n")

    if inner:
        ret_expr = inner[0]
        # Strip ImplicitCastExpr BitCast to id/instancetype on returns — the
        # C function already uses the class pointer type for these returns.
        ret_qt = ret_expr.get("type", {}).get("qualType", "")
        ret_desugared = ret_expr.get("type", {}).get("desugaredQualType", "")
        if (ret_expr.get("kind") == "ImplicitCastExpr"
                and ret_expr.get("castKind") == "BitCast"
                and (ret_qt in ("id", "instancetype")
                     or ret_desugared == "id")):
            ret_inner = ret_expr.get("inner", [])
            if ret_inner:
                ret_expr = ret_inner[0]
        out.write(f"{tabs}return ")
        _emit_expr(ret_expr, out, ctx)
        out.write(";\n")
    else:
        out.write(f"{tabs}return;\n")


def _find_returned_var(node: dict) -> str | None:
    """Walk through casts to find a DeclRefExpr name in a return expression."""
    if not node:
        return None
    kind = node.get("kind", "")
    if kind == "DeclRefExpr":
        return node.get("referencedDecl", {}).get("name")
    if kind in ("ImplicitCastExpr", "CStyleCastExpr", "ParenExpr"):
        inner = node.get("inner", [])
        if inner:
            return _find_returned_var(inner[0])
    return None


def _flatten_scope_vars(ctx: _EmitCtx) -> list[str]:
    """Collect all object var names from all scope frames."""
    result = []
    for frame in ctx.scope_vars:
        for name in frame:
            result.append(name)
    return result


def _is_param_name(name: str, ctx: _EmitCtx) -> bool:
    """Check if a variable name is a method/function parameter."""
    if ctx.method:
        return any(p.name == name for p in ctx.method.params)
    return False


def _is_object_ivar_assign(lhs_node: dict, ctx: _EmitCtx) -> bool:
    """Check if LHS of an assignment is an object ivar."""
    unwrapped = lhs_node
    while unwrapped.get("kind") in ("ImplicitCastExpr",):
        inner = unwrapped.get("inner", [])
        if inner:
            unwrapped = inner[0]
        else:
            break
    if unwrapped.get("kind") != "ObjCIvarRefExpr":
        return False
    ivar_name = unwrapped.get("decl", {}).get("name", "")
    for ivar in ctx.cls.ivars:
        if ivar.name == ivar_name and ivar.oz_type.is_object:
            return True
    return False


def _is_object_local_assign(lhs_node: dict, ctx: _EmitCtx) -> bool:
    """Check if LHS of an assignment is an object-typed local variable in scope."""
    unwrapped = lhs_node
    while unwrapped.get("kind") in ("ImplicitCastExpr",):
        inner = unwrapped.get("inner", [])
        if inner:
            unwrapped = inner[0]
        else:
            break
    if unwrapped.get("kind") != "DeclRefExpr":
        return False
    name = unwrapped.get("referencedDecl", {}).get("name", "")
    if not name or name == "self":
        return False
    for frame in ctx.scope_vars:
        if name in frame:
            return True
    return False


def _is_local_var_rhs(rhs_node: dict, ctx: _EmitCtx) -> bool:
    """Check if RHS is a local/param variable reference (needs retain on assign)."""
    unwrapped = rhs_node
    while unwrapped.get("kind") in ("ImplicitCastExpr",):
        inner = unwrapped.get("inner", [])
        if inner:
            unwrapped = inner[0]
        else:
            break
    if unwrapped.get("kind") != "DeclRefExpr":
        return False
    name = unwrapped.get("referencedDecl", {}).get("name", "")
    if not name or name == "self":
        return False
    for frame in ctx.scope_vars:
        if name in frame:
            return True
    return False


def _emit_strong_local_assign(node: dict, out: StringIO, ctx: _EmitCtx,
                               indent: int) -> None:
    """Emit release(old); var = new; for object local variable reassignment."""
    tabs = "\t" * indent
    inner = node.get("inner", [])
    rhs = inner[1]
    root = ctx.root_class

    # Get var name from LHS
    unwrapped = inner[0]
    while unwrapped.get("kind") in ("ImplicitCastExpr",):
        unwrapped = unwrapped.get("inner", [unwrapped])[0]
    var_name = unwrapped.get("referencedDecl", {}).get("name", "")

    rhs_buf = StringIO()
    _emit_expr(rhs, rhs_buf, ctx)
    rhs_str = rhs_buf.getvalue()

    _flush_pre_stmts(out, ctx, indent)

    # Retain new value if RHS is a local variable (not +1 from alloc)
    if _is_local_var_rhs(rhs, ctx):
        out.write(f"{tabs}{root}_retain((struct {root} *){rhs_str});\n")

    out.write(f"{tabs}{root}_release((struct {root} *){var_name});\n")
    out.write(f"{tabs}{var_name} = {rhs_str};\n")

    # If var was previously consumed (transferred to ivar), un-consume it.
    # It now holds a new value that must be released at scope exit.
    if var_name in ctx.consumed_vars:
        ctx.consumed_vars.discard(var_name)


def _emit_strong_ivar_assign(node: dict, out: StringIO, ctx: _EmitCtx,
                              indent: int) -> None:
    """Emit retain(new); release(old); ivar = new; for object ivar assignment."""
    tabs = "\t" * indent
    inner = node.get("inner", [])
    lhs = inner[0]
    rhs = inner[1]
    root = ctx.root_class

    # Get ivar name
    unwrapped = lhs
    while unwrapped.get("kind") in ("ImplicitCastExpr",):
        unwrapped = unwrapped.get("inner", [unwrapped])[0]
    ivar_name = unwrapped.get("decl", {}).get("name", "")

    # Emit: retain new, release old, assign
    rhs_buf = StringIO()
    _emit_expr(rhs, rhs_buf, ctx)
    _flush_pre_stmts(out, ctx, indent)
    rhs_str = rhs_buf.getvalue()

    # Use temp var if RHS may have side effects (function/method calls)
    rhs_var = _find_returned_var(rhs)
    needs_retain = False
    if rhs_var and rhs_var != "self":
        # Simple variable reference — safe to use directly
        val = rhs_str
        # Params need retain (borrowed reference); locals transfer ownership
        if _is_param_name(rhs_var, ctx):
            needs_retain = True
    else:
        # Function/method call returns +1; use temp, no extra retain needed
        tmp = f"_oz_recv{ctx._tmp_counter}"
        ctx._tmp_counter += 1
        lhs_unwrapped = lhs
        while lhs_unwrapped.get("kind") in ("ImplicitCastExpr",):
            lhs_unwrapped = lhs_unwrapped.get("inner", [lhs_unwrapped])[0]
        ivar_type = lhs_unwrapped.get("type", {}).get("qualType", "id")
        c_type = OZType(ivar_type).c_type
        out.write(f"{tabs}{c_type}{tmp} = {rhs_str};\n")
        val = tmp

    if needs_retain:
        out.write(f"{tabs}{root}_retain((struct {root} *){val});\n")
    out.write(f"{tabs}{root}_release((struct {root} *)self->{ivar_name});\n")
    out.write(f"{tabs}self->{ivar_name} = {val};\n")

    # Track consumed local: mark so it won't be released at scope exit.
    # Skip params — they have their own entry-retain balanced by scope-exit release.
    if rhs_var and rhs_var != "self" and not _is_param_name(rhs_var, ctx):
        for frame in ctx.scope_vars:
            if rhs_var in frame:
                ctx.consumed_vars.add(rhs_var)
                break


def _emit_user_dealloc(ctx: _EmitCtx, m: OZMethod, out: StringIO) -> None:
    """Emit user-defined dealloc with prepended ivar releases and appended parent dealloc."""
    cls = ctx.cls
    module = ctx.module
    root_class = ctx.root_class
    is_root = not cls.superclass or cls.superclass not in module.classes
    obj_ivars = [iv for iv in cls.ivars if iv.oz_type.is_object]

    out.write(f"{_method_prototype(cls, m)}\n")
    out.write("{\n")

    # Emit user body statements, filtering out [super dealloc]
    if m.body_ast:
        ctx.method = m
        ctx.scope_vars = []
        ctx.consumed_vars = set()
        ctx._sync_counter = 0
        for child in m.body_ast.get("inner", []):
            if _is_super_dealloc(child):
                continue
            _emit_stmt(child, out, ctx, indent=1)

    # Release object ivars after user body
    for iv in obj_ivars:
        out.write(f"\t{root_class}_release((struct {root_class} *)self->{iv.name});\n")

    # Append: parent dealloc or dispatch_free (root)
    if not is_root:
        parent = cls.superclass
        out.write(f"\t{parent}_dealloc((struct {parent} *)self);\n")
    else:
        out.write(f"\t{root_class}_dispatch_free((struct {root_class} *)self);\n")

    out.write("}\n\n")


def _is_super_dealloc(node: dict) -> bool:
    """Check if a statement is [super dealloc]."""
    if node.get("kind") == "ObjCMessageExpr":
        if (node.get("selector") == "dealloc" and
                node.get("receiverKind", "").startswith("super")):
            return True
    return False


def _emit_auto_dealloc(ctx: _EmitCtx, out: StringIO) -> None:
    """Auto-generate dealloc method if class has object ivars or needs parent chain."""
    cls = ctx.cls
    module = ctx.module
    root_class = ctx.root_class
    is_root = not cls.superclass or cls.superclass not in module.classes

    # Special dealloc for collection classes (only when item pool exists)
    if ctx.has_item_pool:
        if cls.name == "OZArray":
            _emit_collection_dealloc_array(cls, root_class, is_root, out)
            return
        if cls.name == "OZDictionary":
            _emit_collection_dealloc_dict(cls, root_class, is_root, out)
            return

    # Collect object ivars
    obj_ivars = [iv for iv in cls.ivars if iv.oz_type.is_object]

    # Only generate if there are object ivars or a parent dealloc to call
    if not obj_ivars and is_root:
        return

    out.write(f"void {cls.name}_dealloc(struct {cls.name} *self)\n")
    out.write("{\n")

    for iv in obj_ivars:
        out.write(f"\t{root_class}_release((struct {root_class} *)self->{iv.name});\n")

    if not is_root:
        parent = cls.superclass
        out.write(f"\t{parent}_dealloc((struct {parent} *)self);\n")
    else:
        out.write(f"\t{root_class}_dispatch_free((struct {root_class} *)self);\n")

    out.write("}\n\n")


def _emit_collection_dealloc_array(cls: OZClass, root_class: str,
                                   is_root: bool, out: StringIO) -> None:
    """Emit dealloc for OZArray: release elements, free contiguous items buffer."""
    out.write(f"void {cls.name}_dealloc(struct {cls.name} *self)\n")
    out.write("{\n")
    out.write(f"\tfor (unsigned int i = 0; i < self->_count; i++) {{\n")
    out.write(f"\t\t{root_class}_release(self->_items[i]);\n")
    out.write(f"\t}}\n")
    out.write(f"\tif (self->_items) {{\n")
    out.write(f"\t\toz_mem_blocks_free_contiguous(&oz_item_pool,\n")
    out.write(f"\t\t\tself->_items, self->_count);\n")
    out.write(f"\t}}\n")
    if not is_root:
        out.write(f"\t{cls.superclass}_dealloc((struct {cls.superclass} *)self);\n")
    else:
        out.write(f"\t{root_class}_dispatch_free((struct {root_class} *)self);\n")
    out.write("}\n\n")


def _emit_collection_dealloc_dict(cls: OZClass, root_class: str,
                                  is_root: bool, out: StringIO) -> None:
    """Emit dealloc for OZDictionary: release keys+values, free contiguous buffer."""
    out.write(f"void {cls.name}_dealloc(struct {cls.name} *self)\n")
    out.write("{\n")
    out.write(f"\tfor (unsigned int i = 0; i < self->_count; i++) {{\n")
    out.write(f"\t\t{root_class}_release(self->_keys[i]);\n")
    out.write(f"\t\t{root_class}_release(self->_values[i]);\n")
    out.write(f"\t}}\n")
    out.write(f"\tif (self->_keys) {{\n")
    out.write(f"\t\toz_mem_blocks_free_contiguous(&oz_item_pool,\n")
    out.write(f"\t\t\tself->_keys, self->_count * 2);\n")
    out.write(f"\t}}\n")
    if not is_root:
        out.write(f"\t{cls.superclass}_dealloc((struct {cls.superclass} *)self);\n")
    else:
        out.write(f"\t{root_class}_dispatch_free((struct {root_class} *)self);\n")
    out.write("}\n\n")


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _is_fresh_alloc(node: dict) -> bool:
    """Check if an AST node produces a fresh +1 allocation (ownership transferred).

    Fresh allocs: boxed numbers (@42), array/dict literals, string literals.
    These expressions produce a +1 object that can be consumed without retain.
    Everything else (DeclRefExpr, ObjCMessageExpr, etc.) is an existing
    reference that needs retain before being consumed.
    """
    kind = node.get("kind", "")
    if kind in ("ObjCBoxedExpr", "ObjCArrayLiteral",
                "ObjCDictionaryLiteral", "ObjCStringLiteral"):
        return True
    if kind == "ImplicitCastExpr":
        inner = node.get("inner", [])
        if inner:
            return _is_fresh_alloc(inner[0])
    return False


def _is_owning_expr(node: dict) -> bool:
    """Check if an expression returns a +1 (owning) reference.

    Returns True for expressions that produce +1 ownership:
    - Literal expressions (@42, @[...], @{...}, @"...")
    - alloc, new, copy, mutableCopy messages
    - init message on alloc receiver (the [[Foo alloc] init] pattern)
    Returns False for everything else (+0 borrowed reference).
    """
    kind = node.get("kind", "")
    if kind in ("ObjCBoxedExpr", "ObjCArrayLiteral",
                "ObjCDictionaryLiteral", "ObjCStringLiteral"):
        return True
    if kind in ("ImplicitCastExpr", "ExprWithCleanups", "ParenExpr",
                "CStyleCastExpr", "ExplicitCastExpr"):
        inner = node.get("inner", [])
        if inner:
            return _is_owning_expr(inner[0])
    if kind == "ObjCMessageExpr":
        sel = node.get("selector", "")
        if sel in ("alloc", "new", "copy", "mutableCopy"):
            return True
        # init on alloc receiver: [[Foo alloc] init] → +1
        if sel.startswith("init"):
            inner = node.get("inner", [])
            if inner and _is_owning_expr(inner[0]):
                return True
    return False


def _is_borrowed_object_expr(node: dict) -> bool:
    """Check if an expression returns a +0 (borrowed) object reference.

    Returns True for non-owning method calls (objectAtIndex:, etc.) and
    variable references (DeclRefExpr). Returns False for owning returns
    (alloc, init, new, copy, literals) and non-object expressions (nil, 0).
    """
    kind = node.get("kind", "")
    if kind in ("ImplicitCastExpr", "ExprWithCleanups", "ParenExpr",
                "CStyleCastExpr", "ExplicitCastExpr"):
        inner = node.get("inner", [])
        return bool(inner) and _is_borrowed_object_expr(inner[0])
    if kind == "ObjCMessageExpr":
        sel = node.get("selector", "")
        if sel in ("alloc", "new", "copy", "mutableCopy"):
            return False
        if sel.startswith("init"):
            return False
        return True
    if kind == "DeclRefExpr":
        qt = node.get("type", {}).get("qualType", "")
        oz = OZType(qt)
        return oz.is_object
    return False


def _count_item_slots(module: OZModule) -> int:
    """Count total id-slots needed for dynamic array/dict literals."""
    total = 0

    def walk(node: dict) -> None:
        nonlocal total
        kind = node.get("kind", "")
        if kind == "ObjCArrayLiteral":
            total += len(node.get("inner", []))
        elif kind == "ObjCDictionaryLiteral":
            total += len(node.get("inner", []))  # keys + values
        for child in node.get("inner", []):
            walk(child)

    for cls in module.classes.values():
        for m in cls.methods:
            if m.body_ast:
                walk(m.body_ast)
        for func in cls.functions:
            if func.body_ast:
                walk(func.body_ast)
    for func in module.functions:
        if func.body_ast:
            walk(func.body_ast)

    return total


def _count_alloc_calls(module: OZModule) -> dict[str, int]:
    """Count allocations across all method/function body ASTs.

    Counts explicit [ClassName alloc] calls plus implicit allocations
    from literal expressions (@42 → OZNumber, @[...] → OZArray,
    @{...} → OZDictionary).
    """
    counts: dict[str, int] = {}

    def walk(node: dict) -> None:
        kind = node.get("kind", "")
        if (kind == "ObjCMessageExpr" and
                node.get("selector") == "alloc" and
                node.get("receiverKind") == "class"):
            class_name = node.get("classType", {}).get("qualType", "")
            if class_name:
                counts[class_name] = counts.get(class_name, 0) + 1
        elif kind == "ObjCArrayLiteral":
            counts["OZArray"] = counts.get("OZArray", 0) + 1
        elif kind == "ObjCDictionaryLiteral":
            counts["OZDictionary"] = counts.get("OZDictionary", 0) + 1
        elif kind == "ObjCBoxedExpr":
            counts["OZNumber"] = counts.get("OZNumber", 0) + 1
        elif kind == "ObjCAtSynchronizedStmt":
            counts["OZLock"] = counts.get("OZLock", 0) + 1
        for child in node.get("inner", []):
            walk(child)

    for cls in module.classes.values():
        for m in cls.methods:
            if m.body_ast:
                walk(m.body_ast)
        for func in cls.functions:
            if func.body_ast:
                walk(func.body_ast)
    for func in module.functions:
        if func.body_ast:
            walk(func.body_ast)

    return counts


def _selector_to_c(selector: str) -> str:
    """Convert an ObjC selector to a C-safe identifier.

    Example: "setPin:color:" -> "setPin_color_"
    """
    return selector.replace(":", "_")


def _base_chain(class_name: str, module: OZModule) -> str:
    """Build 'obj->base.base...' prefix for reaching root fields."""
    cls = module.classes.get(class_name)
    if not cls:
        return "obj->"
    parts = []
    cur = cls
    while cur.superclass and cur.superclass in module.classes:
        parts.append("base.")
        cur = module.classes[cur.superclass]
    prefix = "obj->" + "".join(parts)
    return prefix


def _method_prototype(cls: OZClass, m: OZMethod) -> str:
    # instancetype / id returns the class's own pointer type
    raw_ret = m.return_type.raw_qual_type.strip()
    is_init_family = (m.selector == "init" or m.selector.startswith("init:")
                      or m.selector.startswith("initWith"))
    if raw_ret == "instancetype" or (raw_ret == "id" and is_init_family):
        ret = f"struct {cls.name} *"
    else:
        ret = m.return_type.c_type
    c_sel = _selector_to_c(m.selector)
    if m.is_class_method:
        prefix = "cls_"
        parts = [p.oz_type.c_param_decl(p.name) for p in m.params]
        params_str = ", ".join(parts) if parts else "void"
    else:
        prefix = ""
        params_str = f"struct {cls.name} *self"
        for p in m.params:
            params_str += f", {p.oz_type.c_param_decl(p.name)}"
    return f"{ret} {cls.name}_{prefix}{c_sel}({params_str})"


def _write_file(path: str, content: str) -> None:
    with open(path, "w") as f:
        f.write(content)
