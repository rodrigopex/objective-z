# SPDX-License-Identifier: Apache-2.0
#
# collect.py - Pass 1: Clang JSON AST -> OZModule

from __future__ import annotations

from pathlib import Path

import tree_sitter_objc as tsobjc
from tree_sitter import Language, Parser

from .model import (OZClass, OZFunction, OZIvar, OZMethod, OZModule, OZParam,
                     OZProperty, OZProtocol, OZStaticVar, OZType)


SKIP_CLASSES = frozenset({"Protocol"})

_UNSUPPORTED_METHOD_SELECTORS = frozenset({
    "forwardInvocation:",
    "addObserver:forKeyPath:options:context:",
    "removeObserver:forKeyPath:",
    "removeObserver:forKeyPath:context:",
    "observeValueForKeyPath:ofObject:change:context:",
    "willChangeValueForKey:",
    "didChangeValueForKey:",
})

_UNSUPPORTED_AST_KINDS = frozenset({
    "ObjCAtTryStmt",
})


def merge_modules(modules: list[OZModule]) -> OZModule:
    """Merge multiple OZModules into one, combining class data."""
    merged = OZModule()
    for m in modules:
        for name, cls in m.classes.items():
            if name in merged.classes:
                existing = merged.classes[name]
                if cls.superclass and not existing.superclass:
                    existing.superclass = cls.superclass
                if cls.ivars and not existing.ivars:
                    existing.ivars = cls.ivars
                if cls.protocols:
                    existing.protocols = list(
                        dict.fromkeys(existing.protocols + cls.protocols))
                if cls.properties and not existing.properties:
                    existing.properties = cls.properties
                for method in cls.methods:
                    for i, ex in enumerate(existing.methods):
                        if ex.selector == method.selector:
                            if method.body_ast or not ex.body_ast:
                                existing.methods[i] = method
                            break
                    else:
                        existing.methods.append(method)
            else:
                merged.classes[name] = cls
        merged.protocols.update(m.protocols)
        merged.functions.extend(m.functions)
        merged.statics.extend(m.statics)
        for line in m.verbatim_lines:
            if line not in merged.verbatim_lines:
                merged.verbatim_lines.append(line)
        merged.type_defs.update(m.type_defs)
        merged.diagnostics.extend(m.diagnostics)
        merged.errors.extend(m.errors)
    return merged


def collect(ast_root: dict) -> OZModule:
    """Walk a Clang JSON AST and extract classes, protocols, methods, ivars."""
    module = OZModule()
    _walk(ast_root, module)
    # Remove auto-generated Clang classes
    for name in SKIP_CLASSES:
        module.classes.pop(name, None)
    _collect_verbatim_lines(ast_root, module)
    _check_unsupported_features(ast_root, module)
    return module


def _is_from_main_file(node: dict) -> bool:
    """Check if a node is defined in the main source file, not an included header."""
    loc = node.get("loc", {})
    if "includedFrom" in loc:
        return False
    if "expansionLoc" in loc:
        return "includedFrom" not in loc["expansionLoc"]
    return True


_SYSTEM_PATH_SEGMENTS = frozenset({"/zephyr/", "/sdk/", "/clang/", "/picolibc/",
                                   "/cmsis/", "/CMSIS/", "/sys-include/"})


def _is_user_struct(node: dict) -> bool:
    """Check if a RecordDecl is a user-defined struct (not from system headers)."""
    if not node.get("completeDefinition"):
        return False
    name = node.get("name", "")
    if not name:
        return False
    loc = node.get("loc", {})
    file_path = loc.get("file", "")
    if not file_path:
        return False
    if any(p in file_path for p in _SYSTEM_PATH_SEGMENTS):
        return False
    if "oz_transpile" in file_path or "/stubs/" in file_path:
        return False
    return True


def _is_stub_source(path: str) -> bool:
    """Check if a path belongs to oz_transpile stubs."""
    return "oz_transpile" in path or "/stubs/" in path


def _is_oz_transpile_type(node: dict, last_file: str = "") -> bool:
    """Check if a node is a named type definition from oz_transpile stubs.

    Clang JSON AST omits 'file' in 'loc' when it is the same as the previous
    node.  ``last_file`` carries the last explicitly seen file path so we can
    still identify stubs nodes that lack a 'file' key.
    """
    name = node.get("name", "")
    if not name:
        return False
    loc = node.get("loc", {})
    file_path = loc.get("file", "") or last_file
    included_from = loc.get("includedFrom", {}).get("file", "")
    return _is_stub_source(file_path) or _is_stub_source(included_from)


def _collect_enum_def(node: dict, module: OZModule) -> None:
    """Reconstruct an enum definition from a Clang EnumDecl AST node."""
    name = node.get("name", "")
    consts = []
    for i, child in enumerate(node.get("inner", [])):
        if child.get("kind") == "EnumConstantDecl":
            cname = child.get("name", "")
            inner = child.get("inner", [])
            val = None
            if inner and inner[0].get("kind") in ("IntegerLiteral",
                                                    "ConstantExpr"):
                val = inner[0].get("value")
            if val is not None:
                consts.append(f"\t{cname} = {val},")
            elif i == 0:
                consts.append(f"\t{cname} = 0,")
            else:
                consts.append(f"\t{cname},")
    if not consts:
        return
    definition = f"enum {name} {{\n" + "\n".join(consts) + "\n};"
    module.type_defs[f"enum {name}"] = definition


def _collect_union_def(node: dict, module: OZModule) -> None:
    """Reconstruct a union definition from a Clang RecordDecl AST node."""
    name = node.get("name", "")
    tag = node.get("tagUsed", "union")
    fields = []
    for child in node.get("inner", []):
        if child.get("kind") == "FieldDecl":
            fname = child.get("name", "")
            ftype = child.get("type", {}).get("qualType", "")
            fields.append(f"\t{ftype} {fname};")
    if not fields:
        return
    definition = f"{tag} {name} {{\n" + "\n".join(fields) + "\n};"
    module.type_defs[f"{tag} {name}"] = definition


def _collect_struct_def(node: dict, module: OZModule) -> None:
    """Reconstruct a struct definition from a Clang RecordDecl AST node."""
    name = node.get("name", "")
    tag = node.get("tagUsed", "struct")
    fields = []
    for child in node.get("inner", []):
        if child.get("kind") == "FieldDecl":
            fname = child.get("name", "")
            ftype = child.get("type", {}).get("qualType", "")
            fields.append(f"\t{ftype} {fname};")
    if not fields:
        return
    definition = f"{tag} {name} {{\n" + "\n".join(fields) + "\n};"
    if definition not in module.verbatim_lines:
        module.verbatim_lines.append(definition)


def _walk(node: dict, module: OZModule, impl_name: str | None = None,
          last_file: str = "") -> None:
    kind = node.get("kind", "")

    # Track current file — Clang omits 'file' when unchanged from previous node
    loc = node.get("loc", {})
    if "file" in loc:
        last_file = loc["file"]

    if kind == "ObjCInterfaceDecl":
        _collect_interface(node, module)
    elif kind == "ObjCImplementationDecl":
        _collect_implementation(node, module)
        return  # children handled inside
    elif kind == "ObjCProtocolDecl":
        _collect_protocol(node, module)
        return
    elif kind == "ObjCCategoryDecl":
        _collect_category(node, module)
        return
    elif kind == "ObjCCategoryImplDecl":
        _collect_category(node, module)
        return
    elif kind == "FunctionDecl":
        if _is_from_main_file(node):
            _collect_function(node, module)
        return
    elif kind == "EnumDecl":
        if _is_oz_transpile_type(node, last_file):
            _collect_enum_def(node, module)
        return
    elif kind == "RecordDecl":
        if _is_oz_transpile_type(node, last_file):
            tag = node.get("tagUsed", "struct")
            if tag == "union":
                _collect_union_def(node, module)
        elif _is_user_struct(node):
            _collect_struct_def(node, module)
        return
    elif kind == "VarDecl":
        if _is_from_main_file(node) and node.get("storageClass") == "static":
            _collect_static_var(node, module)
        return

    for child in node.get("inner", []):
        child_loc = child.get("loc", {})
        if "file" in child_loc:
            last_file = child_loc["file"]
        _walk(child, module, impl_name, last_file)


def _collect_property(node: dict, module: OZModule) -> OZProperty | None:
    """Collect an ObjCPropertyDecl into an OZProperty."""
    name = node.get("name", "")
    qual_type = node.get("type", {}).get("qualType", "")
    is_readonly = node.get("readonly", False)
    is_nonatomic = node.get("nonatomic", False)

    if node.get("weak"):
        module.errors.append(
            f"'weak' property '{name}' is not supported; "
            f"use 'unsafe_unretained' instead")
        return None

    ownership = "strong"
    if node.get("unsafe_unretained"):
        ownership = "unsafe_unretained"
    elif node.get("assign") and not node.get("strong"):
        ownership = "assign"

    getter_sel = None
    getter_node = node.get("getter")
    if isinstance(getter_node, dict):
        getter_sel = getter_node.get("name")

    setter_sel = None
    setter_node = node.get("setter")
    if isinstance(setter_node, dict):
        setter_sel = setter_node.get("name")

    return OZProperty(
        name=name,
        oz_type=OZType(qual_type),
        is_readonly=is_readonly,
        is_nonatomic=is_nonatomic,
        ownership=ownership,
        getter_sel=getter_sel,
        setter_sel=setter_sel,
    )


def _link_property_impl(node: dict, cls: OZClass) -> None:
    """Link an ObjCPropertyImplDecl to its OZProperty, setting the ivar name."""
    prop_decl = node.get("propertyDecl", {})
    prop_name = prop_decl.get("name", "")
    if not prop_name:
        return

    ivar_decl = node.get("ivarDecl", {})
    ivar_name = ivar_decl.get("name", "")

    for prop in cls.properties:
        if prop.name == prop_name:
            if ivar_name:
                prop.ivar_name = ivar_name
            return


def _collect_interface(node: dict, module: OZModule) -> None:
    name = node.get("name", "")
    if not name:
        return

    superclass = node.get("super", {}).get("name") if "super" in node else None
    ivars = []
    protocols = []
    properties = []

    for child in node.get("inner", []):
        ckind = child.get("kind", "")
        if ckind == "ObjCIvarDecl":
            ivar_name = child.get("name", "")
            qual_type = child.get("type", {}).get("qualType", "")
            ivars.append(OZIvar(ivar_name, OZType(qual_type)))
        elif ckind == "ObjCProtocol":
            proto_name = child.get("name", "")
            if proto_name:
                protocols.append(proto_name)
        elif ckind == "ObjCPropertyDecl":
            prop = _collect_property(child, module)
            if prop:
                properties.append(prop)

    if name in module.classes:
        cls = module.classes[name]
        if superclass and not cls.superclass:
            cls.superclass = superclass
        if ivars:
            cls.ivars = ivars
        if protocols:
            cls.protocols = protocols
        if properties:
            cls.properties = properties
    else:
        module.classes[name] = OZClass(
            name=name,
            superclass=superclass,
            ivars=ivars,
            protocols=protocols,
            properties=properties,
        )


def _collect_implementation(node: dict, module: OZModule) -> None:
    name = node.get("name", "")
    if not name:
        return

    superclass = node.get("super", {}).get("name") if "super" in node else None

    if name not in module.classes:
        module.classes[name] = OZClass(name=name, superclass=superclass)
    cls = module.classes[name]
    if superclass and not cls.superclass:
        cls.superclass = superclass

    for child in node.get("inner", []):
        ckind = child.get("kind", "")
        if ckind == "ObjCIvarDecl":
            ivar_name = child.get("name", "")
            qual_type = child.get("type", {}).get("qualType", "")
            cls.ivars.append(OZIvar(ivar_name, OZType(qual_type)))
        elif ckind == "ObjCMethodDecl":
            if child.get("isImplicit"):
                continue
            method = _collect_method(child)
            if method:
                if method.selector in _UNSUPPORTED_METHOD_SELECTORS:
                    module.errors.append(
                        f"'{method.selector}' is not supported "
                        f"(class '{name}')")
                    continue
                if method.body_ast:
                    _collect_block_vars(method.body_ast, module)
                cls.methods.append(method)
        elif ckind == "ObjCPropertyImplDecl":
            _link_property_impl(child, cls)


def _collect_method(node: dict) -> OZMethod | None:
    selector = node.get("name", "")
    if not selector:
        return None

    is_class = node.get("instance", True) is False
    ret_type = OZType(node.get("returnType", {}).get("qualType", "void"))

    params = []
    body_ast = None
    for child in node.get("inner", []):
        ckind = child.get("kind", "")
        if ckind == "ParmVarDecl":
            pname = child.get("name", "")
            ptype = OZType(child.get("type", {}).get("qualType", ""))
            params.append(OZParam(pname, ptype))
        elif ckind == "CompoundStmt":
            body_ast = child

    return OZMethod(
        selector=selector,
        return_type=ret_type,
        params=params,
        is_class_method=is_class,
        body_ast=body_ast,
    )


def _collect_protocol(node: dict, module: OZModule) -> None:
    name = node.get("name", "")
    if not name:
        return

    methods = []
    for child in node.get("inner", []):
        if child.get("kind") == "ObjCMethodDecl":
            m = _collect_method(child)
            if m:
                methods.append(m)

    module.protocols[name] = OZProtocol(name=name, methods=methods)


def _collect_category(node: dict, module: OZModule) -> None:
    """Collect category methods and merge into the target class."""
    interface = node.get("interface", {})
    class_name = interface.get("name", "") if isinstance(interface, dict) else ""
    if not class_name:
        return

    if class_name not in module.classes:
        module.classes[class_name] = OZClass(name=class_name)
    cls = module.classes[class_name]

    for child in node.get("inner", []):
        if child.get("kind") == "ObjCMethodDecl":
            method = _collect_method(child)
            if method:
                # Deduplicate: replace existing method if new has body
                for i, existing in enumerate(cls.methods):
                    if existing.selector == method.selector:
                        if method.body_ast or not existing.body_ast:
                            cls.methods[i] = method
                        break
                else:
                    cls.methods.append(method)


def _collect_function(node: dict, module: OZModule) -> None:
    """Collect a top-level C function declaration with body."""
    name = node.get("name", "")
    if not name:
        return

    # Only collect functions that have a body (CompoundStmt)
    body_ast = None
    params = []
    for child in node.get("inner", []):
        ckind = child.get("kind", "")
        if ckind == "ParmVarDecl":
            pname = child.get("name", "")
            ptype = OZType(child.get("type", {}).get("qualType", ""))
            params.append(OZParam(pname, ptype))
        elif ckind == "CompoundStmt":
            body_ast = child

    if body_ast is None:
        return  # Forward declaration only, skip

    _collect_block_vars(body_ast, module)

    ret_type = OZType(node.get("type", {}).get("qualType", "int ()").split("(")[0].strip())
    module.functions.append(OZFunction(
        name=name,
        return_type=ret_type,
        params=params,
        body_ast=body_ast,
    ))


def _collect_static_var(node: dict, module: OZModule) -> None:
    """Collect a file-scope static variable declaration."""
    name = node.get("name", "")
    if not name:
        return
    qual_type = node.get("type", {}).get("qualType", "")
    if not qual_type:
        return
    init_value = _extract_init_value(node)
    module.statics.append(OZStaticVar(name=name, oz_type=OZType(qual_type),
                                      init_value=init_value))


def _has_blocks_attr(node: dict) -> bool:
    """Check if a VarDecl has a BlocksAttr child (__block qualifier)."""
    return any(c.get("kind") == "BlocksAttr" for c in node.get("inner", []))


def _extract_init_value(node: dict) -> str | None:
    """Extract a simple literal init value from a VarDecl."""
    for child in node.get("inner", []):
        kind = child.get("kind", "")
        if kind in ("IntegerLiteral", "FloatingLiteral"):
            return child.get("value")
        if kind == "ImplicitCastExpr":
            return _extract_init_value(child)
        if kind == "UnaryOperator" and child.get("opcode") == "-":
            inner_val = _extract_init_value(child)
            if inner_val is not None:
                return f"-{inner_val}"
    return None


def _collect_block_vars(node: dict, module: OZModule) -> None:
    """Walk an AST subtree and collect __block VarDecls and captured block-typed
    vars as file-scope statics."""
    kind = node.get("kind", "")
    if kind == "VarDecl" and _has_blocks_attr(node):
        name = node.get("name", "")
        qual_type = node.get("type", {}).get("qualType", "")
        if name and qual_type:
            init_value = _extract_init_value(node)
            module.statics.append(OZStaticVar(name=name, oz_type=OZType(qual_type),
                                              init_value=init_value))
    for child in node.get("inner", []):
        _collect_block_vars(child, module)


def _check_unsupported_features(ast_root: dict, module: OZModule) -> None:
    """Scan AST for unsupported language features and report errors."""
    _scan_unsupported(ast_root, module)


def _scan_unsupported(node: dict, module: OZModule) -> None:
    """Recursively scan for unsupported AST node kinds."""
    kind = node.get("kind", "")
    if kind in _UNSUPPORTED_AST_KINDS:
        module.errors.append(
            "@try/@catch/@finally is not supported")
        return
    for child in node.get("inner", []):
        _scan_unsupported(child, module)


_TS_LANG = Language(tsobjc.language())


def _find_main_file(ast_root: dict) -> str | None:
    """Extract the main source file path from AST loc fields."""
    for child in ast_root.get("inner", []):
        loc = child.get("loc", {})
        f = loc.get("file", "")
        if f and "includedFrom" not in loc:
            return f
    return None


def _collect_verbatim_lines(ast_root: dict, module: OZModule) -> None:
    """Scan original source for top-level expression statements (macro calls)."""
    main_file = _find_main_file(ast_root)
    if not main_file:
        return
    path = Path(main_file)
    if not path.is_file():
        return

    source = path.read_bytes()
    parser = Parser(_TS_LANG)
    tree = parser.parse(source)

    children = tree.root_node.children
    for i, child in enumerate(children):
        if child.type == "expression_statement":
            module.verbatim_lines.append(
                source[child.start_byte:child.end_byte].decode())
        elif child.type == "declaration" and _has_struct_definition(child):
            module.verbatim_lines.append(
                source[child.start_byte:child.end_byte].decode())
        elif child.type == "struct_specifier" and _has_field_list(child):
            end = child.end_byte
            if i + 1 < len(children) and children[i + 1].type == ";":
                end = children[i + 1].end_byte
            module.verbatim_lines.append(
                source[child.start_byte:end].decode())


def _has_struct_definition(node) -> bool:
    """Check if a tree-sitter declaration node contains a struct definition."""
    for child in node.children:
        if child.type == "struct_specifier":
            if _has_field_list(child):
                return True
    return False


def _has_field_list(node) -> bool:
    """Check if a tree-sitter struct_specifier has a field_declaration_list."""
    for child in node.children:
        if child.type == "field_declaration_list":
            return True
    return False
