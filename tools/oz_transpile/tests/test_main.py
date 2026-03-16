# SPDX-License-Identifier: Apache-2.0

from oz_transpile.__main__ import (
    _associate_module_items_with_class,
    _source_stem,
    parse_pool_sizes,
)
from oz_transpile.model import (OZClass, OZFunction, OZMethod, OZModule,
                                OZStaticVar, OZType)


class TestParsePoolSizes:
    def test_empty_string(self):
        assert parse_pool_sizes("") == {}

    def test_single_pair(self):
        assert parse_pool_sizes("OZLed=4") == {"OZLed": 4}

    def test_multiple_pairs(self):
        result = parse_pool_sizes("OZLed=4,OZRgbLed=2")
        assert result == {"OZLed": 4, "OZRgbLed": 2}

    def test_whitespace_handling(self):
        result = parse_pool_sizes(" OZLed = 4 , OZRgbLed = 2 ")
        assert result == {"OZLed": 4, "OZRgbLed": 2}

    def test_trailing_comma_ignored(self):
        result = parse_pool_sizes("OZLed=4,")
        assert result == {"OZLed": 4}

    def test_no_equals_entry_ignored(self):
        result = parse_pool_sizes("OZLed=4,bogus,OZBar=1")
        assert result == {"OZLed": 4, "OZBar": 1}


class TestSourceStem:
    def test_basic_path(self):
        assert _source_stem("/a/b/Producer.m") == "Producer"

    def test_nested_path(self):
        assert _source_stem("/home/user/src/models/Sensor.m") == "Sensor"

    def test_m_extension(self):
        assert _source_stem("MyClass.m") == "MyClass"

    def test_c_extension(self):
        assert _source_stem("helpers.c") == "helpers"

    def test_no_extension(self):
        assert _source_stem("Makefile") == "Makefile"


class TestAssociateModuleItemsWithClass:
    def test_no_classes_no_items_noop(self):
        m = OZModule()
        _associate_module_items_with_class(m)
        assert len(m.orphan_sources) == 0

    def test_no_classes_with_items_creates_orphan(self):
        m = OZModule()
        m.source_stem = "helpers"
        m.functions.append(OZFunction("helper_fn", OZType("void")))
        m.statics.append(OZStaticVar("_count", OZType("int")))
        m.verbatim_lines.append("K_DEFINE(x);")
        m.user_includes.append('#include "util.h"')

        _associate_module_items_with_class(m)

        assert len(m.orphan_sources) == 1
        orphan = m.orphan_sources[0]
        assert orphan.stem == "helpers"
        assert len(orphan.functions) == 1
        assert len(orphan.statics) == 1
        assert len(orphan.verbatim_lines) == 1
        assert len(orphan.user_includes) == 1
        assert m.functions == []
        assert m.statics == []

    def test_no_classes_no_stem_no_orphan(self):
        m = OZModule()
        m.functions.append(OZFunction("fn", OZType("void")))
        _associate_module_items_with_class(m)
        assert len(m.orphan_sources) == 0

    def test_class_with_impl_receives_items(self):
        m = OZModule()
        body = {"kind": "CompoundStmt", "inner": []}
        m.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("init", OZType("instancetype"), body_ast=body),
        ])
        m.functions.append(OZFunction("helper", OZType("void")))
        m.statics.append(OZStaticVar("_s", OZType("int")))
        m.verbatim_lines.append("MACRO();")
        m.user_includes.append('#include "x.h"')

        _associate_module_items_with_class(m)

        cls = m.classes["Foo"]
        assert len(cls.functions) == 1
        assert len(cls.statics) == 1
        assert "MACRO();" in cls.verbatim_lines
        assert '#include "x.h"' in cls.user_includes
        assert m.functions == []
        assert m.statics == []

    def test_no_items_noop(self):
        m = OZModule()
        body = {"kind": "CompoundStmt", "inner": []}
        m.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("init", OZType("instancetype"), body_ast=body),
        ])
        _associate_module_items_with_class(m)
        assert m.classes["Foo"].functions == []

    def test_classes_without_impl_creates_orphan(self):
        m = OZModule()
        m.source_stem = "stubs"
        m.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("init", OZType("instancetype")),
        ])
        m.functions.append(OZFunction("fn", OZType("void")))

        _associate_module_items_with_class(m)

        assert len(m.orphan_sources) == 1
        assert m.orphan_sources[0].stem == "stubs"
        assert m.functions == []

    def test_verbatim_dedup_on_associate(self):
        m = OZModule()
        body = {"kind": "CompoundStmt", "inner": []}
        m.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("init", OZType("instancetype"), body_ast=body),
        ])
        m.classes["Foo"].verbatim_lines.append("EXISTING();")
        m.verbatim_lines.append("EXISTING();")
        m.verbatim_lines.append("NEW();")

        _associate_module_items_with_class(m)

        assert m.classes["Foo"].verbatim_lines == ["EXISTING();", "NEW();"]
