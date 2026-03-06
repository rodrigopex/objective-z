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
    def is_object(self) -> bool:
        qt = self._strip_qualifiers()
        if qt == "id" or qt == "instancetype":
            return True
        return qt.endswith("*") and qt[0].isupper()

    @property
    def is_void(self) -> bool:
        return self._strip_qualifiers() == "void"

    @property
    def c_type(self) -> str:
        qt = self._strip_qualifiers()
        if qt == "id" or qt == "instancetype":
            return "struct OZObject *"
        if self.is_object:
            name = qt.rstrip(" *")
            return f"struct {name} *"
        return qt

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
class OZModule:
    classes: dict[str, OZClass] = field(default_factory=dict)
    protocols: dict[str, OZProtocol] = field(default_factory=dict)
    diagnostics: list[str] = field(default_factory=list)
