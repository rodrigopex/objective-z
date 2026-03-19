# SPDX-License-Identifier: Apache-2.0

from oz_transpile.collect import collect, is_stub_source, merge_modules
from oz_transpile.model import (OZClass, OZFunction, OZIvar, OZMethod, OZModule,
                                OZParam, OZProperty, OZProtocol, OZStaticVar,
                                OZType)

from .conftest import clang_collect


class TestCollectInterface:
    def test_basic_class(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject {
        int _pin;
}
@end
""")
        assert "OZObject" in mod.classes
        assert "OZLed" in mod.classes
        assert mod.classes["OZLed"].superclass == "OZObject"
        assert any(iv.name == "_pin" for iv in mod.classes["OZLed"].ivars)

    def test_protocols(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@protocol OZToggleable
- (void)toggle;
@end
@interface OZLed : OZObject <OZToggleable>
@end
""")
        assert "OZToggleable" in mod.classes["OZLed"].protocols

    def test_no_superclass(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
""")
        assert mod.classes["OZObject"].superclass is None

    def test_ivar_access_collected(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject {
@protected
        int _color;
@public
        int _speed;
}
@end
""")
        ivars = [iv for iv in mod.classes["Car"].ivars
                 if iv.name in ("_color", "_speed")]
        color = next(iv for iv in ivars if iv.name == "_color")
        speed = next(iv for iv in ivars if iv.name == "_speed")
        assert color.access == "protected"
        assert speed.access == "public"

    def test_ivar_access_default_protected(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject {
        int _color;
}
@end
""")
        color = next(iv for iv in mod.classes["Car"].ivars
                     if iv.name == "_color")
        assert color.access == "protected"


class TestCollectImplementation:
    def test_methods(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject
- (instancetype)init;
- (void)turnOn;
@end
@implementation OZLed
- (instancetype)init { return self; }
- (void)turnOn {}
@end
""")
        cls = mod.classes["OZLed"]
        sels = [m.selector for m in cls.methods]
        assert "init" in sels
        assert "turnOn" in sels
        init_m = next(m for m in cls.methods if m.selector == "init")
        assert init_m.return_type.is_object
        assert init_m.body_ast is not None

    def test_class_method(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject
+ (instancetype)alloc;
@end
@implementation OZLed
+ (instancetype)alloc { return nil; }
@end
""")
        alloc = next(m for m in mod.classes["OZLed"].methods
                     if m.selector == "alloc")
        assert alloc.is_class_method

    def test_method_params(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject
- (void)setPin:(int)pin;
@end
@implementation OZLed
- (void)setPin:(int)pin {}
@end
""")
        m = next(m for m in mod.classes["OZLed"].methods
                 if m.selector == "setPin:")
        assert len(m.params) == 1
        assert m.params[0].name == "pin"
        assert m.params[0].oz_type.c_type == "int"

    def test_interface_then_impl_merge(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject {
        int _pin;
}
- (instancetype)init;
@end
@implementation OZLed
- (instancetype)init { return self; }
@end
""")
        cls = mod.classes["OZLed"]
        assert cls.superclass == "OZObject"
        assert any(iv.name == "_pin" for iv in cls.ivars)
        assert any(m.selector == "init" for m in cls.methods)


class TestCollectProtocol:
    def test_protocol(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@protocol OZToggleable
- (void)toggle;
@end
""")
        assert "OZToggleable" in mod.protocols
        proto = mod.protocols["OZToggleable"]
        assert any(m.selector == "toggle" for m in proto.methods)


class TestCollectCategory:
    def test_category_merges(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject
@end
@interface OZLed (Extras)
- (void)blink;
@end
""")
        assert any(m.selector == "blink" for m in mod.classes["OZLed"].methods)

    def test_category_creates_class(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface OZFoo : OZObject
@end
@interface OZFoo (Extra)
- (void)bar;
@end
""")
        assert "OZFoo" in mod.classes
        assert any(m.selector == "bar" for m in mod.classes["OZFoo"].methods)


class TestCollectCategoryImpl:
    def test_category_impl_merges_with_body(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@end
@implementation Car (Extra)
- (int)milage { return 100; }
@end
""")
        m = next(m for m in mod.classes["Car"].methods
                 if m.selector == "milage")
        assert m.body_ast is not None

    def test_category_impl_creates_class(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Bike : OZObject
@end
@implementation Bike (Extra)
- (int)speed { return 0; }
@end
""")
        assert "Bike" in mod.classes
        assert any(m.selector == "speed" for m in mod.classes["Bike"].methods)


class TestCollectStaticVar:
    def test_static_var_collected(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface AppConfig : OZObject
@end
static AppConfig *_sharedConfig;
""")
        statics = [s for s in mod.statics if s.name == "_sharedConfig"]
        assert len(statics) == 1
        assert "AppConfig" in statics[0].oz_type.c_type

    def test_non_static_var_skipped(self):
        """Non-static module-level vars are not collected."""
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
""")
        # OZObject.h doesn't declare non-static vars at module level
        user_statics = [s for s in mod.statics
                        if "oz_sdk" not in s.name and "OZ" not in s.name]
        assert len(user_statics) == 0

    def test_static_with_nil_init(self):
        """static with nil initializer should collect NULL."""
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface PXAppConfig : OZObject
@end
static PXAppConfig *_shared = nil;
""")
        s = next(s for s in mod.statics if s.name == "_shared")
        assert s.init_value == "NULL"


class TestCollectTypeDefs:
    def test_user_enum_from_main_file(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
enum PXDeviceState {
        PXDeviceStateIdle = 0,
        PXDeviceStateRunning = 1,
};
""")
        assert "enum PXDeviceState" in mod.type_defs
        defn = mod.type_defs["enum PXDeviceState"]
        assert "PXDeviceStateIdle = 0," in defn
        assert "PXDeviceStateRunning = 1," in defn

    def test_user_enum_non_sequential_values(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
enum PXPriority {
        PXLow = 0,
        PXMedium = 5,
        PXHigh = 100,
};
""")
        defn = mod.type_defs["enum PXPriority"]
        assert "PXLow = 0," in defn
        assert "PXMedium = 5," in defn
        assert "PXHigh = 100," in defn

    def test_user_enum_in_header_collected(self):
        """Enum in included user .h file should be collected (OZ-061)."""
        mod = clang_collect(
            '#import "MyHeader.h"\n',
            extra_files={
                "MyHeader.h": "enum HeaderEnum { HE_A };\n",
            },
        )
        assert "enum HeaderEnum" in mod.type_defs

    def test_user_anonymous_enum_not_collected(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
enum { ANON_VAL = 42 };
""")
        # Anonymous enums should not be collected
        for key in mod.type_defs:
            assert "ANON_VAL" not in mod.type_defs[key]

    def test_multiple_user_enums_collected(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
enum EnumA { A1 = 0 };
enum EnumB { B1 = 0 };
""")
        assert "enum EnumA" in mod.type_defs
        assert "enum EnumB" in mod.type_defs


class TestCollectVerbatimLines:
    """Verbatim line collection requires Zephyr macros (K_THREAD_DEFINE) that
    Clang cannot parse without Zephyr headers.  These are intentionally
    handcrafted AST tests — corner case exception."""

    def test_k_thread_define_collected(self, tmp_path):
        from oz_transpile.collect import collect

        src = tmp_path / "main.m"
        src.write_text(
            '#import <Foundation/OZObject.h>\n'
            "@interface Foo : OZObject\n@end\n"
            "K_THREAD_DEFINE(my_thread, 1024, entry, NULL, NULL, NULL, 7, 0, 0);\n"
        )
        ast = {"kind": "TranslationUnitDecl", "inner": [
            {"kind": "ObjCInterfaceDecl", "name": "Foo",
             "super": {"name": "OZObject"}, "inner": []},
            {"kind": "VarDecl", "name": "dummy",
             "loc": {"file": str(src)},
             "type": {"qualType": "int"}},
        ]}
        mod = collect(ast)
        assert any("K_THREAD_DEFINE" in v and "my_thread" in v
                    for v in mod.verbatim_lines)

    def test_no_macros_no_verbatim(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
@end
""")
        assert not any("K_THREAD_DEFINE" in v for v in mod.verbatim_lines)


class TestCollectProperty:
    def test_readonly_nonatomic(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, readonly) int color;
@end
""")
        props = mod.classes["Car"].properties
        color = next(p for p in props if p.name == "color")
        assert color.is_readonly is True
        assert color.is_nonatomic is True

    def test_strong_property(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@class OZString;
@interface Car : OZObject
@property (nonatomic, strong) OZString *model;
@end
""")
        prop = next(p for p in mod.classes["Car"].properties
                    if p.name == "model")
        assert prop.ownership == "strong"
        assert prop.is_readonly is False

    def test_unsafe_unretained_property(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, unsafe_unretained) id delegate;
@end
""")
        prop = next(p for p in mod.classes["Car"].properties
                    if p.name == "delegate")
        assert prop.ownership == "unsafe_unretained"

    def test_assign_property(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, assign) int speed;
@end
""")
        prop = next(p for p in mod.classes["Car"].properties
                    if p.name == "speed")
        assert prop.ownership == "unsafe_unretained"

    def test_atomic_is_default(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@class OZString;
@interface Car : OZObject
@property (strong) OZString *model;
@end
""")
        prop = next(p for p in mod.classes["Car"].properties
                    if p.name == "model")
        assert prop.is_nonatomic is False

    def test_custom_getter(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, assign, getter=isEnabled) BOOL enabled;
@end
""")
        prop = next(p for p in mod.classes["Car"].properties
                    if p.name == "enabled")
        assert prop.getter_sel == "isEnabled"

    def test_custom_setter(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, assign, setter=setCustomSpeed:) int speed;
@end
""")
        prop = next(p for p in mod.classes["Car"].properties
                    if p.name == "speed")
        assert prop.setter_sel == "setCustomSpeed:"

    def test_property_impl_links_ivar(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, readonly) int color;
@end
@implementation Car
@synthesize color = _color;
@end
""")
        prop = next(p for p in mod.classes["Car"].properties
                    if p.name == "color")
        assert prop.ivar_name == "_color"

    def test_property_impl_custom_ivar_name(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, assign) int ackCount;
@end
@implementation Car
@synthesize ackCount = _count;
@end
""")
        prop = next(p for p in mod.classes["Car"].properties
                    if p.name == "ackCount")
        assert prop.ivar_name == "_count"

    def test_weak_property_raises_error(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property (nonatomic, weak) id delegate;
@end
""")
        assert not any(p.name == "delegate"
                       for p in mod.classes["Car"].properties)
        assert any("weak" in e and "delegate" in e for e in mod.errors)


class TestIsStubSource:
    def test_oz_transpile_path(self):
        assert is_stub_source("/path/to/oz_transpile/OZObject.m") is True

    def test_oz_sdk_path(self):
        assert is_stub_source("/path/to/include/oz_sdk/Foundation/OZString.h") is True

    def test_user_source(self):
        assert is_stub_source("/path/to/src/MyClass.m") is False

    def test_empty_path(self):
        assert is_stub_source("") is False


class TestMergeModules:
    def test_disjoint_classes(self):
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo", superclass="OZObject")
        m2 = OZModule()
        m2.classes["Bar"] = OZClass("Bar", superclass="OZObject")

        merged = merge_modules([m1, m2])
        assert "Foo" in merged.classes
        assert "Bar" in merged.classes

    def test_superclass_fill_in(self):
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo")
        m2 = OZModule()
        m2.classes["Foo"] = OZClass("Foo", superclass="OZObject")

        merged = merge_modules([m1, m2])
        assert merged.classes["Foo"].superclass == "OZObject"

    def test_superclass_not_overwritten(self):
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo", superclass="OZObject")
        m2 = OZModule()
        m2.classes["Foo"] = OZClass("Foo", superclass="OtherBase")

        merged = merge_modules([m1, m2])
        assert merged.classes["Foo"].superclass == "OZObject"

    def test_ivar_fill_in(self):
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo")
        m2 = OZModule()
        m2.classes["Foo"] = OZClass("Foo", ivars=[
            OZIvar("_x", OZType("int")),
        ])

        merged = merge_modules([m1, m2])
        assert len(merged.classes["Foo"].ivars) == 1
        assert merged.classes["Foo"].ivars[0].name == "_x"

    def test_ivars_not_overwritten(self):
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo", ivars=[
            OZIvar("_a", OZType("int")),
        ])
        m2 = OZModule()
        m2.classes["Foo"] = OZClass("Foo", ivars=[
            OZIvar("_b", OZType("float")),
        ])

        merged = merge_modules([m1, m2])
        assert len(merged.classes["Foo"].ivars) == 1
        assert merged.classes["Foo"].ivars[0].name == "_a"

    def test_method_body_ast_override(self):
        body = {"kind": "CompoundStmt", "inner": []}
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("init", OZType("instancetype")),
        ])
        m2 = OZModule()
        m2.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("init", OZType("instancetype"), body_ast=body),
        ])

        merged = merge_modules([m1, m2])
        assert len(merged.classes["Foo"].methods) == 1
        assert merged.classes["Foo"].methods[0].body_ast is body

    def test_method_with_body_not_overwritten_by_decl(self):
        body = {"kind": "CompoundStmt", "inner": []}
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("init", OZType("instancetype"), body_ast=body),
        ])
        m2 = OZModule()
        m2.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("init", OZType("instancetype")),
        ])

        merged = merge_modules([m1, m2])
        assert merged.classes["Foo"].methods[0].body_ast is body

    def test_new_method_appended(self):
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("init", OZType("instancetype")),
        ])
        m2 = OZModule()
        m2.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("doWork", OZType("void")),
        ])

        merged = merge_modules([m1, m2])
        selectors = [m.selector for m in merged.classes["Foo"].methods]
        assert selectors == ["init", "doWork"]

    def test_protocol_deduplication(self):
        m1 = OZModule()
        m1.protocols["Proto"] = OZProtocol("Proto", methods=[
            OZMethod("run", OZType("void")),
        ])
        m2 = OZModule()
        m2.protocols["Proto"] = OZProtocol("Proto", methods=[
            OZMethod("run", OZType("void")),
        ])

        merged = merge_modules([m1, m2])
        assert len(merged.protocols) == 1
        assert "Proto" in merged.protocols

    def test_protocol_class_list_dedup(self):
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo", protocols=["ProtoA", "ProtoB"])
        m2 = OZModule()
        m2.classes["Foo"] = OZClass("Foo", protocols=["ProtoB", "ProtoC"])

        merged = merge_modules([m1, m2])
        assert merged.classes["Foo"].protocols == ["ProtoA", "ProtoB", "ProtoC"]

    def test_property_fill_in(self):
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo")
        m2 = OZModule()
        m2.classes["Foo"] = OZClass("Foo", properties=[
            OZProperty("color", OZType("int")),
        ])

        merged = merge_modules([m1, m2])
        assert len(merged.classes["Foo"].properties) == 1
        assert merged.classes["Foo"].properties[0].name == "color"

    def test_properties_not_overwritten(self):
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo", properties=[
            OZProperty("color", OZType("int")),
        ])
        m2 = OZModule()
        m2.classes["Foo"] = OZClass("Foo", properties=[
            OZProperty("speed", OZType("float")),
        ])

        merged = merge_modules([m1, m2])
        assert len(merged.classes["Foo"].properties) == 1
        assert merged.classes["Foo"].properties[0].name == "color"

    def test_diagnostics_accumulated(self):
        m1 = OZModule()
        m1.diagnostics.append("warn1")
        m2 = OZModule()
        m2.diagnostics.append("warn2")

        merged = merge_modules([m1, m2])
        assert merged.diagnostics == ["warn1", "warn2"]

    def test_errors_accumulated(self):
        m1 = OZModule()
        m1.errors.append("err1")
        m2 = OZModule()
        m2.errors.append("err2")

        merged = merge_modules([m1, m2])
        assert merged.errors == ["err1", "err2"]

    def test_verbatim_lines_dedup(self):
        m1 = OZModule()
        m1.verbatim_lines.append("K_THREAD_DEFINE(a);")
        m2 = OZModule()
        m2.verbatim_lines.append("K_THREAD_DEFINE(a);")
        m2.verbatim_lines.append("K_THREAD_DEFINE(b);")

        merged = merge_modules([m1, m2])
        assert merged.verbatim_lines == [
            "K_THREAD_DEFINE(a);", "K_THREAD_DEFINE(b);"]

    def test_user_includes_dedup(self):
        m1 = OZModule()
        m1.user_includes.append('#include "foo.h"')
        m2 = OZModule()
        m2.user_includes.append('#include "foo.h"')
        m2.user_includes.append('#include "bar.h"')

        merged = merge_modules([m1, m2])
        assert merged.user_includes == ['#include "foo.h"', '#include "bar.h"']

    def test_type_defs_merged(self):
        m1 = OZModule()
        m1.type_defs["enum A"] = "enum A { A1 };"
        m2 = OZModule()
        m2.type_defs["enum B"] = "enum B { B1 };"

        merged = merge_modules([m1, m2])
        assert "enum A" in merged.type_defs
        assert "enum B" in merged.type_defs

    def test_module_level_functions_merged(self):
        m1 = OZModule()
        m1.functions.append(OZFunction("func1", OZType("void")))
        m2 = OZModule()
        m2.functions.append(OZFunction("func2", OZType("int")))

        merged = merge_modules([m1, m2])
        assert len(merged.functions) == 2

    def test_module_level_statics_merged(self):
        m1 = OZModule()
        m1.statics.append(OZStaticVar("s1", OZType("int")))
        m2 = OZModule()
        m2.statics.append(OZStaticVar("s2", OZType("float")))

        merged = merge_modules([m1, m2])
        assert len(merged.statics) == 2

    def test_class_functions_and_statics_merged(self):
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo", functions=[
            OZFunction("helper1", OZType("void")),
        ])
        m2 = OZModule()
        m2.classes["Foo"] = OZClass("Foo", statics=[
            OZStaticVar("_count", OZType("int")),
        ])

        merged = merge_modules([m1, m2])
        assert len(merged.classes["Foo"].functions) == 1
        assert len(merged.classes["Foo"].statics) == 1

    def test_orphan_sources_accumulated(self):
        from oz_transpile.model import OrphanSource
        m1 = OZModule()
        m1.orphan_sources.append(OrphanSource(stem="helpers"))
        m2 = OZModule()

        merged = merge_modules([m1, m2])
        assert len(merged.orphan_sources) == 1

    def test_source_paths_merged(self):
        from pathlib import Path
        m1 = OZModule()
        m1.source_paths["Foo"] = Path("/a/Foo.m")
        m2 = OZModule()
        m2.source_paths["Bar"] = Path("/b/Bar.m")

        merged = merge_modules([m1, m2])
        assert merged.source_paths["Foo"] == Path("/a/Foo.m")
        assert merged.source_paths["Bar"] == Path("/b/Bar.m")

    def test_source_stem_fill_in(self):
        m1 = OZModule()
        m1.classes["Foo"] = OZClass("Foo")
        m2 = OZModule()
        m2.classes["Foo"] = OZClass("Foo")
        m2.classes["Foo"].source_stem = "FooImpl"

        merged = merge_modules([m1, m2])
        assert merged.classes["Foo"].source_stem == "FooImpl"

    def test_empty_modules(self):
        merged = merge_modules([OZModule(), OZModule()])
        assert len(merged.classes) == 0
        assert len(merged.protocols) == 0

    def test_single_module(self):
        m = OZModule()
        m.classes["Foo"] = OZClass("Foo")
        merged = merge_modules([m])
        assert "Foo" in merged.classes


class TestDiagnostics:
    def test_unsupported_selector_skipped(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Proxy : OZObject
- (void)forwardInvocation:(id)inv;
@end
@implementation Proxy
- (void)forwardInvocation:(id)inv {}
@end
""")
        assert not any(m.selector == "forwardInvocation:"
                       for m in mod.classes["Proxy"].methods)
        assert any("forwardInvocation:" in e for e in mod.errors)

    def test_try_stmt_produces_error(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)doWork;
@end
@implementation Foo
- (void)doWork {
        @try {
        } @catch (id e) {
        }
}
@end
""")
        assert any("@try" in e for e in mod.errors)

    def test_weak_property_error(self):
        mod = clang_collect("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
@property (nonatomic, weak) id delegate;
@end
""")
        assert not any(p.name == "delegate"
                       for p in mod.classes["Foo"].properties)
        assert any("weak" in e for e in mod.errors)
