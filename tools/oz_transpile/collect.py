# SPDX-License-Identifier: Apache-2.0
#
# collect.py - Pass 1: Clang JSON AST -> OZModule

from __future__ import annotations

from .model import OZClass, OZFunction, OZIvar, OZMethod, OZModule, OZParam, OZProtocol, OZType


SKIP_CLASSES = frozenset({"Protocol"})


def collect(ast_root: dict) -> OZModule:
    """Walk a Clang JSON AST and extract classes, protocols, methods, ivars."""
    module = OZModule()
    _walk(ast_root, module)
    # Remove auto-generated Clang classes
    for name in SKIP_CLASSES:
        module.classes.pop(name, None)
    return module


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
    elif kind == "FunctionDecl":
        _collect_function(node, module)
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
