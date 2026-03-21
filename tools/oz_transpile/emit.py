# SPDX-License-Identifier: Apache-2.0
#
# emit.py - Pass 3: OZModule -> .h/.c files

from __future__ import annotations

import os
from dataclasses import dataclass, field
from io import StringIO

from jinja2 import Environment, FileSystemLoader

import re
from pathlib import Path

import tree_sitter_objc as tsobjc
from tree_sitter import Language, Parser

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
    source_bytes: bytes | None = None
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


def _inject_oz_spinlock(module: OZModule, root_class: str) -> None:
    """Add synthetic OZSpinLock class if any @synchronized is used."""
    counts = _count_alloc_calls(module)
    if counts.get("OZSpinLock", 0) == 0:
        return
    if "OZSpinLock" in module.classes:
        return
    max_id = max((c.class_id for c in module.classes.values()), default=-1)
    module.classes["OZSpinLock"] = OZClass(
        name="OZSpinLock",
        superclass=root_class,
        ivars=[
            OZIvar("_lock", OZType("oz_spinlock_t")),
            OZIvar("_key", OZType("oz_spinlock_key_t")),
            OZIvar("_obj", OZType("OZObject *")),
        ],
        class_id=max_id + 1,
        base_depth=1,
        is_foundation=True,
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


"""Known foundation class names auto-tagged when --sources is not provided."""
_FOUNDATION_NAMES = frozenset({
    "OZObject", "OZString", "OZMutableString", "OZArray", "OZDictionary",
    "OZNumber", "OZDefer", "OZHeap", "OZSpinLock",
})


def _ensure_foundation_tags(module: OZModule, root_class: str) -> None:
    """Auto-tag well-known foundation classes that were not tagged by __main__."""
    for cls in module.classes.values():
        if not cls.is_foundation and cls.name in _FOUNDATION_NAMES:
            cls.is_foundation = True
    if root_class in module.classes:
        module.classes[root_class].is_foundation = True


def emit(module: OZModule, outdir: str, pool_sizes: dict[str, int] | None = None,
         root_class: str = "OZObject",
         item_pool_size: int | None = None,
         heap_support: bool = False) -> list[str]:
    """Generate C files from OZModule. Returns list of generated file paths."""
    os.makedirs(outdir, exist_ok=True)
    foundation_dir = os.path.join(outdir, "Foundation")
    os.makedirs(foundation_dir, exist_ok=True)
    env = _create_env()
    files = []

    _associate_module_items(module)
    _inject_oz_spinlock(module, root_class)
    _ensure_foundation_tags(module, root_class)

    # Compute pool sizes and item pool count early (needed by per-class templates)
    auto_counts = _count_alloc_calls(module)
    _pool_sizes = pool_sizes or {}
    if item_pool_size is not None:
        _item_pool_count = item_pool_size
    else:
        _item_pool_count = _count_item_slots(module)
    _has_item_pool = _item_pool_count > 0

    def _pool_count_for(cls_name: str) -> int:
        return _pool_sizes.get(cls_name,
                               max(auto_counts.get(cls_name, 0), 1))

    files.append(_render(env, "oz_dispatch.h.j2",
                         _dispatch_header_ctx(module, root_class,
                                              _item_pool_count),
                         foundation_dir, "oz_dispatch.h"))
    files.append(_render(env, "oz_dispatch.c.j2",
                         _dispatch_source_ctx(module, root_class,
                                              _item_pool_count,
                                              heap_support=heap_support),
                         foundation_dir, "oz_dispatch.c"))

    # Group classes by source stem for per-file emission
    stem_groups: dict[str, list[OZClass]] = {}
    for cls in module.classes.values():
        stem = _header_stem(cls)
        stem_groups.setdefault(stem, []).append(cls)

    for stem, classes in stem_groups.items():
        is_foundation = all(c.is_foundation for c in classes)
        dest = foundation_dir if is_foundation else outdir

        # Headers always use templates
        header_tmpl = env.get_template("class_header.h.j2")
        header_parts = []
        for cls in classes:
            ctx = _EmitCtx(cls=cls, module=module, root_class=root_class,
                           has_item_pool=_has_item_pool)
            header_parts.append(header_tmpl.render(
                **_class_header_ctx(ctx, stem,
                                    item_pool_count=_item_pool_count,
                                    heap_support=heap_support)))
        header_path = os.path.join(dest, f"{stem}_ozh.h")
        _write_file(header_path, "\n".join(header_parts))
        files.append(header_path)

        # Sources: use patched emission for user classes with source_path
        stem_source = module.source_paths.get(stem)
        use_patched = (not is_foundation
                       and stem_source is not None
                       and stem_source.is_file())
        if use_patched:
            content = _emit_patched_source(
                stem_source, module, classes, stem,
                root_class, _has_item_pool, _pool_count_for)
            source_path = os.path.join(dest, f"{stem}_ozm.c")
            _write_file(source_path, content)
        else:
            source_tmpl = env.get_template("class_source.c.j2")
            source_parts = []
            for cls in classes:
                ctx = _EmitCtx(cls=cls, module=module, root_class=root_class,
                               has_item_pool=_has_item_pool)
                source_parts.append(
                    source_tmpl.render(**_class_source_ctx(
                        ctx, stem,
                        pool_count=_pool_count_for(cls.name))))
            source_path = os.path.join(dest, f"{stem}_ozm.c")
            _write_file(source_path, "\n".join(source_parts))
        files.append(source_path)

    # Emit orphan sources (class-less .m files)
    for orphan in module.orphan_sources:
        if (orphan.source_path is not None
                and orphan.source_path.is_file()):
            content = _emit_patched_orphan_source(orphan, module, root_class)
            orphan_path = os.path.join(outdir, f"{orphan.stem}_ozm.c")
            _write_file(orphan_path, content)
            files.append(orphan_path)
        else:
            ctx_dict = _orphan_source_ctx(orphan, module, root_class)
            files.append(_render(env, "orphan_source.c.j2",
                                 ctx_dict, outdir, f"{orphan.stem}_ozm.c"))

    return files


# ---------------------------------------------------------------------------
# oz_dispatch.h
# ---------------------------------------------------------------------------

def _dispatch_header_ctx(module: OZModule, root_class: str = "OZObject",
                         item_pool_count: int = 0) -> dict:
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

    # Build impl_map: for each class x selector pair, the implementing class
    impl_map = []
    for sel_name in sorted(proto_sels_map.keys()):
        c_sel = _selector_to_c(sel_name)
        for cls in classes:
            impl_cls = _find_implementing_class(cls, sel_name, module)
            if impl_cls:
                impl_map.append({
                    "cls_name": cls.name,
                    "c_sel": c_sel,
                    "impl_name": impl_cls.name,
                })

    # Collect user includes needed by protocol dispatch param types
    dispatch_includes = []
    for cls in module.classes.values():
        for inc in cls.user_includes:
            if inc not in dispatch_includes:
                dispatch_includes.append(inc)

    return {
        "classes": classes,
        "class_count": len(module.classes),
        "proto_sels": proto_sels,
        "impl_map": impl_map,
        "root_class": root_class,
        "item_pool_count": item_pool_count,
        "dispatch_includes": dispatch_includes,
        "initialize_classes": module.initialize_classes,
    }


def _dispatch_source_ctx(module: OZModule, root_class: str = "OZObject",
                         item_pool_count: int = 0,
                         heap_support: bool = False) -> dict:
    """Build template context for oz_dispatch.c."""
    sorted_classes = sorted(module.classes.values(), key=lambda c: c.class_id)

    classes = []
    for cls in sorted_classes:
        super_id = (f"OZ_CLASS_{cls.superclass}"
                    if cls.superclass and cls.superclass in module.classes
                    else "OZ_CLASS_COUNT")
        classes.append({"name": cls.name, "super_id_expr": super_id,
                        "header_stem": _header_stem(cls)})

    # Collect unique protocol selectors (instance methods only)
    proto_sels_map: dict[str, OZMethod] = {}
    for cls in module.classes.values():
        for m in cls.methods:
            if (m.dispatch == DispatchKind.PROTOCOL
                    and not m.is_class_method
                    and m.selector not in proto_sels_map):
                proto_sels_map[m.selector] = m

    # Build vtable selectors grouped by selector for const array emission
    vtable_sels = []
    for sel_name in sorted(proto_sels_map.keys()):
        c_sel = _selector_to_c(sel_name)
        entries = []
        for cls in sorted_classes:
            impl_cls = _find_implementing_class(cls, sel_name, module)
            if impl_cls:
                entries.append({
                    "cls_name": cls.name,
                    "impl_name": impl_cls.name,
                })
        vtable_sels.append({"c_sel": c_sel, "entries": entries})

    return {
        "classes": classes,
        "vtable_sels": vtable_sels,
        "root_class": root_class,
        "item_pool_count": item_pool_count,
        "initialize_classes": module.initialize_classes,
        "heap_support": heap_support,
    }



def _has_auto_dealloc(cls: OZClass, module: OZModule) -> bool:
    """Check if a class will get an auto-generated dealloc method."""
    is_root = not cls.superclass or cls.superclass not in module.classes
    obj_ivars = [iv for iv in cls.ivars
                 if iv.oz_type.is_object and not iv.oz_type.is_unretained]
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
# Type-definition scanning for source files (OZ-069)
# ---------------------------------------------------------------------------

_TYPEDEF_IDENT_RE = re.compile(r'^\t(\w+)', re.MULTILINE)


def _type_def_constant_map(module: OZModule) -> dict[str, str]:
    """Map each enum constant name to its full enum definition.

    Only processes enum type_defs — struct/union field names are too
    generic (e.g. 'x', 'i') and their definitions are already visible
    through preserved user_includes (#include directives).
    """
    result: dict[str, str] = {}
    for key, definition in module.type_defs.items():
        if not key.startswith("enum "):
            continue
        for m in _TYPEDEF_IDENT_RE.finditer(definition):
            ident = m.group(1)
            result[ident] = definition
    return result


def _scan_source_type_defs(source_texts: list[str],
                           const_map: dict[str, str],
                           header_type_defs: list[str]) -> list[str]:
    """Find type definitions needed by source but not already in header."""
    needed: list[str] = []
    for text in source_texts:
        for ident, definition in const_map.items():
            if (ident in text
                    and definition not in needed
                    and definition not in header_type_defs):
                needed.append(definition)
    return needed


def _header_type_defs_for_class(cls: OZClass, module: OZModule) -> list[str]:
    """Compute type_defs that are already emitted in the class header."""
    type_defs: list[str] = []

    def _add(qual_type: str) -> None:
        key = qual_type
        if key not in module.type_defs:
            key = qual_type.rstrip(" *").removeprefix("const ")
        if key in module.type_defs and module.type_defs[key] not in type_defs:
            type_defs.append(module.type_defs[key])

    for ivar in cls.ivars:
        _add(ivar.oz_type.raw_qual_type)
    for m in cls.methods:
        _add(m.return_type.raw_qual_type)
        for p in m.params:
            _add(p.oz_type.raw_qual_type)
    for func in cls.functions:
        _add(func.return_type.raw_qual_type)
        for p in func.params:
            _add(p.oz_type.raw_qual_type)
    return type_defs


# ---------------------------------------------------------------------------
# Per-class .h
# ---------------------------------------------------------------------------

def _class_header_ctx(ctx: _EmitCtx, stem: str | None = None,
                      item_pool_count: int = 0,
                      heap_support: bool = False) -> dict:
    """Build template context for a class header file."""
    cls = ctx.cls
    module = ctx.module
    _root_builtins = {"_refcount", "oz_class_id", "_meta"}
    is_root = not cls.superclass or cls.superclass not in module.classes

    user_ivars = []
    for ivar in cls.ivars:
        if is_root and ivar.name in _root_builtins:
            continue
        user_ivars.append({
            "c_type": ivar.oz_type.c_type,
            "name": ivar.name,
            "decl": ivar.oz_type.c_param_decl(ivar.name),
        })

    _root_skip_sels = {"retain", "release", "retainCount",
                       "isEqual:", "cDescription:maxLength:"}
    method_prototypes = []
    has_dealloc = False
    for m in cls.methods:
        if is_root and m.selector in _root_skip_sels:
            continue
        if m.selector == "allocWithHeap:" and heap_support:
            continue
        if m.selector == "dealloc":
            has_dealloc = True
        method_prototypes.append(_method_prototype(cls, m))

    auto_dealloc_proto = False
    if not has_dealloc:
        obj_ivars = [iv for iv in cls.ivars
                     if iv.oz_type.is_object and not iv.oz_type.is_unretained]
        if obj_ivars or not is_root:
            auto_dealloc_proto = True

    # Collect type definitions needed by ivars, methods, and functions
    # (enum/union/struct from stubs/user)
    type_defs = []

    def _add_type_def(qual_type: str) -> None:
        key = qual_type
        if key not in module.type_defs:
            key = qual_type.rstrip(" *").removeprefix("const ")
        if key in module.type_defs and module.type_defs[key] not in type_defs:
            type_defs.append(module.type_defs[key])

    for ivar in cls.ivars:
        _add_type_def(ivar.oz_type.raw_qual_type)
    for m in cls.methods:
        _add_type_def(m.return_type.raw_qual_type)
        for p in m.params:
            _add_type_def(p.oz_type.raw_qual_type)
    for func in cls.functions:
        _add_type_def(func.return_type.raw_qual_type)
        for p in func.params:
            _add_type_def(p.oz_type.raw_qual_type)

    function_protos = []
    for func in cls.functions:
        ret = func.return_type.c_type
        parts = [p.oz_type.c_param_decl(p.name) for p in func.params]
        params_str = ", ".join(parts) if parts else "void"
        function_protos.append(f"{ret} {func.name}({params_str});")

    extern_decls = []
    # Static variables are TU-private — no extern declarations in headers

    superclass_stem = None
    if cls.superclass and cls.superclass in module.classes:
        superclass_stem = _header_stem(module.classes[cls.superclass])

    number_inits = [
        {"suffix": "Int32", "c_type": "int32_t", "tag": "OZ_NUM_INT32",
         "field": "i32"},
        {"suffix": "Uint32", "c_type": "uint32_t", "tag": "OZ_NUM_UINT32",
         "field": "u32"},
        {"suffix": "Float", "c_type": "float", "tag": "OZ_NUM_FLOAT",
         "field": "f32"},
        {"suffix": "Int8", "c_type": "int8_t", "tag": "OZ_NUM_INT8",
         "field": "i8"},
        {"suffix": "Uint8", "c_type": "uint8_t", "tag": "OZ_NUM_UINT8",
         "field": "u8"},
        {"suffix": "Int16", "c_type": "int16_t", "tag": "OZ_NUM_INT16",
         "field": "i16"},
        {"suffix": "Uint16", "c_type": "uint16_t", "tag": "OZ_NUM_UINT16",
         "field": "u16"},
    ]

    # Check if any class in the module uses atomic properties — the lock
    # field lives in the root class so it must be emitted when any class
    # in the hierarchy needs it.
    has_atomic_props = False
    if is_root:
        for c in module.classes.values():
            for p in c.properties:
                if not p.is_nonatomic:
                    has_atomic_props = True
                    break
            if has_atomic_props:
                break

    return {
        "name": cls.name,
        "stem": stem or _header_stem(cls),
        "is_root": is_root,
        "superclass": cls.superclass,
        "superclass_header": superclass_stem,
        "user_ivars": user_ivars,
        "ivar_type_defs": type_defs,
        "method_prototypes": method_prototypes,
        "auto_dealloc_proto": auto_dealloc_proto,
        "has_any_methods": bool(cls.methods) or is_root,
        "function_protos": function_protos,
        "extern_decls": extern_decls,
        "base_chain": _base_chain(cls.name, module),
        "root_class": ctx.root_class,
        "number_inits": number_inits,
        "item_pool_count": item_pool_count,
        "user_includes": cls.user_includes,
        "has_atomic_props": has_atomic_props,
        "heap_support": heap_support,
    }


# ---------------------------------------------------------------------------
# Synthesized property accessors
# ---------------------------------------------------------------------------

def _self_lock_chain(cls: OZClass, module: OZModule) -> str:
    """Build 'self->base.base..._oz_prop_lock' for reaching the root lock field."""
    parts = []
    cur = cls
    while cur.superclass and cur.superclass in module.classes:
        parts.append("base.")
        cur = module.classes[cur.superclass]
    return "self->" + "".join(parts) + "_oz_prop_lock"


def _emit_synthesized_accessor(cls: OZClass, m: OZMethod,
                                out: StringIO,
                                root_class: str = "OZObject",
                                module: OZModule | None = None) -> None:
    """Emit a synthesized getter or setter for a property."""
    prop = m.synthesized_property
    ivar = prop.ivar_name
    is_getter = len(m.params) == 0
    is_atomic = not prop.is_nonatomic
    c_type = prop.oz_type.c_type
    is_strong_obj = prop.oz_type.is_object and prop.ownership == "strong"
    root = root_class

    lock_expr = _self_lock_chain(cls, module) if (is_atomic and module) else None

    if is_getter:
        if is_atomic:
            out.write("{\n")
            out.write(f"\t{c_type} val;\n")
            out.write(f"\tOZ_SPINLOCK(&{lock_expr}) {{\n")
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
                out.write(f"\t{c_type} old;\n")
                out.write(f"\t{root}_retain((struct {root} *){param_name});\n")
                out.write(f"\tOZ_SPINLOCK(&{lock_expr}) {{\n")
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
                out.write(f"\tOZ_SPINLOCK(&{lock_expr}) {{\n")
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

def _dep_includes(cls: OZClass, module: OZModule, stem: str) -> list[str]:
    """Collect header stems of other classes this class depends on."""
    own_stem = stem
    # Collect stems of all classes whose alloc/init/methods are called
    deps = set()
    for other in module.classes.values():
        other_stem = _header_stem(other)
        if other_stem == own_stem:
            continue
        deps.add(other_stem)
    # The class header already includes superclass chain via superclass_header,
    # but source files may call sibling classes. Include all to be safe.
    return sorted(deps)


def _class_source_ctx(ctx: _EmitCtx, stem: str | None = None,
                      pool_count: int = 1) -> dict:
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
            _emit_synthesized_accessor(cls, m, buf, root_class, ctx.module)
        elif m.body_ast:
            _emit_compound_stmt(m.body_ast, buf, ctx, indent=0,
                                param_retains=_object_params(m))
        else:
            buf.write("{\n}\n")
        method_bodies.append(buf.getvalue().rstrip("\n"))

    if not has_user_dealloc and cls.name != "OZSpinLock":
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
        static_decls.append(f"static {decl_str}{init};")

    # OZ-069: scan rendered source texts for type_def references not
    # already emitted in any class header (all headers are included via
    # dep_includes, so type_defs from any header are visible here).
    header_td: list[str] = []
    for c in ctx.module.classes.values():
        for td in _header_type_defs_for_class(c, ctx.module):
            if td not in header_td:
                header_td.append(td)
    const_map = _type_def_constant_map(ctx.module)
    src_type_defs = _scan_source_type_defs(
        method_bodies + function_bodies + static_decls + cls.verbatim_lines,
        const_map, header_td)

    return {
        "name": cls.name,
        "stem": stem or _header_stem(cls),
        "is_root": is_root,
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
        "pool_count": pool_count,
        "dep_includes": _dep_includes(cls, ctx.module, stem or _header_stem(cls)),
        "src_type_defs": src_type_defs,
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
        static_decls.append(f"static {decl_str}{init};")

    # OZ-069: orphan sources have no header, so header_type_defs is empty.
    const_map = _type_def_constant_map(module)
    src_type_defs = _scan_source_type_defs(
        function_bodies + static_decls + orphan.verbatim_lines,
        const_map, [])

    return {
        "stem": orphan.stem,
        "function_bodies": function_bodies,
        "static_decls": static_decls,
        "user_includes": orphan.user_includes,
        "verbatim_lines": orphan.verbatim_lines,
        "src_type_defs": src_type_defs,
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
    out.write("\tif (self->_meta.immortal) {\n")
    out.write("\t\treturn;\n")
    out.write("\t}\n")
    out.write(f"\tif (oz_atomic_dec_and_test(&self->_refcount)) {{\n")
    out.write("\t\tif (self->_meta.deallocating) {\n")
    out.write("\t\t\treturn;\n")
    out.write("\t\t}\n")
    out.write("\t\tself->_meta.deallocating = 1;\n")
    out.write(f"\t\tOZ_PROTOCOL_SEND_dealloc((struct {cls.name} *)self);\n")
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
    out.write("\t\toz_class_names[self->_meta.class_id], (void *)self);\n")
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
    """Emit @synchronized(obj) { ... } as OZSpinLock RAII block."""
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

    out.write(f"{tabs1}struct OZSpinLock *{sync_name} = OZSpinLock_initWithObject("
              f"OZSpinLock_alloc(), (struct {ctx.root_class} *){obj_buf.getvalue()});\n")
    ctx.scope_vars[-1][sync_name] = OZType("OZSpinLock *")

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

    elif kind == "SwitchStmt":
        _emit_switch_stmt(node, out, ctx, indent)

    elif kind == "CaseStmt":
        _emit_case_stmt(node, out, ctx, indent)

    elif kind == "DefaultStmt":
        inner = node.get("inner", [])
        out.write(f"{tabs}default:\n")
        for child in inner:
            _emit_stmt(child, out, ctx, indent + 1)

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


def _emit_switch_stmt(node: dict, out: StringIO, ctx: _EmitCtx,
                      indent: int) -> None:
    """Emit switch(cond) { case/default ... }."""
    tabs = "\t" * indent
    inner = node.get("inner", [])
    if len(inner) < 2:
        return
    cond = inner[0]
    body = inner[1]
    cond_buf = StringIO()
    _emit_expr(cond, cond_buf, ctx)
    _flush_pre_stmts(out, ctx, indent)
    out.write(f"{tabs}switch ({cond_buf.getvalue()}) {{\n")
    if body.get("kind") == "CompoundStmt":
        for child in body.get("inner", []):
            _emit_stmt(child, out, ctx, indent)
    else:
        _emit_stmt(body, out, ctx, indent)
    out.write(f"{tabs}}}\n")


def _emit_case_stmt(node: dict, out: StringIO, ctx: _EmitCtx,
                    indent: int) -> None:
    """Emit case <expr>: <body>."""
    tabs = "\t" * indent
    inner = node.get("inner", [])
    if not inner:
        return
    case_val = inner[0]
    val_buf = StringIO()
    _emit_expr(case_val, val_buf, ctx)
    out.write(f"{tabs}case {val_buf.getvalue()}:\n")
    for child in inner[1:]:
        _emit_stmt(child, out, ctx, indent + 1)


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
        struct OZObject *_oz_iterN = (struct OZObject *)OZ_PROTOCOL_SEND_iter(collection);
        struct OZObject *_oz_recvN = _oz_iterN;
        for (struct OZObject *obj = OZ_PROTOCOL_SEND_next(_oz_recvN); obj != NULL;
             obj = OZ_PROTOCOL_SEND_next(_oz_recvN)) { body }
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
              f"(struct OZObject *)OZ_PROTOCOL_SEND_iter("
              f"(struct OZObject *){coll_buf.getvalue()});\n")
    out.write(f"{itabs}struct OZObject *{next_recv} = {iter_tmp};\n")
    next_call = f"OZ_PROTOCOL_SEND_next({next_recv})"
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


_BOXED_TYPE_MAP: dict[str, tuple[str, str | None]] = {
    "int":              ("Int32",  None),
    "int32_t":          ("Int32",  None),
    "signed int":       ("Int32",  None),
    "unsigned int":     ("Uint32", None),
    "uint32_t":         ("Uint32", None),
    "float":            ("Float",  None),
    "int8_t":           ("Int8",   None),
    "signed char":      ("Int8",   None),
    "char":             ("Int8",   None),
    "uint8_t":          ("Uint8",  None),
    "unsigned char":    ("Uint8",  None),
    "int16_t":          ("Int16",  None),
    "short":            ("Int16",  None),
    "signed short":     ("Int16",  None),
    "uint16_t":         ("Uint16", None),
    "unsigned short":   ("Uint16", None),
    "long":             ("Int32",  "(int32_t)"),
    "unsigned long":    ("Uint32", "(uint32_t)"),
    "BOOL":             ("Int8",   "(int8_t)"),
    "_Bool":            ("Int8",   "(int8_t)"),
    "double":           ("Float",  "(float)"),
    "long double":      ("Float",  "(float)"),
}


def _boxed_type_lookup(qt: str) -> tuple[str, str | None, bool]:
    """Look up boxed number suffix and optional cast for a qualType string.

    Returns (suffix, cast_or_None, needs_warning).
    """
    entry = _BOXED_TYPE_MAP.get(qt)
    if entry:
        warn = qt in ("double", "long double")
        return entry[0], entry[1], warn
    if qt.startswith("enum "):
        return "Int32", "(int32_t)", False
    return "Int32", "(int32_t)", True


def _emit_boxed_number(node: dict, out: StringIO, ctx: _EmitCtx) -> None:
    """Emit a dynamically allocated OZNumber via OZNumber_initXxx() helper."""
    inner = node.get("inner", [])
    if not inner:
        ctx.module.errors.append(
            "boxed expression @(...) is not supported; "
            "use OZNumber_initInt32() or [[OZNumber alloc] initWithInt:]")
        out.write("((void *)0)")
        return

    original_child = inner[0]
    inner_qt = original_child.get("type", {}).get("qualType", "")

    child = original_child
    child_kind = child.get("kind", "")

    # Unwrap ImplicitCastExpr wrappers to inspect the leaf kind
    while child_kind == "ImplicitCastExpr":
        child = child.get("inner", [{}])[0]
        child_kind = child.get("kind", "")

    # Fast path: literal children (existing behavior, unchanged)
    if child_kind == "IntegerLiteral":
        val = child.get("value", "0")
        out.write(f"OZNumber_initInt32({val})")
        return
    if child_kind == "FloatingLiteral":
        val = child.get("value", "0.0")
        fval = val if val.endswith("f") or val.endswith("F") else f"{val}f"
        out.write(f"OZNumber_initFloat({fval})")
        return
    if child_kind == "CharacterLiteral":
        val = str(child.get("value", 0))
        out.write(f"OZNumber_initInt32({val})")
        return
    if child_kind == "ObjCBoolLiteralExpr":
        raw = child.get("value", False)
        if isinstance(raw, str):
            val = "0" if "no" in raw.lower() else "1"
        else:
            val = "1" if raw else "0"
        out.write(f"OZNumber_initInt8({val})")
        return

    # Expression path: variables, arithmetic, function calls, etc.
    # Reject string types — OZString has no factory for dynamic creation.
    if "char *" in inner_qt or "NSString" in inner_qt or "OZString" in inner_qt:
        ctx.module.errors.append(
            f"boxed string expression @(expr) is not supported; "
            f"use OZString literals instead")
        out.write("((void *)0)")
        return

    suffix, cast, warn = _boxed_type_lookup(inner_qt)
    if warn:
        ctx.module.diagnostics.append(
            f"warning: @(expr) with type '{inner_qt}' narrowed to "
            f"OZNumber_{suffix}; verify no precision loss")

    buf = StringIO()
    _emit_expr(original_child, buf, ctx)
    expr_str = buf.getvalue()

    if cast:
        out.write(f"OZNumber_init{suffix}({cast}({expr_str}))")
    else:
        out.write(f"OZNumber_init{suffix}({expr_str})")


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
            ctx.module.errors.append(
                f"capturing blocks not supported "
                f"(block captures '{var_name}'). "
                f"Use a non-capturing block or a file-scope static variable instead."
            )
            out.write("((void *)0)")
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

    # Generate function name (loc-based to avoid collisions across methods)
    loc = node.get("loc", {})
    line = loc.get("line")
    col = loc.get("col")
    if line is not None and col is not None:
        func_name = f"_oz_block_L{line}_C{col}"
    else:
        func_name = f"_oz_block_{len(ctx.block_functions)}"

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
            source_bytes=ctx.source_bytes,
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


_OBJC_NODE_KINDS = frozenset({
    "ObjCMessageExpr", "ObjCIvarRefExpr", "ObjCStringLiteral",
    "ObjCArrayLiteral", "ObjCDictionaryLiteral", "ObjCBoxedExpr",
    "ObjCSelectorExpr", "ObjCProtocolExpr", "PseudoObjectExpr",
})


def _extract_macro_text(source: bytes, offset: int, tok_len: int) -> str:
    """Extract full macro invocation from source via paren matching.

    Given the offset/tokLen of the macro name, finds the matching closing
    parenthesis (if any) to capture the complete invocation.
    """
    end = offset + tok_len
    rest = source[end:]
    i = 0
    while i < len(rest) and rest[i:i + 1] in (b' ', b'\t', b'\n', b'\r'):
        i += 1
    if i < len(rest) and rest[i:i + 1] == b'(':
        depth = 1
        j = i + 1
        while j < len(rest) and depth > 0:
            if rest[j:j + 1] == b'(':
                depth += 1
            elif rest[j:j + 1] == b')':
                depth -= 1
            j += 1
        return source[offset:end + j].decode()
    return source[offset:end].decode()


def _collect_objc_patches(node: dict, macro_start: int,
                          source: bytes, ctx: _EmitCtx,
                          seen: set | None = None) -> list[tuple[int, int, str]]:
    """Walk macro-expanded AST subtree, find ObjC nodes, return patches.

    Returns list of (rel_start, rel_end, transpiled_text) sorted by offset
    descending for back-to-front application.  Deduplicates by source range.
    """
    if seen is None:
        seen = set()
    patches = []
    kind = node.get("kind", "")
    rng = node.get("range", {})
    begin_spell = rng.get("begin", {}).get("spellingLoc", {})
    end_spell = rng.get("end", {}).get("spellingLoc", {})

    if kind in _OBJC_NODE_KINDS and begin_spell.get("offset") is not None:
        soff = begin_spell["offset"]
        eoff = end_spell.get("offset", soff) + end_spell.get("tokLen", 1)
        key = (soff, eoff)
        if key not in seen:
            seen.add(key)
            buf = StringIO()
            _emit_expr(node, buf, ctx)
            patches.append((soff - macro_start, eoff - macro_start,
                            buf.getvalue()))
            return patches

    for child in node.get("inner", []):
        patches.extend(
            _collect_objc_patches(child, macro_start, source, ctx, seen))
    return patches


def _try_macro_passthrough(node: dict, out: StringIO,
                           ctx: _EmitCtx) -> bool:
    """Attempt macro source passthrough.  Returns True if handled."""
    source = ctx.source_bytes
    if source is None:
        return False
    rng = node.get("range", {})
    begin = rng.get("begin", {})
    exp = begin.get("expansionLoc", {})
    if not exp or exp.get("isMacroArgExpansion"):
        return False
    # Skip system/SDK macros (nil, YES, BOOL, etc.) — their spellingLoc
    # points to an included header, not the user's source file.
    spelling = begin.get("spellingLoc", {})
    if spelling.get("includedFrom"):
        return False
    offset = exp.get("offset")
    tok_len = exp.get("tokLen")
    if offset is None or tok_len is None:
        return False

    macro_text = _extract_macro_text(source, offset, tok_len)
    patches = _collect_objc_patches(node, offset, source, ctx)

    if not patches:
        out.write(macro_text)
        return True

    result = bytearray(macro_text.encode())
    for rel_start, rel_end, transpiled in sorted(patches, reverse=True):
        if 0 <= rel_start < rel_end <= len(result):
            result[rel_start:rel_end] = transpiled.encode()
    out.write(result.decode())
    return True


def _emit_expr(node: dict, out: StringIO, ctx: _EmitCtx) -> None:
    kind = node.get("kind", "")

    if _try_macro_passthrough(node, out, ctx):
        return

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
                # Enforce access control on external ivar access
                owner = _find_ivar_owner(ivar_name, ctx.module)
                if owner:
                    owner_cls, ivar = owner
                    # For free functions (method is None), use a dummy
                    # class that never matches any real class
                    if ctx.method is None:
                        accessor = OZClass(name="__free_function__")
                    else:
                        accessor = ctx.cls
                    if not _check_ivar_access(ivar, owner_cls,
                                              accessor, ctx.module):
                        ctx.module.errors.append(
                            f"instance variable '{ivar_name}' "
                            f"is {ivar.access}")
                        out.write(f"/* access error: {ivar_name} */")
                        return
                _emit_expr(inner[0], out, ctx)
                out.write(f"->{ivar_name}")
                return
        # Build base chain for inherited ivars
        base_prefix = _ivar_base_chain(ivar_name, ctx.cls, ctx.module)
        out.write(f"self->{base_prefix}{ivar_name}")
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

    if kind == "ConstantExpr":
        inner = node.get("inner", [])
        if inner:
            _emit_expr(inner[0], out, ctx)
        else:
            out.write(node.get("value", "0"))
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
            loc = node.get("loc", {})
            line = loc.get("line")
            col = loc.get("col")
            if line is not None and col is not None:
                name = f"_oz_str_L{line}_C{col}"
            else:
                name = f"_oz_str_{len(ctx._string_dedup)}"
            ctx._string_dedup[val] = name
            ctx.string_constants.append(
                f"static struct OZString {name} = {{"
                f"{{{{.class_id = OZ_CLASS_OZString, .immortal = 1}}, 1}}, "
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

    if kind == "UnaryExprOrTypeTraitExpr":
        op_name = node.get("name", "sizeof")
        arg_type = node.get("argType", {}).get("qualType")
        if arg_type:
            out.write(f"{op_name}({arg_type})")
        else:
            inner = node.get("inner", [])
            if inner:
                out.write(f"{op_name}(")
                _emit_expr(inner[0], out, ctx)
                out.write(")")
            else:
                out.write(f"{op_name}(/* unknown */)")
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

    # Track explicit [obj release] so ARC doesn't double-release at scope exit
    if selector == "release" and inner:
        recv = inner[0]
        if recv.get("kind") == "ImplicitCastExpr":
            recv = recv.get("inner", [{}])[0]
        if recv.get("kind") == "DeclRefExpr":
            var_name = recv.get("referencedDecl", {}).get("name", "")
            if var_name and var_name != "self":
                for frame in ctx.scope_vars:
                    if var_name in frame:
                        ctx.consumed_vars.add(var_name)
                        break

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
        if selector == "initialize":
            module.errors.append(
                f"explicit call [{ class_type } initialize] is not allowed; "
                f"+initialize is automatically called before main()")
            return
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
        # Try compile-time dispatch when concrete receiver type is known
        concrete = _try_infer_concrete_class(receiver, module) if receiver else None
        if concrete:
            # Compile-time dispatch: direct function call
            defining = _find_defining_class(concrete, selector, module)
            ret_qt = node.get("type", {}).get("qualType", "void")
            ret_oz = OZType(ret_qt)
            ret_c = ret_oz.c_type
            needs_ret_cast = ret_oz.is_object and ret_c != f"struct {defining} *"
            if needs_ret_cast:
                out.write(f"({ret_c})")
            out.write(f"{defining}_{c_sel}(")
            if receiver:
                needs_arg_cast = defining != concrete
                if needs_arg_cast:
                    out.write(f"(struct {defining} *)")
                _emit_expr(receiver, out, ctx)
            for arg in args_exprs:
                out.write(", ")
                _emit_expr(arg, out, ctx)
            out.write(")")
        else:
            # Polymorphic fallback via const vtable
            ret_qt = node.get("type", {}).get("qualType", "void")
            ret_oz = OZType(ret_qt)
            ret_c = ret_oz.c_type
            needs_cast = ret_oz.is_object and ret_c != f"struct {root_class} *"
            if needs_cast:
                out.write(f"({ret_c})")
            # Emit receiver into a temp var to avoid double evaluation
            # in the OZ_PROTOCOL_SEND macro
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
                out.write(f"OZ_PROTOCOL_SEND_{c_sel}({tmp}")
            else:
                out.write(f"OZ_PROTOCOL_SEND_{c_sel}(")
            for arg in args_exprs:
                out.write(", ")
                _emit_expr(arg, out, ctx)
            out.write(")")
    else:
        inferred_class = _infer_receiver_class(receiver, cls, module) if receiver else cls.name
        # Walk up hierarchy to find class that actually defines the selector
        defining_class = _find_defining_class(inferred_class, selector, module)
        if defining_class == inferred_class:
            cls_obj = module.classes.get(inferred_class)
            if cls_obj and not any(m.selector == selector for m in cls_obj.methods):
                module.diagnostics.append(
                    f"warning: method '{selector}' not found on class "
                    f"'{inferred_class}' or its superclasses")
        out.write(f"{defining_class}_{c_sel}(")
        if receiver:
            needs_cast = defining_class != inferred_class
            if needs_cast:
                out.write(f"(struct {defining_class} *)")
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


def _try_infer_concrete_class(node: dict | None, module: OZModule) -> str | None:
    """Try to infer a concrete class from AST type info. Returns None if unknown."""
    if not node:
        return None
    qt = node.get("type", {}).get("qualType", "")
    oz = OZType(qt)
    if oz.is_object:
        name = oz._strip_qualifiers().rstrip(" *")
        if name and name not in ("id", "instancetype") and name in module.classes:
            return name
    # Unwrap casts to check inner expression
    unwrapped = node
    while unwrapped.get("kind") in ("ImplicitCastExpr", "ParenExpr"):
        inner = unwrapped.get("inner", [])
        if inner:
            unwrapped = inner[0]
        else:
            break
    if unwrapped.get("kind") == "ObjCMessageExpr":
        ct = unwrapped.get("classType", {}).get("qualType", "")
        if ct and ct in module.classes:
            return ct
    return None


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
        # Buffer the expression first so any pre_stmts (e.g. protocol
        # dispatch receiver vars) are emitted before the return keyword
        expr_buf = StringIO()
        _emit_expr(ret_expr, expr_buf, ctx)
        _flush_pre_stmts(out, ctx, indent)
        out.write(f"{tabs}return {expr_buf.getvalue()};\n")
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
        if (ivar.name == ivar_name and ivar.oz_type.is_object
                and not ivar.oz_type.is_unretained):
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
    obj_ivars = [iv for iv in cls.ivars
                 if iv.oz_type.is_object and not iv.oz_type.is_unretained]

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

    # Collect object ivars (skip __unsafe_unretained)
    obj_ivars = [iv for iv in cls.ivars
                 if iv.oz_type.is_object and not iv.oz_type.is_unretained]

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
    for orphan in module.orphan_sources:
        for func in orphan.functions:
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
            counts["OZSpinLock"] = counts.get("OZSpinLock", 0) + 1
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
    for orphan in module.orphan_sources:
        for func in orphan.functions:
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


def _ivar_base_chain(ivar_name: str, cls: OZClass, module: OZModule) -> str:
    """Build 'base.' prefix chain to reach an inherited ivar.

    Returns '' if the ivar is in the current class, 'base.' if in the
    parent, 'base.base.' if in the grandparent, etc.
    """
    # Check if ivar is in the current class
    if any(iv.name == ivar_name for iv in cls.ivars):
        return ""
    # Walk up the hierarchy
    depth = 0
    cur = cls
    while cur.superclass and cur.superclass in module.classes:
        depth += 1
        cur = module.classes[cur.superclass]
        if any(iv.name == ivar_name for iv in cur.ivars):
            return "base." * depth
    return ""


def _find_ivar_owner(ivar_name: str,
                     module: OZModule) -> tuple[OZClass, OZIvar] | None:
    """Find the class that declares a given ivar."""
    for cls in module.classes.values():
        for iv in cls.ivars:
            if iv.name == ivar_name:
                return cls, iv
    return None


def _is_subclass(cls_name: str, parent_name: str, module: OZModule) -> bool:
    """Check if cls_name is a subclass of parent_name."""
    cur = module.classes.get(cls_name)
    while cur and cur.superclass:
        if cur.superclass == parent_name:
            return True
        cur = module.classes.get(cur.superclass)
    return False


def _check_ivar_access(ivar: OZIvar, owner_cls: OZClass,
                       accessor_cls: OZClass,
                       module: OZModule) -> bool:
    """Check if accessor_cls is allowed to access an ivar owned by owner_cls."""
    if ivar.access == "public":
        return True
    if ivar.access == "private":
        return accessor_cls.name == owner_cls.name
    # protected: allow if accessor is the owner or a subclass
    return (accessor_cls.name == owner_cls.name
            or _is_subclass(accessor_cls.name, owner_cls.name, module))


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


_TS_LANG = Language(tsobjc.language())

_OZ_IMPORT_PREFIXES = (
    "Foundation/", "Foundation.h", "objc/", "objc.h",
    "OZObject", "OZLog", "OZString", "OZArray", "OZDictionary", "OZNumber",
)

def _is_objc_header(header_path: Path) -> bool:
    """Check if a header file contains ObjC syntax."""
    if not header_path.is_file():
        return False
    content = header_path.read_text(errors="replace")
    return "@interface" in content or "@implementation" in content


def _find_header(name: str, source_dir: Path) -> Path | None:
    """Find a header file relative to source or include directories."""
    candidates = [
        source_dir / name,
        source_dir.parent / "include" / name,
    ]
    for header in candidates:
        if header.is_file():
            return header
    return None


def _emit_include_replacement(text: str, out: StringIO,
                               source_dir: Path | None = None) -> None:
    """Handle #import and #include directives.

    - #import of OZ SDK headers → skip (covered by generated includes)
    - #import of ObjC headers → #include "Header_ozh.h"
    - #import of non-ObjC → #include
    - #include of ObjC headers → #include "Header_ozh.h"
    - #include of non-ObjC → keep as-is
    """
    stripped = text.strip()
    is_import = stripped.startswith("#import")

    if is_import:
        for prefix in _OZ_IMPORT_PREFIXES:
            if prefix in stripped:
                return

    # For #include, check if it references an ObjC header
    m = re.search(r'"([^"]+)"', stripped)
    if m and source_dir:
        header_name = m.group(1)
        header = _find_header(header_name, source_dir)
        if header and _is_objc_header(header):
            stem = Path(header_name).stem
            out.write(f'#include "{stem}_ozh.h"\n')
            return

    if is_import:
        out.write(stripped.replace("#import", "#include", 1))
        out.write("\n")
    else:
        out.write(text)
        out.write("\n")


def _is_func_prototype(node) -> bool:
    """Check if a tree-sitter declaration is a function prototype (no body)."""
    has_func_decl = False
    for child in node.children:
        if child.type == "function_declarator":
            has_func_decl = True
        elif child.type == "pointer_declarator":
            for sub in child.children:
                if sub.type == "function_declarator":
                    has_func_decl = True
    return has_func_decl


def _extract_decl_name(node) -> str | None:
    """Extract variable name from a tree-sitter declaration node."""
    for child in node.children:
        if child.type == "identifier":
            return child.text.decode()
        elif child.type in ("pointer_declarator", "init_declarator"):
            for sub in child.children:
                if sub.type == "identifier":
                    return sub.text.decode()
                elif sub.type == "pointer_declarator":
                    for subsub in sub.children:
                        if subsub.type == "identifier":
                            return subsub.text.decode()
    return None


def _extract_class_name(node) -> str | None:
    """Extract the class name from a tree-sitter class_implementation node."""
    for child in node.children:
        if child.type == "identifier":
            return child.text.decode()
    return None


def _extract_func_name(node) -> str | None:
    """Extract function name from a tree-sitter function_definition node."""
    for child in node.children:
        if child.type == "function_declarator":
            for sub in child.children:
                if sub.type == "identifier":
                    return sub.text.decode()
        elif child.type == "pointer_declarator":
            for sub in child.children:
                if sub.type == "function_declarator":
                    for subsub in sub.children:
                        if subsub.type == "identifier":
                            return subsub.text.decode()
    return None


def _emit_transpiled_function(func: OZFunction, module: OZModule,
                               out: StringIO,
                               root_class: str,
                               has_item_pool: bool,
                               source_bytes: bytes | None = None) -> None:
    """Emit a transpiled C function (replacing one that contained ObjC)."""
    dummy_cls = OZClass(name="__patched__")
    ctx = _EmitCtx(cls=dummy_cls, module=module, root_class=root_class,
                   has_item_pool=has_item_pool, source_bytes=source_bytes)
    ctx.method = None
    ctx.scope_vars = []
    ctx.consumed_vars = set()
    ctx.loop_scope_depth = []
    ctx.pre_stmts = []
    ctx._tmp_counter = 0

    ret = func.return_type.c_type
    parts = [p.oz_type.c_param_decl(p.name) for p in func.params]
    params_str = ", ".join(parts) if parts else "void"

    # Emit body to buffer first to collect string constants/block functions
    body_buf = StringIO()
    body_buf.write(f"{ret} {func.name}({params_str})\n")
    if func.body_ast:
        _emit_compound_stmt(func.body_ast, body_buf, ctx, indent=0,
                            param_retains=_object_params(func))
    else:
        body_buf.write("{\n}\n")

    # Emit string constants and block functions before the function body
    for sc in ctx.string_constants:
        out.write(sc)
        out.write("\n")
    for bf in ctx.block_functions:
        out.write(bf)
        out.write("\n\n")
    out.write(body_buf.getvalue())


def _emit_patched_source(source_path: Path, module: OZModule,
                          classes: list[OZClass], stem: str,
                          root_class: str,
                          has_item_pool: bool,
                          pool_count_fn=None) -> str:
    """Emit .c via extract+context+render: tree-sitter template + AST context."""
    from jinja2 import Environment as J2Env
    from oz_transpile.extract import extract_template
    from oz_transpile.context import build_source_context

    source = source_path.read_bytes()
    parser = Parser(_TS_LANG)
    tree = parser.parse(source)

    template_str = extract_template(source, tree)
    context = build_source_context(
        source_path, module, classes, stem,
        root_class, has_item_pool, pool_count_fn,
    )

    # Aggregate dependency includes from ALL classes in this stem
    dep_stem_set = set()
    for cls in classes:
        dep_stem_set.update(_dep_includes(cls, module, stem))
    dep_stems = sorted(dep_stem_set)

    # Deduplicate includes: track what the preamble already provides
    emitted_includes = {f'#include "{stem}_ozh.h"'}
    for dep_stem in dep_stems:
        emitted_includes.add(f'#include "{dep_stem}_ozh.h"')

    # Filter include context values to avoid duplicates
    # Normalize whitespace for comparison: "#include  <x>" -> "#include <x>"
    def _normalize_include(s: str) -> str:
        s = s.strip()
        if s.startswith("#include"):
            return "#include " + s[len("#include"):].lstrip()
        return s

    for key, val in list(context.items()):
        stripped = val.strip()
        if stripped.startswith("#include"):
            normalized = _normalize_include(stripped)
            if normalized in emitted_includes:
                context[key] = ""
            else:
                emitted_includes.add(normalized)

    env = J2Env(undefined=__import__("jinja2").StrictUndefined)
    rendered = env.from_string(template_str).render(**context)

    # Build preamble
    out = StringIO()
    out.write("/* Auto-generated by oz_transpile -- do not edit */\n")
    out.write(f'#include "{stem}_ozh.h"\n')
    for dep_stem in dep_stems:
        out.write(f'#include "{dep_stem}_ozh.h"\n')
    for cls in classes:
        pc = pool_count_fn(cls.name) if pool_count_fn else 1
        if not isinstance(pc, int) or pc < 1:
            pc = 1
        out.write(f"\nOZ_SLAB_DEFINE(oz_slab_{cls.name}, "
                  f"sizeof(struct {cls.name}), {pc}, 4);\n")

    out.write(rendered)
    return out.getvalue()


def _emit_patched_orphan_source(orphan: OrphanSource, module: OZModule,
                                 root_class: str) -> str:
    """Emit orphan .c via roundtrip: tree-sitter template + AST context.

    Preserves original declarations (including macro initializers) verbatim.
    """
    from jinja2 import Environment as J2Env
    from oz_transpile.context import build_source_context
    from oz_transpile.extract import extract_template

    source_path = orphan.source_path
    source = source_path.read_bytes()
    parser = Parser(_TS_LANG)
    tree = parser.parse(source)

    template_str = extract_template(source, tree)

    # Collect static names to blank from the template (re-emitted in preamble)
    orphan_static_names = {sv.name for sv in orphan.statics}

    # Temporarily restore orphan functions into module for context building
    saved_funcs = module.functions
    module.functions = list(orphan.functions)
    try:
        context = build_source_context(
            source_path, module, [], orphan.stem,
            root_class, False, None,
            extra_static_names=orphan_static_names,
        )
    finally:
        module.functions = saved_funcs

    # Compute dependency includes: all class headers from the module
    dep_stems = sorted({_header_stem(cls) for cls in module.classes.values()})

    # Deduplicate includes against preamble
    emitted_includes = {'#include "oz_dispatch.h"'}
    for dep_stem in dep_stems:
        emitted_includes.add(f'#include "{dep_stem}_ozh.h"')

    def _normalize_include(s: str) -> str:
        s = s.strip()
        if s.startswith("#include"):
            return "#include " + s[len("#include"):].lstrip()
        return s

    for key, val in list(context.items()):
        stripped = val.strip()
        if stripped.startswith("#include"):
            normalized = _normalize_include(stripped)
            if normalized in emitted_includes:
                context[key] = ""
            else:
                emitted_includes.add(normalized)

    # Rewrite ObjC class type names to struct form in preserved declarations
    known_classes = set(module.classes.keys())
    for key, val in list(context.items()):
        for cls_name in known_classes:
            if cls_name in val:
                val = re.sub(
                    r'(?<!\w)(?<!struct )' + re.escape(cls_name)
                    + r'(?=\s*\*)',
                    f'struct {cls_name}', val,
                )
        context[key] = val

    env = J2Env(undefined=__import__("jinja2").StrictUndefined)
    rendered = env.from_string(template_str).render(**context)

    out = StringIO()
    out.write("/* Auto-generated by oz_transpile -- do not edit */\n")
    out.write('#include "oz_dispatch.h"\n')
    for dep_stem in dep_stems:
        out.write(f'#include "{dep_stem}_ozh.h"\n')
    # Emit orphan statics not already preserved in the roundtrip
    for sv in orphan.statics:
        if sv.name not in orphan_static_names:
            continue
        decl_str = sv.oz_type.c_param_decl(sv.name)
        init = f" = {sv.init_value}" if sv.init_value is not None else ""
        out.write(f"static {decl_str}{init};\n")
    out.write(rendered)
    return out.getvalue()


def _write_file(path: str, content: str) -> None:
    with open(path, "w") as f:
        f.write(content)
