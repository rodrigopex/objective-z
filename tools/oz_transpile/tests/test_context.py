# SPDX-License-Identifier: Apache-2.0

import os
import tempfile
from pathlib import Path

from oz_transpile.context import build_source_context, _build_impl_context
from oz_transpile.extract import _loc_key, _impl_loc_key
from oz_transpile.model import (
    OZClass,
    OZFunction,
    OZIvar,
    OZMethod,
    OZModule,
    OZParam,
    OZProperty,
    OZStaticVar,
    OZType,
)
from oz_transpile.resolve import resolve

import tree_sitter_objc as tsobjc
from tree_sitter import Language, Parser

_TS_LANG = Language(tsobjc.language())


def _parse(source: str):
    """Parse ObjC source, return (bytes, tree)."""
    raw = source.encode()
    parser = Parser(_TS_LANG)
    tree = parser.parse(raw)
    return raw, tree


def _write_temp(content: str) -> Path:
    """Write content to a temp .m file, return path."""
    f = tempfile.NamedTemporaryFile(suffix=".m", mode="w", delete=False)
    f.write(content)
    f.close()
    return Path(f.name)


def _minimal_module(class_name="Foo", methods=None, ivars=None,
                    statics=None, functions=None, properties=None):
    """Build a minimal resolved module with OZObject -> class_name."""
    m = OZModule()
    m.classes["OZObject"] = OZClass("OZObject", methods=[
        OZMethod("init", OZType("instancetype"), body_ast={
            "kind": "CompoundStmt",
            "inner": [{"kind": "ReturnStmt", "inner": [
                {"kind": "DeclRefExpr",
                 "referencedDecl": {"name": "self"},
                 "type": {"qualType": "OZObject *"}}
            ]}],
        }),
        OZMethod("dealloc", OZType("void"), body_ast={
            "kind": "CompoundStmt", "inner": [],
        }),
    ])
    cls = OZClass(
        class_name,
        superclass="OZObject",
        ivars=ivars or [],
        methods=methods or [],
        statics=statics or [],
        functions=functions or [],
        properties=properties or [],
    )
    m.classes[class_name] = cls
    if functions:
        m.functions.extend(functions)
    resolve(m)
    return m, m.classes[class_name]


# ---------------------------------------------------------------------------
# Include rewriting
# ---------------------------------------------------------------------------


class TestIncludeRewriting:
    def test_oz_import_removed(self):
        path = _write_temp("#import <Foundation/Foundation.h>\n@implementation Foo\n@end\n")
        try:
            m, cls = _minimal_module()
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            key = "_n_1_1"
            assert key in ctx
            assert ctx[key] == ""
        finally:
            os.unlink(path)

    def test_c_include_preserved(self):
        path = _write_temp('#include <zephyr/kernel.h>\n@implementation Foo\n@end\n')
        try:
            m, cls = _minimal_module()
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            key = "_n_1_1"
            assert key in ctx
            assert "zephyr/kernel.h" in ctx[key]
        finally:
            os.unlink(path)


# ---------------------------------------------------------------------------
# Function definitions
# ---------------------------------------------------------------------------


class TestFunctionDefinition:
    def test_function_with_body_ast_transpiled(self):
        path = _write_temp(
            "void my_func(void) { }\n"
            "@implementation Foo\n@end\n"
        )
        try:
            func = OZFunction("my_func", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [],
            })
            m, cls = _minimal_module(functions=[func])
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            key = "_n_1_1"
            assert key in ctx
            assert "my_func" in ctx[key]
        finally:
            os.unlink(path)

    def test_function_without_body_ast_verbatim(self):
        path = _write_temp(
            "void helper(void) { return; }\n"
            "@implementation Foo\n@end\n"
        )
        try:
            m, cls = _minimal_module()
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            key = "_n_1_1"
            assert key in ctx
            assert "void helper(void)" in ctx[key]
        finally:
            os.unlink(path)


# ---------------------------------------------------------------------------
# Declarations
# ---------------------------------------------------------------------------


class TestDeclarations:
    def test_func_prototype_filtered(self):
        path = _write_temp(
            "int printk(const char *fmt, ...);\n"
            "@implementation Foo\n@end\n"
        )
        try:
            m, cls = _minimal_module()
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            key = "_n_1_1"
            assert key in ctx
            assert ctx[key] == ""
        finally:
            os.unlink(path)

    def test_collected_static_filtered(self):
        path = _write_temp(
            "static int _count;\n"
            "@implementation Foo\n@end\n"
        )
        try:
            m, cls = _minimal_module(
                statics=[OZStaticVar("_count", OZType("int"))]
            )
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            key = "_n_1_1"
            assert key in ctx
            assert ctx[key] == ""
        finally:
            os.unlink(path)

    def test_other_declaration_preserved(self):
        path = _write_temp(
            "int global_var = 42;\n"
            "@implementation Foo\n@end\n"
        )
        try:
            m, cls = _minimal_module()
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            key = "_n_1_1"
            assert key in ctx
            assert "global_var" in ctx[key]
        finally:
            os.unlink(path)


# ---------------------------------------------------------------------------
# Method body rendering
# ---------------------------------------------------------------------------


class TestMethodRendering:
    def test_method_body_rendered(self):
        path = _write_temp(
            "@implementation Foo\n"
            "- (void)doWork { }\n"
            "@end\n"
        )
        try:
            m, cls = _minimal_module(methods=[
                OZMethod("doWork", OZType("void"), body_ast={
                    "kind": "CompoundStmt", "inner": [],
                }),
            ])
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            # Find the method key (line 2)
            method_key = "_n_2_1"
            assert method_key in ctx
            assert "Foo_doWork" in ctx[method_key]
        finally:
            os.unlink(path)

    def test_class_method_vs_instance_method(self):
        """Both +greet and -greet should get separate context entries."""
        path = _write_temp(
            "@implementation Foo\n"
            "- (void)greet { }\n"
            "+ (void)greet { }\n"
            "@end\n"
        )
        try:
            m, cls = _minimal_module(methods=[
                OZMethod("greet", OZType("void"), body_ast={
                    "kind": "CompoundStmt", "inner": [],
                }),
                OZMethod("greet", OZType("void"), is_class_method=True,
                         body_ast={
                    "kind": "CompoundStmt", "inner": [],
                }),
            ])
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            # Instance method line 2, class method line 3
            inst_key = "_n_2_1"
            cls_key = "_n_3_1"
            assert inst_key in ctx
            assert cls_key in ctx
            assert "Foo_greet" in ctx[inst_key]
            assert "Foo_cls_greet" in ctx[cls_key]
        finally:
            os.unlink(path)

    def test_method_not_in_module_empty(self):
        """Method in source but not in AST module gets empty context.
        Note: auto-dealloc may be appended to last method key."""
        path = _write_temp(
            "@implementation Foo\n"
            "- (void)unknown { }\n"
            "- (void)also_unknown { }\n"
            "@end\n"
        )
        try:
            m, cls = _minimal_module()  # no methods on Foo
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            method_key = "_n_2_1"
            assert method_key in ctx
            # First unknown method should be empty (auto-dealloc attaches to last)
            assert ctx[method_key] == ""
        finally:
            os.unlink(path)


# ---------------------------------------------------------------------------
# Synthesized accessors
# ---------------------------------------------------------------------------


class TestSynthesizedAccessors:
    def test_synthesized_method_in_preamble(self):
        """Synthesized methods (from @synthesize) go into the _impl_ preamble."""
        path = _write_temp(
            "@implementation Foo\n"
            "@synthesize color = _color;\n"
            "- (void)doWork { }\n"
            "@end\n"
        )
        try:
            prop = OZProperty("color", OZType("int"),
                              ivar_name="_color", is_readonly=True)
            getter = OZMethod("color", OZType("int"),
                              synthesized_property=prop)
            m, cls = _minimal_module(
                ivars=[OZIvar("_color", OZType("int"))],
                methods=[
                    OZMethod("doWork", OZType("void"), body_ast={
                        "kind": "CompoundStmt", "inner": [],
                    }),
                    getter,
                ],
                properties=[prop],
            )
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            # Preamble key (line 1 col 1 for @implementation)
            impl_key = "_impl_1_1"
            assert impl_key in ctx
            preamble = ctx[impl_key]
            assert "Foo_color" in preamble
        finally:
            os.unlink(path)


# ---------------------------------------------------------------------------
# Static variables in preamble
# ---------------------------------------------------------------------------


class TestStaticVarsInPreamble:
    def test_static_var_in_preamble(self):
        path = _write_temp(
            "static int _count;\n"
            "@implementation Foo\n"
            "- (void)doWork { }\n"
            "@end\n"
        )
        try:
            m, cls = _minimal_module(
                statics=[OZStaticVar("_count", OZType("int"))],
                methods=[
                    OZMethod("doWork", OZType("void"), body_ast={
                        "kind": "CompoundStmt", "inner": [],
                    }),
                ],
            )
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            impl_key = "_impl_2_1"
            assert impl_key in ctx
            assert "_count" in ctx[impl_key]
        finally:
            os.unlink(path)

    def test_static_var_with_init(self):
        path = _write_temp(
            "static int _count = 5;\n"
            "@implementation Foo\n@end\n"
        )
        try:
            m, cls = _minimal_module(
                statics=[OZStaticVar("_count", OZType("int"),
                                     init_value="5")],
            )
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            impl_key = "_impl_2_1"
            assert impl_key in ctx
            assert "_count" in ctx[impl_key]
            assert "= 5" in ctx[impl_key]
        finally:
            os.unlink(path)


# ---------------------------------------------------------------------------
# Preamble ordering
# ---------------------------------------------------------------------------


class TestPreambleOrdering:
    def test_statics_before_synthesized(self):
        """Static vars should appear before synthesized accessor methods."""
        path = _write_temp(
            "static int _count;\n"
            "@implementation Foo\n"
            "@synthesize color = _color;\n"
            "@end\n"
        )
        try:
            prop = OZProperty("color", OZType("int"),
                              ivar_name="_color", is_readonly=True)
            getter = OZMethod("color", OZType("int"),
                              synthesized_property=prop)
            m, cls = _minimal_module(
                ivars=[OZIvar("_color", OZType("int"))],
                statics=[OZStaticVar("_count", OZType("int"))],
                methods=[getter],
                properties=[prop],
            )
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            impl_key = "_impl_2_1"
            assert impl_key in ctx
            preamble = ctx[impl_key]
            count_pos = preamble.find("_count")
            color_pos = preamble.find("Foo_color")
            assert count_pos >= 0, "static _count missing from preamble"
            assert color_pos >= 0, "synthesized Foo_color missing from preamble"
            assert count_pos < color_pos, \
                "static vars must come before synthesized methods"
        finally:
            os.unlink(path)


# ---------------------------------------------------------------------------
# Auto-dealloc
# ---------------------------------------------------------------------------


class TestAutoDealloc:
    def test_auto_dealloc_appended_to_last_method(self):
        """Auto-dealloc should be appended to the last method's context value."""
        path = _write_temp(
            "@implementation Foo\n"
            "- (void)doWork { }\n"
            "@end\n"
        )
        try:
            m, cls = _minimal_module(
                ivars=[OZIvar("_name", OZType("OZString *"))],
                methods=[
                    OZMethod("doWork", OZType("void"), body_ast={
                        "kind": "CompoundStmt", "inner": [],
                    }),
                ],
            )
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            method_key = "_n_2_1"
            assert method_key in ctx
            value = ctx[method_key]
            assert "dealloc" in value or "Foo_dealloc" in value
        finally:
            os.unlink(path)

    def test_auto_dealloc_in_preamble_when_no_methods(self):
        """When no methods exist, auto-dealloc goes in preamble."""
        path = _write_temp(
            "@implementation Foo\n"
            "@end\n"
        )
        try:
            m, cls = _minimal_module(
                ivars=[OZIvar("_name", OZType("OZString *"))],
            )
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            impl_key = "_impl_1_1"
            assert impl_key in ctx
            preamble = ctx[impl_key]
            assert "dealloc" in preamble or "Foo_dealloc" in preamble
        finally:
            os.unlink(path)


# ---------------------------------------------------------------------------
# User dealloc
# ---------------------------------------------------------------------------


class TestUserDealloc:
    def test_user_dealloc_method(self):
        path = _write_temp(
            "@implementation Foo\n"
            "- (void)dealloc { }\n"
            "@end\n"
        )
        try:
            m, cls = _minimal_module(
                ivars=[OZIvar("_name", OZType("OZString *"))],
                methods=[
                    OZMethod("dealloc", OZType("void"), body_ast={
                        "kind": "CompoundStmt", "inner": [],
                    }),
                ],
            )
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            method_key = "_n_2_1"
            assert method_key in ctx
            assert "dealloc" in ctx[method_key] or "Foo_dealloc" in ctx[method_key]
        finally:
            os.unlink(path)


# ---------------------------------------------------------------------------
# Unknown class in @implementation
# ---------------------------------------------------------------------------


class TestUnknownClass:
    def test_unknown_class_empty_preamble(self):
        """If class isn't in module, preamble should be empty."""
        path = _write_temp(
            "@implementation Unknown\n"
            "- (void)bar { }\n"
            "@end\n"
        )
        try:
            m, cls = _minimal_module()
            ctx = build_source_context(path, m, [cls], "Foo", "OZObject", False)
            impl_key = "_impl_1_1"
            assert impl_key in ctx
            assert ctx[impl_key] == ""
        finally:
            os.unlink(path)

    def test_unknown_class_emits_diagnostic(self):
        """If class isn't in module, a diagnostic warning should be emitted."""
        path = _write_temp(
            "@implementation Unknown\n"
            "- (void)bar { }\n"
            "@end\n"
        )
        try:
            m, cls = _minimal_module()
            m.diagnostics.clear()
            build_source_context(path, m, [cls], "Foo", "OZObject", False)
            assert any("Unknown" in d for d in m.diagnostics)
            assert any("not found" in d for d in m.diagnostics)
        finally:
            os.unlink(path)


# ---------------------------------------------------------------------------
# Category context
# ---------------------------------------------------------------------------


class TestCategoryContext:
    def test_category_methods_rendered(self):
        """Category @implementation should match methods merged into base class."""
        path = _write_temp(
            "@implementation Car (Maintenance)\n"
            "- (int)mileage { return 100; }\n"
            "- (int)oilCapacity { return 30; }\n"
            "@end\n"
        )
        try:
            # Category methods are merged into the base class by collect.py
            m, cls = _minimal_module(
                class_name="Car",
                methods=[
                    OZMethod("mileage", OZType("int"), body_ast={
                        "kind": "CompoundStmt",
                        "inner": [{"kind": "ReturnStmt", "inner": [
                            {"kind": "IntegerLiteral", "value": "100",
                             "type": {"qualType": "int"}},
                        ]}],
                    }),
                    OZMethod("oilCapacity", OZType("int"), body_ast={
                        "kind": "CompoundStmt",
                        "inner": [{"kind": "ReturnStmt", "inner": [
                            {"kind": "IntegerLiteral", "value": "30",
                             "type": {"qualType": "int"}},
                        ]}],
                    }),
                ],
            )
            ctx = build_source_context(
                path, m, [cls], "Car", "OZObject", False)
            # Preamble key present
            impl_key = "_impl_1_1"
            assert impl_key in ctx
            # Both methods should be rendered
            mileage_key = "_n_2_1"
            oil_key = "_n_3_1"
            assert mileage_key in ctx
            assert oil_key in ctx
            assert "Car_mileage" in ctx[mileage_key]
            assert "Car_oilCapacity" in ctx[oil_key]
        finally:
            os.unlink(path)


# ---------------------------------------------------------------------------
# Pool count function
# ---------------------------------------------------------------------------


class TestPoolCount:
    def test_pool_count_fn_called(self):
        """pool_count_fn is used by _emit_patched_source, not build_source_context.
        Verify it's accepted as parameter without error."""
        path = _write_temp(
            "@implementation Foo\n@end\n"
        )
        try:
            m, cls = _minimal_module()
            ctx = build_source_context(
                path, m, [cls], "Foo", "OZObject", False,
                pool_count_fn=lambda name: 10
            )
            assert "_impl_1_1" in ctx
        finally:
            os.unlink(path)
