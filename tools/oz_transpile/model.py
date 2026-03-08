# SPDX-License-Identifier: Apache-2.0
#
# model.py - Data model for the OZ transpiler.

from __future__ import annotations

import enum
from dataclasses import dataclass, field


class DispatchKind(enum.Enum):
    STATIC = "static"
    PROTOCOL = "protocol"


@dataclass(slots=True)
class OZType:
    raw_qual_type: str

    @property
    def is_block(self) -> bool:
        return "(^)" in self._strip_qualifiers()

    _NON_OBJECT_TYPES = frozenset({"BOOL"})

    @property
    def is_object(self) -> bool:
        qt = self._strip_qualifiers()
        if self.is_block:
            return False
        if qt == "id" or qt == "instancetype":
            return True
        if qt.endswith("*") and qt[0].isupper():
            name = qt.rstrip(" *")
            return name not in OZType._NON_OBJECT_TYPES
        return False

    @property
    def is_void(self) -> bool:
        return self._strip_qualifiers() == "void"

    @property
    def c_type(self) -> str:
        qt = self._strip_qualifiers()
        if self.is_block:
            return self._block_to_fptr(qt)
        if qt == "id" or qt == "instancetype":
            return "struct OZObject *"
        if qt == "id *":
            return "struct OZObject **"
        if self.is_object:
            name = qt.rstrip(" *")
            return f"struct {name} *"
        return qt

    @staticmethod
    def _block_to_fptr(qt: str) -> str:
        """Convert block type to C function pointer.

        "void (^)(id, unsigned int, BOOL *)" -> "void (*)(struct OZObject *, unsigned int, BOOL *)"
        """
        caret = qt.index("(^)")
        ret_part = qt[:caret].strip()
        ret_c = OZType(ret_part).c_type if ret_part else "void"
        # Extract param list after (^)
        rest = qt[caret + 3:].strip()
        if rest.startswith("(") and rest.endswith(")"):
            param_str = rest[1:-1]
            if param_str.strip():
                parts = [p.strip() for p in param_str.split(",")]
                c_parts = []
                for p in parts:
                    c_parts.append(OZType(p).c_type)
                params_c = ", ".join(c_parts)
            else:
                params_c = "void"
        else:
            params_c = "void"
        return f"{ret_c} (*)({params_c})"

    def c_param_decl(self, name: str) -> str:
        """Format a C parameter declaration, handling function pointers."""
        ct = self.c_type
        if self.is_block and "(*)" in ct:
            return ct.replace("(*)", f"(*{name})", 1)
        return f"{ct} {name}"

    def _strip_qualifiers(self) -> str:
        qt = self.raw_qual_type
        for qual in ("__strong", "__weak", "__unsafe_unretained",
                      "__autoreleasing", "_Nonnull", "_Nullable",
                      "__kindof"):
            qt = qt.replace(qual, "")
        return qt.strip()


@dataclass(slots=True)
class OZParam:
    name: str
    oz_type: OZType


@dataclass(slots=True)
class OZIvar:
    name: str
    oz_type: OZType


@dataclass(slots=True)
class OZMethod:
    selector: str
    return_type: OZType
    params: list[OZParam] = field(default_factory=list)
    is_class_method: bool = False
    body_ast: dict | None = None
    dispatch: DispatchKind = DispatchKind.STATIC


@dataclass(slots=True)
class OZProtocol:
    name: str
    methods: list[OZMethod] = field(default_factory=list)


@dataclass(slots=True)
class OZClass:
    name: str
    superclass: str | None = None
    ivars: list[OZIvar] = field(default_factory=list)
    methods: list[OZMethod] = field(default_factory=list)
    protocols: list[str] = field(default_factory=list)
    class_id: int = -1
    base_depth: int = 0


@dataclass(slots=True)
class OZFunction:
    name: str
    return_type: OZType
    params: list[OZParam] = field(default_factory=list)
    body_ast: dict | None = None


@dataclass(slots=True)
class OZStaticVar:
    name: str
    oz_type: OZType


@dataclass(slots=True)
class OZModule:
    classes: dict[str, OZClass] = field(default_factory=dict)
    protocols: dict[str, OZProtocol] = field(default_factory=dict)
    functions: list[OZFunction] = field(default_factory=list)
    statics: list[OZStaticVar] = field(default_factory=list)
    verbatim_lines: list[str] = field(default_factory=list)
    type_defs: dict[str, str] = field(default_factory=dict)
    diagnostics: list[str] = field(default_factory=list)
