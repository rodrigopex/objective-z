# SPDX-License-Identifier: Apache-2.0
#
# model.py - Data model for the OZ transpiler.

from __future__ import annotations

import enum
from dataclasses import dataclass, field
from pathlib import Path


class DispatchKind(enum.Enum):
    STATIC = "static"
    PROTOCOL = "protocol"


@dataclass(frozen=True, slots=True)
class InlineAccessor:
    """Describes a Foundation method that can be inlined as direct ivar access."""
    class_name: str
    selector: str
    return_c_type: str
    params: tuple[tuple[str, str], ...]   # ((c_type, name), ...)
    body_lines: tuple[str, ...]           # C statements for the inline body
    func_suffix: str = "fast_"            # appended to C selector name


# Mapping of (class_name, selector) -> InlineAccessor for Foundation methods
# whose bodies are simple ivar accesses suitable for static inline emission.
INLINE_ACCESSORS: dict[tuple[str, str], InlineAccessor] = {}


def _register_inline(acc: InlineAccessor) -> None:
    INLINE_ACCESSORS[(acc.class_name, acc.selector)] = acc


_register_inline(InlineAccessor(
    class_name="OZArray",
    selector="objectAtIndex:",
    return_c_type="struct OZObject *",
    params=(("unsigned int", "index"),),
    body_lines=(
        "if (index >= self->_count) {",
        "\treturn (struct OZObject *)0;",
        "}",
        "return self->_items[index];",
    ),
))

_register_inline(InlineAccessor(
    class_name="OZArray",
    selector="count",
    return_c_type="unsigned int",
    params=(),
    body_lines=(
        "return self->_count;",
    ),
))

_register_inline(InlineAccessor(
    class_name="OZString",
    selector="cString",
    return_c_type="const char *",
    params=(),
    body_lines=(
        "return self->_data;",
    ),
))

_register_inline(InlineAccessor(
    class_name="OZString",
    selector="length",
    return_c_type="unsigned int",
    params=(),
    body_lines=(
        "return self->_length;",
    ),
))

_register_inline(InlineAccessor(
    class_name="OZDictionary",
    selector="count",
    return_c_type="unsigned int",
    params=(),
    body_lines=(
        "return self->_count;",
    ),
))


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
    def is_unretained(self) -> bool:
        return "__unsafe_unretained" in self.raw_qual_type

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
        """Format a C parameter declaration, handling function pointers and arrays."""
        ct = self.c_type
        if self.is_block and "(*)" in ct:
            return ct.replace("(*)", f"(*{name})", 1)
        import re
        m = re.match(r'^(.*?)(\[.+)$', ct)
        if m:
            return f"{m.group(1).rstrip()} {name}{m.group(2)}"
        return f"{ct} {name}"

    @property
    def generic_params(self) -> list[str]:
        """Extract generic type parameters from raw_qual_type.

        "OZArray<id<PXDataProcessor>> *" -> ["id<PXDataProcessor>"]
        "OZDictionary<OZString *, id<Foo>> *" -> ["OZString *", "id<Foo>"]
        "int" -> []
        """
        import re
        qt = self.raw_qual_type
        for qual in ("__strong", "__weak", "__unsafe_unretained",
                      "__autoreleasing", "_Nonnull", "_Nullable",
                      "__kindof"):
            qt = qt.replace(qual, "")
        qt = qt.strip()
        # Find the outermost < ... > after the class name
        m = re.match(r"[A-Za-z_]\w*\s*<(.+)>\s*\*?$", qt)
        if not m:
            return []
        inner = m.group(1).strip()
        # Split on top-level commas (not inside nested < >)
        params: list[str] = []
        depth = 0
        start = 0
        for i, ch in enumerate(inner):
            if ch == "<":
                depth += 1
            elif ch == ">":
                depth -= 1
            elif ch == "," and depth == 0:
                params.append(inner[start:i].strip())
                start = i + 1
        params.append(inner[start:].strip())
        return params

    def _strip_qualifiers(self) -> str:
        import re
        qt = self.raw_qual_type
        for qual in ("__strong", "__weak", "__unsafe_unretained",
                      "__autoreleasing", "_Nonnull", "_Nullable",
                      "__kindof"):
            qt = qt.replace(qual, "")
        # Strip nested generics (e.g. OZArray<OZArray<OZFixedPoint *> *>)
        prev = None
        while prev != qt:
            prev = qt
            qt = re.sub(r"<[^<>]+>", "", qt)
        return qt.strip()


@dataclass(slots=True)
class OZParam:
    name: str
    oz_type: OZType


@dataclass(slots=True)
class OZProperty:
    name: str
    oz_type: OZType
    ivar_name: str | None = None
    is_readonly: bool = False
    is_nonatomic: bool = False
    ownership: str = "strong"
    getter_sel: str | None = None
    setter_sel: str | None = None


@dataclass(slots=True)
class OZIvar:
    name: str
    oz_type: OZType
    access: str = "protected"


@dataclass(slots=True)
class OZMethod:
    selector: str
    return_type: OZType
    params: list[OZParam] = field(default_factory=list)
    is_class_method: bool = False
    body_ast: dict | None = None
    dispatch: DispatchKind = DispatchKind.STATIC
    synthesized_property: OZProperty | None = None


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
    properties: list[OZProperty] = field(default_factory=list)
    class_id: int = -1
    base_depth: int = 0
    verbatim_lines: list[str] = field(default_factory=list)
    user_includes: list[str] = field(default_factory=list)
    functions: list[OZFunction] = field(default_factory=list)
    statics: list[OZStaticVar] = field(default_factory=list)
    source_stem: str = ""
    is_foundation: bool = False


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
    init_value: str | None = None


@dataclass(slots=True)
class OrphanSource:
    stem: str
    functions: list[OZFunction] = field(default_factory=list)
    statics: list[OZStaticVar] = field(default_factory=list)
    verbatim_lines: list[str] = field(default_factory=list)
    user_includes: list[str] = field(default_factory=list)
    source_path: Path | None = None


@dataclass(slots=True)
class OZModule:
    classes: dict[str, OZClass] = field(default_factory=dict)
    protocols: dict[str, OZProtocol] = field(default_factory=dict)
    functions: list[OZFunction] = field(default_factory=list)
    statics: list[OZStaticVar] = field(default_factory=list)
    verbatim_lines: list[str] = field(default_factory=list)
    user_includes: list[str] = field(default_factory=list)
    type_defs: dict[str, str] = field(default_factory=dict)
    diagnostics: list[str] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)
    initialize_classes: list[str] = field(default_factory=list)
    generic_types: dict[str, str] = field(default_factory=dict)
    source_stem: str = ""
    source_path: Path | None = None
    source_paths: dict[str, Path] = field(default_factory=dict)
    orphan_sources: list[OrphanSource] = field(default_factory=list)
