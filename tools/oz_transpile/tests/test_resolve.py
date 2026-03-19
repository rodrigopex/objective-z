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

from .conftest import clang_collect_resolve


class TestHierarchy:
    """Corner cases: Clang rejects invalid hierarchies at parse time."""

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
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject
@end
@interface OZRgbLed : OZLed
@end
""")
        assert mod.classes["OZObject"].class_id < mod.classes["OZLed"].class_id
        assert mod.classes["OZLed"].class_id < mod.classes["OZRgbLed"].class_id

    def test_root_gets_zero(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject
@end
""")
        assert mod.classes["OZObject"].class_id == 0


class TestBaseDepth:
    def test_root_depth_zero(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
""")
        assert mod.classes["OZObject"].base_depth == 0

    def test_child_depth_one(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject
@end
""")
        assert mod.classes["OZLed"].base_depth == 1

    def test_grandchild_depth_two(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject
@end
@interface OZRgbLed : OZLed
@end
""")
        assert mod.classes["OZRgbLed"].base_depth == 2


class TestDispatchClassification:
    def test_unique_selector_is_static(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject
- (void)turnOn;
@end
@implementation OZLed
- (void)turnOn {}
@end
""")
        turnOn = next(m for m in mod.classes["OZLed"].methods
                      if m.selector == "turnOn")
        assert turnOn.dispatch == DispatchKind.STATIC

    def test_protocol_selector_is_protocol(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@protocol OZToggleable
- (void)toggle;
@end
@interface OZLed : OZObject <OZToggleable>
- (void)toggle;
@end
@implementation OZLed
- (void)toggle {}
@end
""")
        toggle = next(m for m in mod.classes["OZLed"].methods
                      if m.selector == "toggle")
        assert toggle.dispatch == DispatchKind.PROTOCOL

    def test_overridden_selector_is_protocol(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject
- (instancetype)init;
@end
@implementation OZLed
- (instancetype)init { return self; }
@end
""")
        # init is also declared in OZObject SDK header → overridden → PROTOCOL
        led_init = next(m for m in mod.classes["OZLed"].methods
                        if m.selector == "init")
        assert led_init.dispatch == DispatchKind.PROTOCOL


class TestSynthesizeProperties:
    def test_readonly_synthesizes_getter_only(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, readonly) int color;
@end
@implementation Car
@synthesize color = _color;
@end
""")
        sels = [m.selector for m in mod.classes["Car"].methods]
        assert "color" in sels
        assert "setColor:" not in sels

    def test_readwrite_synthesizes_getter_and_setter(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, assign) int speed;
@end
@implementation Car
@synthesize speed = _speed;
@end
""")
        sels = [m.selector for m in mod.classes["Car"].methods]
        assert "speed" in sels
        assert "setSpeed:" in sels

    def test_default_ivar_name(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, assign) int color;
@end
@implementation Car
@synthesize color = _color;
@end
""")
        prop = next(p for p in mod.classes["Car"].properties
                    if p.name == "color")
        assert prop.ivar_name == "_color"

    def test_custom_getter_selector(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, assign, getter=isEnabled) BOOL enabled;
@end
@implementation Car
@synthesize enabled = _enabled;
@end
""")
        sels = [m.selector for m in mod.classes["Car"].methods]
        assert "isEnabled" in sels
        assert "enabled" not in sels

    def test_custom_setter_selector(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, assign, setter=setCustomSpeed:) int speed;
@end
@implementation Car
@synthesize speed = _speed;
@end
""")
        sels = [m.selector for m in mod.classes["Car"].methods]
        assert "setCustomSpeed:" in sels
        assert "setSpeed:" not in sels

    def test_user_defined_getter_not_duplicated(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, readonly) int color;
- (int)color;
@end
@implementation Car
@synthesize color = _color;
- (int)color { return _color; }
@end
""")
        color_methods = [m for m in mod.classes["Car"].methods
                         if m.selector == "color"]
        assert len(color_methods) == 1
        assert color_methods[0].synthesized_property is None

    def test_synthesized_method_has_property_ref(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, assign) int speed;
@end
@implementation Car
@synthesize speed = _speed;
@end
""")
        getter = next(m for m in mod.classes["Car"].methods
                      if m.selector == "speed")
        assert getter.synthesized_property is not None
        assert getter.synthesized_property.name == "speed"


class TestInitializeClasses:
    def test_class_with_initialize_is_collected(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface AppConfig : OZObject
+ (void)initialize;
@end
@implementation AppConfig
+ (void)initialize {}
@end
""")
        assert "AppConfig" in mod.initialize_classes

    def test_class_without_initialize_not_collected(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject
- (void)turnOn;
@end
@implementation OZLed
- (void)turnOn {}
@end
""")
        assert "OZLed" not in mod.initialize_classes

    def test_instance_method_named_initialize_not_collected(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)initialize;
@end
@implementation Foo
- (void)initialize {}
@end
""")
        assert "Foo" not in mod.initialize_classes

    def test_apple_guard_pattern_emits_diagnostic(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface AppConfig : OZObject
+ (void)initialize;
@end
@implementation AppConfig
+ (void)initialize {
        if (self == [AppConfig class]) {
        }
}
@end
""")
        assert any("+initialize guard" in d for d in mod.diagnostics)
        assert any("exactly once per class" in d for d in mod.diagnostics)

    def test_no_guard_diagnostic_without_class_message(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@interface AppConfig : OZObject
+ (void)initialize;
- (void)doWork;
@end
@implementation AppConfig
+ (void)initialize {
        AppConfig *c = [[AppConfig alloc] init];
}
- (void)doWork {}
@end
""")
        assert not any("+initialize guard" in d for d in mod.diagnostics)


class TestProtocolConformance:
    """OZ-033: missing protocol method must produce an error."""

    def test_missing_protocol_method_errors(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@protocol SensorProto
- (int)readValue;
- (int)name;
@end
@interface Sensor : OZObject <SensorProto>
- (int)name;
@end
@implementation Sensor
- (int)name { return 0; }
@end
""")
        assert any("readValue" in e for e in mod.errors)

    def test_conforming_class_no_error(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@protocol SensorProto
- (int)readValue;
@end
@interface Sensor : OZObject <SensorProto>
- (int)readValue;
@end
@implementation Sensor
- (int)readValue { return 42; }
@end
""")
        assert not any("readValue" in e for e in mod.errors)

    def test_inherited_method_satisfies_protocol(self):
        mod = clang_collect_resolve("""\
#import <Foundation/OZObject.h>
@protocol Proto
- (void)doWork;
@end
@interface Base : OZObject
- (void)doWork;
@end
@implementation Base
- (void)doWork {}
@end
@interface Child : Base <Proto>
@end
@implementation Child
@end
""")
        assert not any("doWork" in e for e in mod.errors)


# ---------------------------------------------------------------------------
# OZ-057: Generic type validation
# Corner case: Clang erases generic type parameters from the AST.
# These tests validate the resolve pass's ability to check generics when
# they ARE present in the AST (via handcrafted input or source extraction).
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
    return {"kind": "VarDecl", "name": name,
            "type": {"qualType": qual_type}, "inner": [init_expr]}


def _assign(lhs_name, lhs_qt, rhs_expr):
    return {"kind": "BinaryOperator", "opcode": "=", "inner": [
        {"kind": "DeclRefExpr", "type": {"qualType": lhs_qt},
         "referencedDecl": {"name": lhs_name}},
        rhs_expr,
    ]}


def _body(*stmts):
    inner = []
    for s in stmts:
        if s.get("kind") == "VarDecl":
            inner.append({"kind": "DeclStmt", "inner": [s]})
        else:
            inner.append(s)
    return {"kind": "CompoundStmt", "inner": inner}


def _generic_module():
    """Corner case: module with generic type annotations preserved in AST."""
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
    """OZ-057: generic type parameter validation.

    Corner case: Clang erases ObjC generics from qualType in AST dumps,
    so these tests use handcrafted AST nodes with generics preserved.
    Real Clang-based generic validation is covered by source-level
    extraction tests (OZ-058) in test_e2e.py.
    """

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
        m = _generic_module()
        body = _body(
            _var_decl("arr", "OZArray<id<DataProto>> *",
                      _arr_literal("id")))
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast=body)])
        resolve(m)
        generic_errors = [e for e in m.errors if "generic type mismatch" in e]
        assert generic_errors == []
