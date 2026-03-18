# SPDX-License-Identifier: Apache-2.0
#
# resolve.py - Pass 2: hierarchy resolution, dispatch classification, class IDs.

from __future__ import annotations

import re

from .model import DispatchKind, OZMethod, OZModule, OZParam, OZType


def resolve(module: OZModule) -> None:
    """Resolve hierarchy, assign class IDs, classify dispatch."""
    _validate_hierarchy(module)
    _synthesize_properties(module)
    _check_duplicate_methods(module)
    _assign_class_ids(module)
    _compute_base_depths(module)
    _classify_dispatch(module)
    _check_protocol_conformance(module)
    _validate_generic_types(module)
    _collect_initialize_classes(module)


def _validate_hierarchy(module: OZModule) -> None:
    """Check for missing superclasses and cycles."""
    for cls in module.classes.values():
        if cls.superclass and cls.superclass not in module.classes:
            module.diagnostics.append(
                f"superclass '{cls.superclass}' of '{cls.name}' not found"
            )

    # Cycle detection
    for cls in module.classes.values():
        visited: set[str] = set()
        cur = cls.name
        while cur:
            if cur in visited:
                raise ValueError(
                    f"inheritance cycle detected: {cls.name} -> ... -> {cur}"
                )
            visited.add(cur)
            parent = module.classes.get(cur)
            cur = parent.superclass if parent else None


def _synthesize_properties(module: OZModule) -> None:
    """Synthesize getter/setter methods for declared properties."""
    for cls in module.classes.values():
        existing_sels = {m.selector for m in cls.methods}
        existing_ivar_names = {iv.name for iv in cls.ivars}
        for prop in cls.properties:
            if prop.ivar_name is None:
                if prop.name in existing_ivar_names:
                    prop.ivar_name = prop.name
                    module.diagnostics.append(
                        f"warning: '@synthesize {prop.name};' uses bare "
                        f"ivar name '{prop.name}' in class '{cls.name}'; "
                        f"prefer '@synthesize {prop.name} = _{prop.name};'")
                else:
                    prop.ivar_name = f"_{prop.name}"

            getter_sel = prop.getter_sel or prop.name
            if getter_sel not in existing_sels:
                getter = OZMethod(
                    selector=getter_sel,
                    return_type=prop.oz_type,
                    synthesized_property=prop,
                )
                cls.methods.append(getter)
                existing_sels.add(getter_sel)

            if not prop.is_readonly:
                setter_sel = prop.setter_sel or f"set{prop.name[0].upper()}{prop.name[1:]}:"
                if setter_sel not in existing_sels:
                    setter = OZMethod(
                        selector=setter_sel,
                        return_type=OZType("void"),
                        params=[OZParam(prop.name, prop.oz_type)],
                        synthesized_property=prop,
                    )
                    cls.methods.append(setter)
                    existing_sels.add(setter_sel)


def _check_duplicate_methods(module: OZModule) -> None:
    """Check for duplicate method selectors within the same class."""
    for cls in module.classes.values():
        seen: dict[str, str] = {}  # selector -> "instance" or "class"
        for m in cls.methods:
            kind = "class" if m.is_class_method else "instance"
            key = f"{kind}:{m.selector}"
            if key in seen:
                module.errors.append(
                    f"duplicate {kind} method '{m.selector}' in '{cls.name}'")
            seen[key] = kind


def _assign_class_ids(module: OZModule) -> None:
    """Assign class_id in topological order (root=0)."""
    assigned: dict[str, int] = {}
    next_id = 0

    def assign(name: str) -> int:
        nonlocal next_id
        if name in assigned:
            return assigned[name]
        cls = module.classes.get(name)
        if cls and cls.superclass and cls.superclass in module.classes:
            assign(cls.superclass)
        assigned[name] = next_id
        if cls:
            cls.class_id = next_id
        next_id += 1
        return assigned[name]

    for name in sorted(module.classes):
        assign(name)


def _compute_base_depths(module: OZModule) -> None:
    """Compute depth of each class (chain length to root)."""
    cache: dict[str, int] = {}

    def depth(name: str) -> int:
        if name in cache:
            return cache[name]
        cls = module.classes.get(name)
        if not cls or not cls.superclass or cls.superclass not in module.classes:
            cache[name] = 0
            return 0
        d = 1 + depth(cls.superclass)
        cache[name] = d
        return d

    for cls in module.classes.values():
        cls.base_depth = depth(cls.name)


def _classify_dispatch(module: OZModule) -> None:
    """Classify methods as STATIC or PROTOCOL.

    PROTOCOL if: selector appears in a protocol, or multiple classes implement it.
    """
    protocol_selectors: set[str] = set()
    for proto in module.protocols.values():
        for m in proto.methods:
            protocol_selectors.add(m.selector)

    # Count how many classes implement each instance selector
    selector_classes: dict[str, set[str]] = {}
    for cls in module.classes.values():
        for m in cls.methods:
            if m.is_class_method:
                continue
            if m.selector not in selector_classes:
                selector_classes[m.selector] = set()
            selector_classes[m.selector].add(cls.name)

    # Selectors that must always be protocol-dispatched (called polymorphically)
    always_protocol = {"dealloc", "init", "isEqual:", "cDescription:maxLength:"}

    for cls in module.classes.values():
        for m in cls.methods:
            if m.is_class_method:
                continue
            if m.selector in protocol_selectors:
                m.dispatch = DispatchKind.PROTOCOL
            elif m.selector in always_protocol:
                m.dispatch = DispatchKind.PROTOCOL
            elif len(selector_classes.get(m.selector, set())) > 1:
                m.dispatch = DispatchKind.PROTOCOL


def _collect_initialize_classes(module: OZModule) -> None:
    """Collect classes that define +initialize, in topological order."""
    sorted_classes = sorted(module.classes.values(), key=lambda c: c.class_id)
    for cls in sorted_classes:
        for m in cls.methods:
            if m.is_class_method and m.selector == "initialize":
                _check_initialize_guard(cls.name, m, module)
                module.initialize_classes.append(cls.name)
                break


def _check_initialize_guard(class_name: str, method: OZMethod,
                            module: OZModule) -> None:
    """Detect Apple-style +initialize guard and emit diagnostic.

    The pattern ``if (self == [ClassName class])`` is unnecessary in
    Objective-Z because +initialize is called exactly once per class
    via SYS_INIT.
    """
    if not method.body_ast:
        return
    if _ast_has_class_message(method.body_ast):
        module.diagnostics.append(
            f"warning: +initialize guard '[{class_name} class]' is "
            f"unnecessary; Objective-Z calls +initialize exactly once "
            f"per class via SYS_INIT")


def _ast_has_class_message(node: dict) -> bool:
    """Recursively check if an AST node contains [ClassName class]."""
    if (node.get("kind") == "ObjCMessageExpr"
            and node.get("selector") == "class"
            and node.get("receiverKind") == "class"):
        return True
    for child in node.get("inner", []):
        if _ast_has_class_message(child):
            return True
    return False


def _check_protocol_conformance(module: OZModule) -> None:
    """Check that classes implementing protocols provide all required methods."""
    for cls in module.classes.values():
        if not cls.protocols:
            continue
        cls_sels = {m.selector for m in cls.methods}
        # Also check inherited methods
        cur = cls
        while cur.superclass and cur.superclass in module.classes:
            cur = module.classes[cur.superclass]
            cls_sels.update(m.selector for m in cur.methods)
        for proto_name in cls.protocols:
            proto = module.protocols.get(proto_name)
            if not proto:
                continue
            for pm in proto.methods:
                if pm.selector not in cls_sels:
                    module.errors.append(
                        f"class '{cls.name}' conforms to protocol "
                        f"'{proto_name}' but does not implement "
                        f"required method '{pm.selector}'")


# ---------------------------------------------------------------------------
# Generic type validation
# ---------------------------------------------------------------------------

_OZ_NS_ALIASES: dict[str, str] = {
    "NSArray": "OZArray", "NSDictionary": "OZDictionary",
    "NSString": "OZString", "NSNumber": "OZNumber",
    "NSObject": "OZObject",
}


def _enrich_model_generics(module: OZModule) -> None:
    """Enrich OZType.raw_qual_type on model objects from source generics."""
    gt = module.generic_types
    if not gt:
        return
    for cls in module.classes.values():
        for ivar in cls.ivars:
            enriched = gt.get(ivar.name)
            if enriched and not ivar.oz_type.generic_params:
                ivar.oz_type.raw_qual_type = enriched
        for prop in cls.properties:
            enriched = gt.get(prop.name)
            if enriched and not prop.oz_type.generic_params:
                prop.oz_type.raw_qual_type = enriched
        for method in cls.methods:
            ret_key = f"__return:{method.selector}"
            enriched = gt.get(ret_key)
            if enriched and not method.return_type.generic_params:
                method.return_type.raw_qual_type = enriched
            for param in method.params:
                enriched = gt.get(param.name)
                if enriched and not param.oz_type.generic_params:
                    param.oz_type.raw_qual_type = enriched
    for func in module.functions:
        ret_key = f"__return:{func.name}"
        enriched = gt.get(ret_key)
        if enriched and not func.return_type.generic_params:
            func.return_type.raw_qual_type = enriched
        for param in func.params:
            enriched = gt.get(param.name)
            if enriched and not param.oz_type.generic_params:
                param.oz_type.raw_qual_type = enriched


def _validate_generic_types(module: OZModule) -> None:
    """Validate generic type parameters in declarations and assignments."""
    _enrich_model_generics(module)
    for cls in module.classes.values():
        for method in cls.methods:
            if not method.body_ast:
                continue
            generic_vars: dict[str, list[str]] = {}
            _walk_generic_validation(method.body_ast, module, generic_vars)
        for func in cls.functions:
            if not func.body_ast:
                continue
            generic_vars: dict[str, list[str]] = {}
            _walk_generic_validation(func.body_ast, module, generic_vars)
    for func in module.functions:
        if not func.body_ast:
            continue
        generic_vars: dict[str, list[str]] = {}
        _walk_generic_validation(func.body_ast, module, generic_vars)
    for orphan in module.orphan_sources:
        for func in orphan.functions:
            if not func.body_ast:
                continue
            generic_vars: dict[str, list[str]] = {}
            _walk_generic_validation(func.body_ast, module, generic_vars)


def _walk_generic_validation(node: dict, module: OZModule,
                              generic_vars: dict[str, list[str]]) -> None:
    """Walk AST looking for generic type violations."""
    kind = node.get("kind", "")

    if kind == "VarDecl":
        qt = node.get("type", {}).get("qualType", "")
        params = OZType(qt).generic_params
        name = node.get("name", "")
        # Clang strips generics from qualType; fall back to source extraction
        if not params and name and name in module.generic_types:
            qt = module.generic_types[name]
            params = OZType(qt).generic_params
        if params and name:
            generic_vars[name] = params
            for child in node.get("inner", []):
                init = _unwrap_implicit_cast(child)
                ikind = init.get("kind", "")
                if ikind == "ObjCArrayLiteral":
                    _validate_array_generics(init, params, qt, module)
                elif ikind == "ObjCDictionaryLiteral":
                    _validate_dict_generics(init, params, qt, module)
        # Still recurse into VarDecl inner for nested expressions
        for child in node.get("inner", []):
            _walk_generic_validation(child, module, generic_vars)
        return

    if kind == "BinaryOperator" and node.get("opcode") == "=":
        inner = node.get("inner", [])
        if len(inner) >= 2:
            lhs = _unwrap_implicit_cast(inner[0])
            rhs = _unwrap_implicit_cast(inner[1])
            params = _generic_params_from_expr(lhs, generic_vars)
            if params:
                lhs_qt = lhs.get("type", {}).get("qualType", "")
                rkind = rhs.get("kind", "")
                if rkind == "ObjCArrayLiteral":
                    _validate_array_generics(rhs, params, lhs_qt, module)
                elif rkind == "ObjCDictionaryLiteral":
                    _validate_dict_generics(rhs, params, lhs_qt, module)

    for child in node.get("inner", []):
        _walk_generic_validation(child, module, generic_vars)


def _validate_array_generics(node: dict, params: list[str],
                              var_qt: str, module: OZModule) -> None:
    """Validate each element of an array literal against the generic constraint."""
    if not params:
        return
    constraint = params[0]
    if constraint == "id":
        return
    container = _extract_class_name(var_qt) or var_qt
    for elem in node.get("inner", []):
        elem_type = _original_type(elem)
        if not elem_type or elem_type == "id":
            continue
        if not _satisfies_constraint(elem_type, constraint, module):
            elem_name = _extract_class_name(elem_type) or elem_type
            module.errors.append(
                f"generic type mismatch: '{elem_name}' does not satisfy "
                f"constraint '{constraint}' "
                f"(required by '{container}<{constraint}>')")


def _validate_dict_generics(node: dict, params: list[str],
                             var_qt: str, module: OZModule) -> None:
    """Validate key/value pairs of a dictionary literal against generic constraints."""
    if len(params) < 2:
        return
    key_constraint = params[0]
    val_constraint = params[1]
    container = _extract_class_name(var_qt) or var_qt
    inner = node.get("inner", [])
    for i in range(0, len(inner), 2):
        if key_constraint != "id":
            key_type = _original_type(inner[i])
            if key_type and key_type != "id":
                if not _satisfies_constraint(key_type, key_constraint, module):
                    key_name = _extract_class_name(key_type) or key_type
                    module.errors.append(
                        f"generic type mismatch: key '{key_name}' does not "
                        f"satisfy constraint '{key_constraint}' "
                        f"(required by '{container}<{key_constraint}, "
                        f"{val_constraint}>')")
        if i + 1 < len(inner) and val_constraint != "id":
            val_type = _original_type(inner[i + 1])
            if val_type and val_type != "id":
                if not _satisfies_constraint(val_type, val_constraint, module):
                    val_name = _extract_class_name(val_type) or val_type
                    module.errors.append(
                        f"generic type mismatch: value '{val_name}' does not "
                        f"satisfy constraint '{val_constraint}' "
                        f"(required by '{container}<{key_constraint}, "
                        f"{val_constraint}>')")


def _satisfies_constraint(elem_type: str, constraint: str,
                           module: OZModule) -> bool:
    """Check if an element type satisfies a generic constraint."""
    proto_match = re.match(r"id<(\w+)>", constraint)
    if proto_match:
        required_proto = proto_match.group(1)
        # Element is id<Proto> — check protocol match
        elem_proto = re.match(r"id<(\w+)>", elem_type)
        if elem_proto:
            return elem_proto.group(1) == required_proto
        # Element is a class — check protocol conformance
        elem_class = _extract_class_name(elem_type)
        if not elem_class:
            return True
        return _class_conforms_to(elem_class, required_proto, module)

    # Class constraint (e.g. "OZString *")
    constraint_class = constraint.rstrip(" *").strip()
    elem_class = _extract_class_name(elem_type)
    if not elem_class:
        return True
    return _is_same_or_subclass(elem_class, constraint_class, module)


def _class_conforms_to(class_name: str, protocol: str,
                        module: OZModule) -> bool:
    """Check if a class conforms to a protocol (including inherited protocols)."""
    name = _normalize_class(class_name)
    visited: set[str] = set()
    while name and name not in visited:
        visited.add(name)
        cls = module.classes.get(name)
        if not cls:
            return False
        if protocol in cls.protocols:
            return True
        name = cls.superclass
    return False


def _is_same_or_subclass(class_name: str, target: str,
                          module: OZModule) -> bool:
    """Check if class_name is target or a subclass of target."""
    name = _normalize_class(class_name)
    target = _normalize_class(target)
    visited: set[str] = set()
    while name and name not in visited:
        if name == target:
            return True
        visited.add(name)
        cls = module.classes.get(name)
        if not cls:
            return False
        name = cls.superclass
    return False


def _normalize_class(name: str) -> str:
    """Normalize NS* aliases to OZ* names."""
    return _OZ_NS_ALIASES.get(name, name)


def _extract_class_name(qt: str) -> str | None:
    """Extract class name from a qualType like 'PXFoo *'."""
    qt = qt.strip()
    if qt.startswith("id"):
        return None
    m = re.match(r"([A-Za-z_]\w*)", qt)
    return m.group(1) if m else None


def _original_type(node: dict) -> str:
    """Get the pre-cast type of an expression (unwrap ImplicitCastExpr)."""
    while node.get("kind") == "ImplicitCastExpr":
        inner = node.get("inner", [])
        if not inner:
            break
        node = inner[0]
    return node.get("type", {}).get("qualType", "")


def _unwrap_implicit_cast(node: dict) -> dict:
    """Unwrap ImplicitCastExpr nodes to get the underlying expression."""
    while node.get("kind") == "ImplicitCastExpr":
        inner = node.get("inner", [])
        if not inner:
            break
        node = inner[0]
    return node


def _generic_params_from_expr(node: dict,
                               generic_vars: dict[str, list[str]]) -> list[str]:
    """Get generic params from an expression (LHS of assignment)."""
    if node.get("kind") == "DeclRefExpr":
        qt = node.get("type", {}).get("qualType", "")
        params = OZType(qt).generic_params
        if params:
            return params
        name = node.get("referencedDecl", {}).get("name", "")
        return generic_vars.get(name, [])
    return []
