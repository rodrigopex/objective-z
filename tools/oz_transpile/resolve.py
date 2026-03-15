# SPDX-License-Identifier: Apache-2.0
#
# resolve.py - Pass 2: hierarchy resolution, dispatch classification, class IDs.

from __future__ import annotations

from .model import DispatchKind, OZMethod, OZModule, OZParam, OZType


def resolve(module: OZModule) -> None:
    """Resolve hierarchy, assign class IDs, classify dispatch."""
    _validate_hierarchy(module)
    _synthesize_properties(module)
    _check_duplicate_methods(module)
    _assign_class_ids(module)
    _compute_base_depths(module)
    _classify_dispatch(module)


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
