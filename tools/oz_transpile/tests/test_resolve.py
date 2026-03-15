# SPDX-License-Identifier: Apache-2.0

import pytest

from oz_transpile.model import (
    DispatchKind,
    OZClass,
    OZIvar,
    OZMethod,
    OZModule,
    OZProperty,
    OZProtocol,
    OZType,
)
from oz_transpile.resolve import resolve


def _module_with_chain():
    """OZObject -> OZLed -> OZRgbLed"""
    m = OZModule()
    m.classes["OZObject"] = OZClass("OZObject")
    m.classes["OZLed"] = OZClass("OZLed", superclass="OZObject")
    m.classes["OZRgbLed"] = OZClass("OZRgbLed", superclass="OZLed")
    return m


class TestHierarchy:
    def test_missing_superclass_diagnostic(self):
        m = OZModule()
        m.classes["OZLed"] = OZClass("OZLed", superclass="OZMissing")
        resolve(m)
        assert any("OZMissing" in d for d in m.diagnostics)

    def test_cycle_detection(self):
        m = OZModule()
        m.classes["A"] = OZClass("A", superclass="B")
        m.classes["B"] = OZClass("B", superclass="A")
        with pytest.raises(ValueError, match="inheritance cycle"):
            resolve(m)


class TestClassIds:
    def test_topological_order(self):
        m = _module_with_chain()
        resolve(m)
        assert m.classes["OZObject"].class_id < m.classes["OZLed"].class_id
        assert m.classes["OZLed"].class_id < m.classes["OZRgbLed"].class_id

    def test_root_gets_zero(self):
        m = _module_with_chain()
        resolve(m)
        assert m.classes["OZObject"].class_id == 0


class TestBaseDepth:
    def test_root_depth_zero(self):
        m = _module_with_chain()
        resolve(m)
        assert m.classes["OZObject"].base_depth == 0

    def test_child_depth_one(self):
        m = _module_with_chain()
        resolve(m)
        assert m.classes["OZLed"].base_depth == 1

    def test_grandchild_depth_two(self):
        m = _module_with_chain()
        resolve(m)
        assert m.classes["OZRgbLed"].base_depth == 2


class TestDispatchClassification:
    def test_unique_selector_is_static(self):
        m = OZModule()
        m.classes["OZLed"] = OZClass("OZLed", methods=[
            OZMethod("turnOn", OZType("void")),
        ])
        resolve(m)
        assert m.classes["OZLed"].methods[0].dispatch == DispatchKind.STATIC

    def test_protocol_selector_is_protocol(self):
        m = OZModule()
        m.classes["OZLed"] = OZClass("OZLed", methods=[
            OZMethod("toggle", OZType("void")),
        ])
        m.protocols["OZToggleable"] = OZProtocol("OZToggleable", [
            OZMethod("toggle", OZType("void")),
        ])
        resolve(m)
        assert m.classes["OZLed"].methods[0].dispatch == DispatchKind.PROTOCOL

    def test_overridden_selector_is_protocol(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject", methods=[
            OZMethod("init", OZType("instancetype")),
        ])
        m.classes["OZLed"] = OZClass("OZLed", superclass="OZObject", methods=[
            OZMethod("init", OZType("instancetype")),
        ])
        resolve(m)
        assert m.classes["OZLed"].methods[0].dispatch == DispatchKind.PROTOCOL
        assert m.classes["OZObject"].methods[0].dispatch == DispatchKind.PROTOCOL


class TestSynthesizeProperties:
    def test_readonly_synthesizes_getter_only(self):
        m = OZModule()
        m.classes["Car"] = OZClass("Car", properties=[
            OZProperty("color", OZType("struct color *"),
                       is_readonly=True, is_nonatomic=True),
        ])
        resolve(m)
        sels = [method.selector for method in m.classes["Car"].methods]
        assert "color" in sels
        assert "setColor:" not in sels

    def test_readwrite_synthesizes_getter_and_setter(self):
        m = OZModule()
        m.classes["Car"] = OZClass("Car", properties=[
            OZProperty("speed", OZType("int"), is_nonatomic=True,
                       ownership="assign"),
        ])
        resolve(m)
        sels = [method.selector for method in m.classes["Car"].methods]
        assert "speed" in sels
        assert "setSpeed:" in sels

    def test_default_ivar_name(self):
        m = OZModule()
        m.classes["Car"] = OZClass("Car", properties=[
            OZProperty("color", OZType("int"), is_nonatomic=True),
        ])
        resolve(m)
        assert m.classes["Car"].properties[0].ivar_name == "_color"

    def test_explicit_ivar_name_preserved(self):
        m = OZModule()
        m.classes["Car"] = OZClass("Car", properties=[
            OZProperty("ackCount", OZType("int"), ivar_name="_count",
                       is_nonatomic=True),
        ])
        resolve(m)
        assert m.classes["Car"].properties[0].ivar_name == "_count"

    def test_custom_getter_selector(self):
        m = OZModule()
        m.classes["Car"] = OZClass("Car", properties=[
            OZProperty("enabled", OZType("BOOL"), getter_sel="isEnabled",
                       is_nonatomic=True, ownership="assign"),
        ])
        resolve(m)
        sels = [method.selector for method in m.classes["Car"].methods]
        assert "isEnabled" in sels
        assert "enabled" not in sels

    def test_custom_setter_selector(self):
        m = OZModule()
        m.classes["Car"] = OZClass("Car", properties=[
            OZProperty("speed", OZType("int"), setter_sel="setCustomSpeed:",
                       is_nonatomic=True, ownership="assign"),
        ])
        resolve(m)
        sels = [method.selector for method in m.classes["Car"].methods]
        assert "setCustomSpeed:" in sels
        assert "setSpeed:" not in sels

    def test_user_defined_getter_not_duplicated(self):
        m = OZModule()
        m.classes["Car"] = OZClass("Car",
            methods=[OZMethod("color", OZType("int"))],
            properties=[
                OZProperty("color", OZType("int"), is_readonly=True,
                           is_nonatomic=True),
            ])
        resolve(m)
        color_methods = [method for method in m.classes["Car"].methods
                         if method.selector == "color"]
        assert len(color_methods) == 1
        assert color_methods[0].synthesized_property is None

    def test_synthesized_method_has_property_ref(self):
        m = OZModule()
        prop = OZProperty("speed", OZType("int"), is_nonatomic=True,
                          ownership="assign")
        m.classes["Car"] = OZClass("Car", properties=[prop])
        resolve(m)
        getter = [method for method in m.classes["Car"].methods
                  if method.selector == "speed"][0]
        assert getter.synthesized_property is prop

    def test_bare_synthesize_uses_existing_ivar(self):
        """OZ-002: @synthesize propName; (no explicit ivar) should use the
        bare ivar name when it already exists in cls.ivars."""
        m = OZModule()
        m.classes["Config"] = OZClass("Config",
            ivars=[OZIvar("sampleRate", OZType("int"))],
            properties=[OZProperty("sampleRate", OZType("int"),
                                   is_nonatomic=True, ownership="assign")])
        resolve(m)
        prop = m.classes["Config"].properties[0]
        assert prop.ivar_name == "sampleRate"
        assert any("bare ivar" in d for d in m.diagnostics)

    def test_bare_synthesize_accessor_matches_struct(self):
        """OZ-002: synthesized accessor must reference same name as struct field."""
        m = OZModule()
        m.classes["Config"] = OZClass("Config",
            ivars=[OZIvar("sampleRate", OZType("int"))],
            properties=[OZProperty("sampleRate", OZType("int"),
                                   is_nonatomic=True, ownership="assign")])
        resolve(m)
        getter = [method for method in m.classes["Config"].methods
                  if method.selector == "sampleRate"][0]
        assert getter.synthesized_property.ivar_name == "sampleRate"

    def test_mixed_bare_and_explicit_synthesize(self):
        """OZ-026: class with both bare and explicit @synthesize forms."""
        m = OZModule()
        m.classes["Config"] = OZClass("Config",
            ivars=[OZIvar("rate", OZType("int")),
                   OZIvar("_name", OZType("OZString *"))],
            properties=[
                OZProperty("rate", OZType("int"),
                           is_nonatomic=True, ownership="assign"),
                OZProperty("name", OZType("OZString *"),
                           ivar_name="_name", is_nonatomic=True),
            ])
        resolve(m)
        props = {p.name: p for p in m.classes["Config"].properties}
        assert props["rate"].ivar_name == "rate"
        assert props["name"].ivar_name == "_name"


class TestProtocolConformance:
    """OZ-033: missing protocol method must produce an error."""

    def test_missing_protocol_method_errors(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.protocols["SensorProto"] = OZProtocol("SensorProto", methods=[
            OZMethod("readValue", OZType("int")),
            OZMethod("name", OZType("OZString *")),
        ])
        m.classes["Sensor"] = OZClass("Sensor", superclass="OZObject",
            protocols=["SensorProto"],
            methods=[OZMethod("name", OZType("OZString *"), body_ast={
                "kind": "CompoundStmt", "inner": []})])
        resolve(m)
        assert any("readValue" in e for e in m.errors)

    def test_conforming_class_no_error(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.protocols["SensorProto"] = OZProtocol("SensorProto", methods=[
            OZMethod("readValue", OZType("int")),
        ])
        m.classes["Sensor"] = OZClass("Sensor", superclass="OZObject",
            protocols=["SensorProto"],
            methods=[OZMethod("readValue", OZType("int"), body_ast={
                "kind": "CompoundStmt", "inner": []})])
        resolve(m)
        assert not any("readValue" in e for e in m.errors)

    def test_inherited_method_satisfies_protocol(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.protocols["Proto"] = OZProtocol("Proto", methods=[
            OZMethod("doWork", OZType("void")),
        ])
        m.classes["Base"] = OZClass("Base", superclass="OZObject",
            methods=[OZMethod("doWork", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": []})])
        m.classes["Child"] = OZClass("Child", superclass="Base",
            protocols=["Proto"])
        resolve(m)
        assert not any("doWork" in e for e in m.errors)
