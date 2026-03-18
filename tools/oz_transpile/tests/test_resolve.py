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


class TestInitializeClasses:
    def test_class_with_initialize_is_collected(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["AppConfig"] = OZClass("AppConfig", superclass="OZObject",
            methods=[OZMethod("initialize", OZType("void"),
                              is_class_method=True)])
        resolve(m)
        assert m.initialize_classes == ["AppConfig"]

    def test_class_without_initialize_not_collected(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["OZLed"] = OZClass("OZLed", superclass="OZObject",
            methods=[OZMethod("turnOn", OZType("void"))])
        resolve(m)
        assert m.initialize_classes == []

    def test_topological_order_superclass_first(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject", methods=[
            OZMethod("initialize", OZType("void"), is_class_method=True)])
        m.classes["AppConfig"] = OZClass("AppConfig", superclass="OZObject",
            methods=[OZMethod("initialize", OZType("void"),
                              is_class_method=True)])
        resolve(m)
        assert m.initialize_classes == ["OZObject", "AppConfig"]

    def test_instance_method_named_initialize_not_collected(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject",
            methods=[OZMethod("initialize", OZType("void"),
                              is_class_method=False)])
        resolve(m)
        assert m.initialize_classes == []

    def test_deep_hierarchy_topological_order(self):
        """Three-level hierarchy: grandchild order is root, child, grandchild."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject", methods=[
            OZMethod("initialize", OZType("void"), is_class_method=True)])
        m.classes["Base"] = OZClass("Base", superclass="OZObject", methods=[
            OZMethod("initialize", OZType("void"), is_class_method=True)])
        m.classes["Derived"] = OZClass("Derived", superclass="Base", methods=[
            OZMethod("initialize", OZType("void"), is_class_method=True)])
        resolve(m)
        assert m.initialize_classes == ["OZObject", "Base", "Derived"]

    def test_multiple_independent_classes(self):
        """Two unrelated classes both define +initialize."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Alpha"] = OZClass("Alpha", superclass="OZObject", methods=[
            OZMethod("initialize", OZType("void"), is_class_method=True)])
        m.classes["Beta"] = OZClass("Beta", superclass="OZObject", methods=[
            OZMethod("initialize", OZType("void"), is_class_method=True)])
        resolve(m)
        assert "Alpha" in m.initialize_classes
        assert "Beta" in m.initialize_classes
        assert len(m.initialize_classes) == 2

    def test_sibling_classes_with_initialize(self):
        """Two siblings (same parent) both define +initialize."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["SibA"] = OZClass("SibA", superclass="OZObject", methods=[
            OZMethod("initialize", OZType("void"), is_class_method=True)])
        m.classes["SibB"] = OZClass("SibB", superclass="OZObject", methods=[
            OZMethod("initialize", OZType("void"), is_class_method=True)])
        resolve(m)
        assert "SibA" in m.initialize_classes
        assert "SibB" in m.initialize_classes
        assert "OZObject" not in m.initialize_classes

    def test_only_subclass_defines_initialize(self):
        """Parent has no +initialize, only subclass does."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Child"] = OZClass("Child", superclass="OZObject", methods=[
            OZMethod("initialize", OZType("void"), is_class_method=True)])
        resolve(m)
        assert m.initialize_classes == ["Child"]

    def test_apple_guard_pattern_emits_diagnostic(self):
        """if (self == [MyClass class]) guard emits a warning diagnostic."""
        body_ast = {"kind": "CompoundStmt", "inner": [{
            "kind": "IfStmt", "inner": [
                {"kind": "BinaryOperator", "opcode": "==", "inner": [
                    {"kind": "DeclRefExpr",
                     "referencedDecl": {"name": "self"}},
                    {"kind": "ObjCMessageExpr", "selector": "class",
                     "receiverKind": "class",
                     "classType": {"qualType": "AppConfig"}},
                ]},
                {"kind": "CompoundStmt", "inner": []},
            ],
        }]}
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["AppConfig"] = OZClass("AppConfig", superclass="OZObject",
            methods=[OZMethod("initialize", OZType("void"),
                              is_class_method=True, body_ast=body_ast)])
        resolve(m)
        assert any("+initialize guard" in d for d in m.diagnostics)
        assert any("exactly once per class" in d for d in m.diagnostics)

    def test_no_guard_diagnostic_without_class_message(self):
        """Normal +initialize body does not trigger guard diagnostic."""
        body_ast = {"kind": "CompoundStmt", "inner": [{
            "kind": "ObjCMessageExpr", "selector": "alloc",
            "receiverKind": "class",
            "classType": {"qualType": "AppConfig"},
            "inner": [],
        }]}
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["AppConfig"] = OZClass("AppConfig", superclass="OZObject",
            methods=[OZMethod("initialize", OZType("void"),
                              is_class_method=True, body_ast=body_ast)])
        resolve(m)
        assert not any("+initialize guard" in d for d in m.diagnostics)


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


# ---------------------------------------------------------------------------
# OZ-057: Generic type validation
# ---------------------------------------------------------------------------

def _arr_literal(*elem_types):
    """Build an ObjCArrayLiteral AST with elements of given qualTypes."""
    inner = []
    for qt in elem_types:
        inner.append({
            "kind": "ImplicitCastExpr",
            "type": {"qualType": "id"},
            "inner": [{
                "kind": "DeclRefExpr",
                "type": {"qualType": qt},
                "referencedDecl": {"name": "_tmp"},
            }],
        })
    return {"kind": "ObjCArrayLiteral",
            "type": {"qualType": "NSArray *"}, "inner": inner}


def _dict_literal(pairs):
    """Build an ObjCDictionaryLiteral AST with (key_qt, val_qt) pairs."""
    inner = []
    for kqt, vqt in pairs:
        inner.append({
            "kind": "ImplicitCastExpr",
            "type": {"qualType": "id"},
            "inner": [{"kind": "DeclRefExpr",
                        "type": {"qualType": kqt},
                        "referencedDecl": {"name": "_k"}}],
        })
        inner.append({
            "kind": "ImplicitCastExpr",
            "type": {"qualType": "id"},
            "inner": [{"kind": "DeclRefExpr",
                        "type": {"qualType": vqt},
                        "referencedDecl": {"name": "_v"}}],
        })
    return {"kind": "ObjCDictionaryLiteral",
            "type": {"qualType": "NSDictionary *"}, "inner": inner}


def _var_decl(name, qual_type, init_expr):
    """Build a VarDecl AST node."""
    return {"kind": "VarDecl", "name": name,
            "type": {"qualType": qual_type}, "inner": [init_expr]}


def _assign(lhs_name, lhs_qt, rhs_expr):
    """Build a BinaryOperator = AST node."""
    return {"kind": "BinaryOperator", "opcode": "=", "inner": [
        {"kind": "DeclRefExpr", "type": {"qualType": lhs_qt},
         "referencedDecl": {"name": lhs_name}},
        rhs_expr,
    ]}


def _body(*stmts):
    """Wrap statements in a CompoundStmt with DeclStmts."""
    inner = []
    for s in stmts:
        if s.get("kind") == "VarDecl":
            inner.append({"kind": "DeclStmt", "inner": [s]})
        else:
            inner.append(s)
    return {"kind": "CompoundStmt", "inner": inner}


def _generic_module():
    """Module with OZObject, DataProto protocol, Sensor (conforms),
    and Filter (conforms to DataProto)."""
    m = OZModule()
    m.classes["OZObject"] = OZClass("OZObject")
    m.protocols["DataProto"] = OZProtocol("DataProto", methods=[
        OZMethod("processValue:", OZType("int")),
    ])
    m.classes["Sensor"] = OZClass("Sensor", superclass="OZObject",
        protocols=["SensorProto"])
    m.classes["Filter"] = OZClass("Filter", superclass="OZObject",
        protocols=["DataProto"])
    m.protocols["SensorProto"] = OZProtocol("SensorProto", methods=[
        OZMethod("readValue", OZType("int")),
    ])
    return m


class TestGenericValidation:
    """OZ-057: generic type parameter validation."""

    def test_array_wrong_protocol_errors(self):
        m = _generic_module()
        body = _body(
            _var_decl("arr", "OZArray<id<DataProto>> *",
                      _arr_literal("Sensor *")))
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        assert any("generic type mismatch" in e for e in m.errors)
        assert any("Sensor" in e for e in m.errors)
        assert any("DataProto" in e for e in m.errors)

    def test_array_correct_protocol_no_error(self):
        m = _generic_module()
        body = _body(
            _var_decl("arr", "OZArray<id<DataProto>> *",
                      _arr_literal("Filter *")))
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        generic_errors = [e for e in m.errors if "generic type mismatch" in e]
        assert generic_errors == []

    def test_array_plain_id_no_validation(self):
        m = _generic_module()
        body = _body(
            _var_decl("arr", "OZArray<id> *",
                      _arr_literal("Sensor *")))
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        generic_errors = [e for e in m.errors if "generic type mismatch" in e]
        assert generic_errors == []

    def test_array_class_constraint_match(self):
        m = _generic_module()
        m.classes["OZString"] = OZClass("OZString", superclass="OZObject")
        body = _body(
            _var_decl("arr", "OZArray<OZString *> *",
                      _arr_literal("OZString *")))
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        generic_errors = [e for e in m.errors if "generic type mismatch" in e]
        assert generic_errors == []

    def test_array_class_constraint_mismatch(self):
        m = _generic_module()
        m.classes["OZString"] = OZClass("OZString", superclass="OZObject")
        m.classes["OZNumber"] = OZClass("OZNumber", superclass="OZObject")
        body = _body(
            _var_decl("arr", "OZArray<OZString *> *",
                      _arr_literal("OZNumber *")))
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        assert any("generic type mismatch" in e for e in m.errors)
        assert any("OZNumber" in e for e in m.errors)

    def test_array_subclass_satisfies_constraint(self):
        m = _generic_module()
        m.classes["OZString"] = OZClass("OZString", superclass="OZObject")
        m.classes["MyString"] = OZClass("MyString", superclass="OZString")
        body = _body(
            _var_decl("arr", "OZArray<OZString *> *",
                      _arr_literal("MyString *")))
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        generic_errors = [e for e in m.errors if "generic type mismatch" in e]
        assert generic_errors == []

    def test_assignment_to_generic_var_errors(self):
        m = _generic_module()
        decl = _var_decl("arr", "OZArray<id<DataProto>> *",
                         {"kind": "ImplicitCastExpr",
                          "type": {"qualType": "id"},
                          "inner": [{"kind": "IntegerLiteral",
                                     "type": {"qualType": "int"},
                                     "value": "0"}]})
        assign = _assign("arr", "OZArray<id<DataProto>> *",
                         _arr_literal("Sensor *"))
        body = _body(decl, assign)
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        assert any("generic type mismatch" in e for e in m.errors)

    def test_assignment_correct_no_error(self):
        m = _generic_module()
        decl = _var_decl("arr", "OZArray<id<DataProto>> *",
                         {"kind": "ImplicitCastExpr",
                          "type": {"qualType": "id"},
                          "inner": [{"kind": "IntegerLiteral",
                                     "type": {"qualType": "int"},
                                     "value": "0"}]})
        assign = _assign("arr", "OZArray<id<DataProto>> *",
                         _arr_literal("Filter *"))
        body = _body(decl, assign)
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        generic_errors = [e for e in m.errors if "generic type mismatch" in e]
        assert generic_errors == []

    def test_dict_wrong_value_type_errors(self):
        m = _generic_module()
        m.classes["OZString"] = OZClass("OZString", superclass="OZObject")
        body = _body(
            _var_decl("d", "OZDictionary<OZString *, id<DataProto>> *",
                      _dict_literal([("OZString *", "Sensor *")])))
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        assert any("generic type mismatch" in e for e in m.errors)
        assert any("Sensor" in e for e in m.errors)

    def test_dict_correct_types_no_error(self):
        m = _generic_module()
        m.classes["OZString"] = OZClass("OZString", superclass="OZObject")
        body = _body(
            _var_decl("d", "OZDictionary<OZString *, id<DataProto>> *",
                      _dict_literal([("OZString *", "Filter *")])))
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        generic_errors = [e for e in m.errors if "generic type mismatch" in e]
        assert generic_errors == []

    def test_ns_alias_matches_oz_class(self):
        m = _generic_module()
        m.classes["OZString"] = OZClass("OZString", superclass="OZObject")
        body = _body(
            _var_decl("arr", "OZArray<OZString *> *",
                      _arr_literal("NSString *")))
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        generic_errors = [e for e in m.errors if "generic type mismatch" in e]
        assert generic_errors == []

    def test_inherited_protocol_conformance(self):
        m = _generic_module()
        m.classes["AdvFilter"] = OZClass("AdvFilter", superclass="Filter",
            protocols=[])
        body = _body(
            _var_decl("arr", "OZArray<id<DataProto>> *",
                      _arr_literal("AdvFilter *")))
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        generic_errors = [e for e in m.errors if "generic type mismatch" in e]
        assert generic_errors == []

    def test_function_body_validated(self):
        m = _generic_module()
        from oz_transpile.model import OZFunction
        func_body = _body(
            _var_decl("arr", "OZArray<id<DataProto>> *",
                      _arr_literal("Sensor *")))
        m.functions.append(OZFunction("main", OZType("int"),
                                       body_ast=func_body))
        resolve(m)
        assert any("generic type mismatch" in e for e in m.errors)

    def test_element_id_type_skipped(self):
        """Plain id element should not trigger validation."""
        m = _generic_module()
        body = _body(
            _var_decl("arr", "OZArray<id<DataProto>> *",
                      _arr_literal("id")))
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        generic_errors = [e for e in m.errors if "generic type mismatch" in e]
        assert generic_errors == []
