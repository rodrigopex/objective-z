# SPDX-License-Identifier: Apache-2.0

import pytest

from oz_transpile.model import (
    DispatchKind,
    OZClass,
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
