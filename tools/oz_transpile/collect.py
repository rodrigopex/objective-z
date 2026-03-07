# SPDX-License-Identifier: Apache-2.0
#
# collect.py - Pass 1: Clang JSON AST -> OZModule

from __future__ import annotations

from pathlib import Path

import tree_sitter_objc as tsobjc
from tree_sitter import Language, Parser

from .model import (OZClass, OZFunction, OZIvar, OZMethod, OZModule, OZParam,
                     OZProtocol, OZStaticVar, OZType)


SKIP_CLASSES = frozenset({"Protocol"})


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
        merged.diagnostics.extend(m.diagnostics)
    return merged


def collect(ast_root: dict) -> OZModule:
    """Walk a Clang JSON AST and extract classes, protocols, methods, ivars."""
    module = OZModule()
    _walk(ast_root, module)
    # Remove auto-generated Clang classes
    for name in SKIP_CLASSES:
        module.classes.pop(name, None)
    _collect_verbatim_lines(ast_root, module)
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
    if "oz_transpile" in file_path:
        return False
    return True


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


def _walk(node: dict, module: OZModule, impl_name: str | None = None) -> None:
    kind = node.get("kind", "")

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
    elif kind == "RecordDecl":
        if _is_user_struct(node):
            _collect_struct_def(node, module)
        return
    elif kind == "VarDecl":
        if _is_from_main_file(node) and node.get("storageClass") == "static":
            _collect_static_var(node, module)
        return

    for child in node.get("inner", []):
        _walk(child, module, impl_name)


def _collect_interface(node: dict, module: OZModule) -> None:
    name = node.get("name", "")
    if not name:
        return

    superclass = node.get("super", {}).get("name") if "super" in node else None
    ivars = []
    protocols = []

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

    if name in module.classes:
        cls = module.classes[name]
        if superclass and not cls.superclass:
            cls.superclass = superclass
        if ivars:
            cls.ivars = ivars
        if protocols:
            cls.protocols = protocols
    else:
        module.classes[name] = OZClass(
            name=name,
            superclass=superclass,
            ivars=ivars,
            protocols=protocols,
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
                cls.methods.append(method)


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
    module.statics.append(OZStaticVar(name=name, oz_type=OZType(qual_type)))


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
