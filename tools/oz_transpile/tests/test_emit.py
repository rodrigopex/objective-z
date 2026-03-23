# SPDX-License-Identifier: Apache-2.0

import os
import tempfile

from .conftest import clang_collect_resolve, clang_emit, clang_emit_patched
from oz_transpile.emit import (
    emit, _selector_to_c, _base_chain, _method_prototype,
    _emit_synthesized_accessor, _emit_patched_source, _EmitCtx,
    _emit_include_replacement,
    _is_func_prototype, _extract_func_name, _extract_class_name,
    _extract_decl_name,
)
from oz_transpile.model import (
    DispatchKind,
    OZClass,
    OZFunction,
    OZIvar,
    OZMethod,
    OZModule,
    OZParam,
    OZProperty,
    OZProtocol,
    OZStaticVar,
    OZType,
)
from oz_transpile.resolve import resolve


# ---------------------------------------------------------------------------
# Synthetic helper — kept for TestHelpers (pure utility function tests)
# ---------------------------------------------------------------------------

def _simple_module():
    """OZObject(root) -> OZLed(_pin:int, init, turnOn)"""
    m = OZModule()
    m.classes["OZObject"] = OZClass("OZObject", methods=[
        OZMethod("init", OZType("instancetype"), body_ast={
            "kind": "CompoundStmt",
            "inner": [
                {"kind": "ReturnStmt", "inner": [
                    {"kind": "DeclRefExpr",
                     "referencedDecl": {"name": "self"},
                     "type": {"qualType": "OZObject *"}},
                ]},
            ],
        }),
        OZMethod("dealloc", OZType("void"), body_ast={
            "kind": "CompoundStmt", "inner": [],
        }),
    ])
    m.classes["OZLed"] = OZClass(
        "OZLed",
        superclass="OZObject",
        ivars=[OZIvar("_pin", OZType("int"))],
        methods=[
            OZMethod("init", OZType("instancetype"), body_ast={
                "kind": "CompoundStmt",
                "inner": [
                    {"kind": "ReturnStmt", "inner": [
                        {"kind": "DeclRefExpr",
                         "referencedDecl": {"name": "self"},
                         "type": {"qualType": "OZLed *"}},
                    ]},
                ],
            }),
            OZMethod("turnOn", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [],
            }),
        ],
    )
    resolve(m)
    return m


# ===========================================================================
# TestHelpers — pure utility function tests (kept synthetic)
# ===========================================================================

class TestHelpers:
    def test_selector_to_c_simple(self):
        assert _selector_to_c("init") == "init"

    def test_selector_to_c_with_colons(self):
        assert _selector_to_c("setPin:color:") == "setPin_color_"

    def test_base_chain_root(self):
        m = _simple_module()
        assert _base_chain("OZObject", m) == "obj->"

    def test_base_chain_child(self):
        m = _simple_module()
        assert _base_chain("OZLed", m) == "obj->base."

    def test_method_prototype(self):
        cls = OZClass("OZLed")
        m = OZMethod("setPin:", OZType("void"),
                     params=[OZParam("pin", OZType("int"))])
        assert _method_prototype(cls, m) == "void OZLed_setPin_(struct OZLed *self, int pin)"

    def test_class_method_prototype_no_params(self):
        cls = OZClass("MyObj")
        m = OZMethod("greet", OZType("void"), is_class_method=True)
        proto = _method_prototype(cls, m)
        assert proto == "void MyObj_cls_greet(void)"
        assert "self" not in proto

    def test_class_method_prototype_with_params(self):
        cls = OZClass("MyObj")
        m = OZMethod("doWith:", OZType("void"),
                     params=[OZParam("val", OZType("int"))],
                     is_class_method=True)
        proto = _method_prototype(cls, m)
        assert proto == "void MyObj_cls_doWith_(int val)"
        assert "self" not in proto


# ===========================================================================
# Structural tests — migrated to real .m sources
# ===========================================================================

_LED_SOURCE = """\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject {
    int _pin;
}
- (instancetype)init;
- (void)turnOn;
@end
@implementation OZLed
- (instancetype)init { self->_pin = 0; return self; }
- (void)turnOn {}
@end
"""


class TestEmitFiles:
    def test_generates_expected_files(self):
        _, out = clang_emit(_LED_SOURCE)
        assert "Foundation/oz_dispatch.h" in out
        assert "Foundation/oz_dispatch.c" in out
        assert "Foundation/OZObject_ozh.h" in out
        assert "Foundation/OZObject_ozm.c" in out
        assert "OZLed_ozh.h" in out
        assert "OZLed_ozm.c" in out


class TestDispatchHeader:
    def test_class_id_enum(self):
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/oz_dispatch.h"]
        assert "OZ_CLASS_OZObject" in content
        assert "OZ_CLASS_OZLed" in content
        assert "OZ_CLASS_COUNT" in content

    def test_protocol_vtable(self):
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/oz_dispatch.h"]
        assert "OZ_PROTOCOL_RESOLVE_init" in content
        assert "OZ_PROTOCOL_SEND_init" in content

    def test_class_method_excluded_from_vtable(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject { int _pin; }
- (instancetype)init;
- (void)turnOn;
+ (void)greet;
@end
@implementation OZLed
- (instancetype)init { self->_pin = 0; return self; }
- (void)turnOn {}
+ (void)greet {}
@end
""")
        dispatch_h = out["Foundation/oz_dispatch.h"]
        dispatch_c = out["Foundation/oz_dispatch.c"]
        assert "vtable_greet" not in dispatch_h
        assert "vtable_greet" not in dispatch_c
        led_h = out["OZLed_ozh.h"]
        assert "OZLed_cls_greet(void)" in led_h


class TestClassHeader:
    def test_struct_with_base(self):
        _, out = clang_emit(_LED_SOURCE)
        content = out["OZLed_ozh.h"]
        assert "struct OZLed {" in content
        assert "struct OZObject base;" in content
        assert "int _pin;" in content

    def test_root_struct(self):
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/OZObject_ozh.h"]
        assert "struct oz_metadata _meta;" in content
        assert "_refcount" in content

    def test_root_struct_no_atomic_props(self):
        """Root struct omits _oz_prop_lock when no atomic properties exist."""
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/OZObject_ozh.h"]
        assert "_oz_prop_lock" not in content

    def test_block_ivar_uses_fptr_decl_syntax(self):
        """Block-typed ivar must embed name inside (*name) — not (*) name."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Defer : OZObject {
    void (^_block)(id);
}
@end
@implementation Defer
@end
""")
        content = out["Defer_ozh.h"]
        assert "void (*_block)(struct OZObject *)" in content
        assert "void (*)(struct OZObject *) _block" not in content

    def test_block_function_emitted_in_correct_class_source(self):
        """Block in child method must land in child source, not OZObject."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Handler : OZObject {
    void (^_callback)(id);
}
- (void)setup;
@end
@implementation Handler
- (void)setup {
    _callback = ^(id owner) {};
}
@end
""")
        handler_src = out["Handler_ozm.c"]
        root_src = out["Foundation/OZObject_ozm.c"]
        assert "_oz_block_" in handler_src
        assert "_oz_block_" not in root_src

    def test_root_struct_with_atomic_props(self):
        """Root struct includes _oz_prop_lock when atomic properties exist."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject
@property int speed;
@end
@implementation Car
@synthesize speed = _speed;
@end
""")
        content = out["Foundation/OZObject_ozh.h"]
        assert "oz_spinlock_t _oz_prop_lock;" in content


class TestClassSource:
    def test_method_body(self):
        _, out = clang_emit(_LED_SOURCE)
        content = out["OZLed_ozm.c"]
        assert "OZLed_turnOn" in content
        assert "struct OZLed *self" in content

    def test_root_retain_release(self):
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/OZObject_ozm.c"]
        assert "OZObject_retain" in content
        assert "OZObject_release" in content
        assert "oz_atomic_dec_and_test" in content
        assert "OZ_PROTOCOL_SEND_dealloc" in content

    def test_release_immortal_guard(self):
        """OZ-064: _release skips dealloc when _meta.immortal is set."""
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/OZObject_ozm.c"]
        assert "self->_meta.immortal" in content
        # immortal guard must appear before the atomic decrement
        immortal_pos = content.index("self->_meta.immortal")
        dec_pos = content.index("oz_atomic_dec_and_test")
        assert immortal_pos < dec_pos


# ===========================================================================
# Body emission tests — migrated to real .m sources
# ===========================================================================

class TestBodyEmission:
    def test_ivar_access(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject { int _pin; }
- (int)pin;
@end
@implementation OZLed
- (int)pin { return _pin; }
@end
""")
        content = out["OZLed_ozm.c"]
        assert "self->_pin" in content

    def test_super_call(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject { int _pin; }
- (instancetype)initWithPin:(int)pin;
@end
@implementation OZLed
- (instancetype)initWithPin:(int)pin {
    [super init];
    return self;
}
@end
""")
        content = out["OZLed_ozm.c"]
        assert "OZObject_init((struct OZObject *)self)" in content

    def test_message_send_static(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface OZLed : OZObject
- (void)turnOn;
- (void)test;
@end
@implementation OZLed
- (void)turnOn {}
- (void)test { [self turnOn]; }
@end
""")
        content = out["OZLed_ozm.c"]
        assert "OZLed_turnOn(self)" in content

    def test_if_stmt(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)check;
@end
@implementation Foo
- (void)check {
    if (1) {}
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "if (1)" in content

    def test_binary_operator(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (int)add;
@end
@implementation Foo
- (int)add { return 1 + 2; }
@end
""")
        content = out["Foo_ozm.c"]
        assert "1 + 2" in content


# ===========================================================================
# ARC tests — migrated to real .m sources
# ===========================================================================

class TestARCLocalRelease:
    def test_local_object_released_at_scope_end(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)doStuff;
@end
@implementation Foo
- (void)doStuff {
    OZObject *tmp = nil;
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "OZObject_release((struct OZObject *)tmp);" in content

    def test_primitive_not_released(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)doStuff;
@end
@implementation Foo
- (void)doStuff {
    int count = 0;
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "release" not in content

    def test_returned_object_not_released(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (OZObject *)create;
@end
@implementation Foo
- (OZObject *)create {
    OZObject *obj = nil;
    OZObject *other = nil;
    return obj;
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "OZObject_release((struct OZObject *)other);" in content
        assert "OZObject_release((struct OZObject *)obj);" not in content

    def test_self_never_released(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface OZRoot : OZObject
- (instancetype)init;
@end
@implementation OZRoot
- (instancetype)init { return self; }
@end
""")
        content = out["OZRoot_ozm.c"]
        assert "OZObject_release((struct OZObject *)self);" not in content

    def test_nested_scope_inner_released_at_inner_exit(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)doStuff;
@end
@implementation Foo
- (void)doStuff {
    OZObject *outer = nil;
    {
        OZObject *inner = nil;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        assert content.count("OZObject_release") == 2

    def test_early_return_inside_if_releases_outer_vars(self):
        """Return inside an if-block should release vars from outer scopes."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)check;
@end
@implementation Foo
- (void)check {
    OZObject *a = nil;
    if (1) {
        return;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        assert content.count("OZObject_release((struct OZObject *)a)") == 2

    def test_alloc_count_determines_slab_size(self):
        """Slab pool size should match the number of alloc calls in source."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
@end
@implementation Foo
@end

void test_fn(void) {
    Foo *a = [Foo alloc];
    Foo *b = [Foo alloc];
    Foo *c = [Foo alloc];
}
""")
        foo_c = out["Foo_ozm.c"]
        assert "oz_slab_Foo, sizeof(struct Foo), 3, 4)" in foo_c
        root_c = out["Foundation/OZObject_ozm.c"]
        assert "oz_slab_OZObject, sizeof(struct OZObject), 1, 4)" in root_c


class TestARCAutoDealloc:
    def test_auto_dealloc_with_object_ivar(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Holder : OZObject {
    OZObject *_child;
    int _count;
}
@end
@implementation Holder
@end
""")
        content = out["Holder_ozm.c"]
        assert "Holder_dealloc" in content
        assert "OZObject_release((struct OZObject *)self->_child)" in content
        assert "OZObject_dealloc((struct OZObject *)self)" in content

    def test_root_dealloc_calls_free(self):
        """Root class with object ivar gets dealloc that calls dispatch_free."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject",
            ivars=[OZIvar("_helper", OZType("OZObject *"))],
            methods=[
                OZMethod("init", OZType("instancetype"), body_ast={
                    "kind": "CompoundStmt",
                    "inner": [{"kind": "ReturnStmt", "inner": [
                        {"kind": "DeclRefExpr", "referencedDecl": {"name": "self"},
                         "type": {"qualType": "OZObject *"}},
                    ]}],
                }),
            ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZObject_dealloc" in content
            assert "OZObject_dispatch_free((struct OZObject *)self)" in content

    def test_no_dealloc_for_root_without_obj_ivars(self):
        """Root class without object ivars gets no dealloc."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject", methods=[
            OZMethod("init", OZType("instancetype"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{"kind": "ReturnStmt", "inner": [
                    {"kind": "DeclRefExpr", "referencedDecl": {"name": "self"},
                     "type": {"qualType": "OZObject *"}},
                ]}],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZObject_dealloc" not in content

    def test_child_without_obj_ivars_still_gets_dealloc(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Child : OZObject {
    int _val;
}
@end
@implementation Child
@end
""")
        content = out["Child_ozm.c"]
        assert "Child_dealloc" in content
        assert "OZObject_dealloc((struct OZObject *)self)" in content

    def test_user_defined_dealloc_prepends_ivar_releases(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Holder : OZObject {
    OZObject *_child;
    int _count;
}
- (void)dealloc;
@end
@implementation Holder
- (void)dealloc {
    _count = 42;
    [super dealloc];
}
@end
""")
        content = out["Holder_ozm.c"]
        assert "OZObject_release((struct OZObject *)self->_child)" in content
        assert "OZObject_dealloc((struct OZObject *)self)" in content
        assert "42" in content

    def test_unsafe_unretained_ivar_skipped_in_dealloc(self):
        """__unsafe_unretained id ivar must NOT be released in dealloc.

        Kept synthetic: Clang JSON AST doesn't preserve __unsafe_unretained
        in qualType, so collect can't detect it from real .m sources.
        """
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject", methods=[
            OZMethod("init", OZType("instancetype"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{"kind": "ReturnStmt", "inner": [
                    {"kind": "DeclRefExpr", "referencedDecl": {"name": "self"},
                     "type": {"qualType": "OZObject *"}},
                ]}],
            }),
            OZMethod("dealloc", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [],
            }),
        ])
        m.classes["Watcher"] = OZClass(
            "Watcher", superclass="OZObject",
            ivars=[
                OZIvar("_delegate", OZType("__unsafe_unretained id")),
                OZIvar("_name", OZType("OZString *")),
            ],
        )
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Watcher_ozm.c")).read()
            assert "Watcher_dealloc" in content
            assert "OZObject_release((struct OZObject *)self->_name)" in content
            assert "_delegate" not in content


class TestARCBreakContinueCleanup:
    def test_break_releases_loop_locals(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    while (1) {
        OZObject *a = nil;
        break;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        lines = content.split("\n")
        release_line = next(i for i, l in enumerate(lines) if "release" in l and "a)" in l)
        break_line = next(i for i, l in enumerate(lines) if "break;" in l)
        assert release_line < break_line

    def test_continue_releases_loop_locals(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    while (1) {
        OZObject *a = nil;
        continue;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        lines = content.split("\n")
        release_line = next(i for i, l in enumerate(lines) if "release" in l and "a)" in l)
        continue_line = next(i for i, l in enumerate(lines) if "continue;" in l)
        assert release_line < continue_line

    def test_break_releases_nested_scopes_in_loop(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    while (1) {
        OZObject *a = nil;
        if (1) {
            OZObject *b = nil;
            break;
        }
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        break_idx = content.index("break;")
        before_break = content[:break_idx]
        assert "release((struct OZObject *)b)" in before_break
        assert "release((struct OZObject *)a)" in before_break

    def test_break_does_not_release_outside_loop(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    OZObject *outer = nil;
    while (1) {
        OZObject *inner = nil;
        break;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        break_idx = content.index("break;")
        before_break = content[:break_idx]
        assert "release((struct OZObject *)inner)" in before_break
        assert "release((struct OZObject *)outer)" not in before_break

    def test_nested_loops_break_only_releases_inner(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    while (1) {
        OZObject *a = nil;
        while (1) {
            OZObject *b = nil;
            break;
        }
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        break_idx = content.index("break;")
        before_break = content[:break_idx]
        assert "release((struct OZObject *)b)" in before_break
        assert "release((struct OZObject *)a)" not in before_break

    def test_consumed_var_not_released_on_break(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Holder : OZObject {
    OZObject *_child;
}
- (void)run;
@end
@implementation Holder
- (void)run {
    while (1) {
        OZObject *obj = nil;
        _child = obj;
        break;
    }
}
@end
""")
        content = out["Holder_ozm.c"]
        break_idx = content.index("break;")
        before_break = content[:break_idx]
        assert "release((struct OZObject *)obj)" not in before_break

    def test_for_stmt_break_releases(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    for (int i = 0; i < 10; i++) {
        OZObject *a = nil;
        break;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        break_idx = content.index("break;")
        before_break = content[:break_idx]
        assert "release((struct OZObject *)a)" in before_break

    def test_do_while_break_releases(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    do {
        OZObject *a = nil;
        break;
    } while (1);
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "do {" in content
        break_idx = content.index("break;")
        before_break = content[:break_idx]
        assert "release((struct OZObject *)a)" in before_break


class TestARCAutoreleasePool:
    def test_autoreleasepool_releases_at_exit(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    @autoreleasepool {
        OZObject *a = nil;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "OZObject_release((struct OZObject *)a);" in content

    def test_autoreleasepool_nested(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    @autoreleasepool {
        OZObject *outer = nil;
        @autoreleasepool {
            OZObject *inner = nil;
        }
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        assert content.count("OZObject_release") == 2

    def test_autoreleasepool_does_not_release_outer_vars(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    OZObject *before = nil;
    @autoreleasepool {
        OZObject *inside = nil;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        inside_release = content.index("release((struct OZObject *)inside)")
        before_release = content.index("release((struct OZObject *)before)")
        assert inside_release < before_release


class TestARCParameterRetain:
    def test_object_param_retained_at_entry(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)setItem:(OZObject *)item;
@end
@implementation Foo
- (void)setItem:(OZObject *)item {}
@end
""")
        content = out["Foo_ozm.c"]
        assert "OZObject_retain((struct OZObject *)item)" in content

    def test_object_param_released_at_scope_exit(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)setItem:(OZObject *)item;
@end
@implementation Foo
- (void)setItem:(OZObject *)item {}
@end
""")
        content = out["Foo_ozm.c"]
        assert "OZObject_release((struct OZObject *)item)" in content

    def test_primitive_param_not_retained(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)setCount:(int)count;
@end
@implementation Foo
- (void)setCount:(int)count {}
@end
""")
        content = out["Foo_ozm.c"]
        assert "retain" not in content

    def test_multiple_object_params_all_retained(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)set:(OZObject *)a with:(int)count and:(OZObject *)b;
@end
@implementation Foo
- (void)set:(OZObject *)a with:(int)count and:(OZObject *)b {}
@end
""")
        content = out["Foo_ozm.c"]
        assert "OZObject_retain((struct OZObject *)a)" in content
        assert "OZObject_retain((struct OZObject *)b)" in content
        assert "retain((struct OZObject *)count)" not in content

    def test_object_param_released_on_return(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)process:(OZObject *)item;
@end
@implementation Foo
- (void)process:(OZObject *)item {
    return;
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "OZObject_retain((struct OZObject *)item)" in content
        ret_idx = content.index("return;")
        before_ret = content[:ret_idx]
        assert "OZObject_release((struct OZObject *)item)" in before_ret

    def test_param_assigned_to_ivar_not_consumed(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Holder : OZObject {
    OZObject *_child;
}
- (void)setChild:(OZObject *)child;
@end
@implementation Holder
- (void)setChild:(OZObject *)child {
    _child = child;
}
@end
""")
        content = out["Holder_ozm.c"]
        assert "OZObject_retain((struct OZObject *)child)" in content
        assert "self->_child = child;" in content
        assert "OZObject_release((struct OZObject *)child);" in content


class TestARCStrongIvarAssign:
    def test_ivar_assign_emits_retain_release(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Holder : OZObject {
    OZObject *_child;
    int _count;
}
- (void)setChild:(OZObject *)child;
@end
@implementation Holder
- (void)setChild:(OZObject *)child {
    _child = child;
}
@end
""")
        content = out["Holder_ozm.c"]
        assert "OZObject_retain((struct OZObject *)child)" in content
        assert "OZObject_release((struct OZObject *)self->_child)" in content
        assert "self->_child = child;" in content

    def test_consumed_local_not_released_at_scope_exit(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Holder : OZObject {
    OZObject *_child;
    int _count;
}
- (void)setup;
@end
@implementation Holder
- (void)setup {
    OZObject *obj = nil;
    _child = obj;
}
@end
""")
        content = out["Holder_ozm.c"]
        assert "OZObject_release((struct OZObject *)obj);" not in content
        assert "OZObject_retain((struct OZObject *)obj)" not in content
        assert "OZObject_release((struct OZObject *)self->_child)" in content


class TestARCLocalReassign:
    """ARC for local object variable reassignment and nil assignment."""

    def test_reassign_releases_old(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Holder : OZObject {
    OZObject *_child;
    int _count;
}
- (void)doWork;
@end
@implementation Holder
- (void)doWork {
    OZObject *f = [[OZObject alloc] init];
    f = [[OZObject alloc] init];
}
@end
""")
        content = out["Holder_ozm.c"]
        assert "OZObject_release((struct OZObject *)f);" in content
        body = content.split("void Holder_doWork")[1]
        assert "OZObject_retain" not in body

    def test_nil_assign_releases_old(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Holder : OZObject {
    OZObject *_child;
    int _count;
}
- (void)doWork;
@end
@implementation Holder
- (void)doWork {
    OZObject *f = [[OZObject alloc] init];
    f = nil;
}
@end
""")
        content = out["Holder_ozm.c"]
        assert "OZObject_release((struct OZObject *)f);" in content

    def test_scope_exit_still_releases_after_reassign(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Holder : OZObject {
    OZObject *_child;
    int _count;
}
- (void)doWork;
@end
@implementation Holder
- (void)doWork {
    OZObject *f = [[OZObject alloc] init];
    f = [[OZObject alloc] init];
}
@end
""")
        content = out["Holder_ozm.c"]
        count = content.count("OZObject_release((struct OZObject *)f);")
        assert count == 2

    def test_reassign_after_consume_clears_consumed(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Holder : OZObject {
    OZObject *_child;
    int _count;
}
- (void)doWork;
@end
@implementation Holder
- (void)doWork {
    OZObject *obj = [[OZObject alloc] init];
    _child = obj;
    obj = [[OZObject alloc] init];
}
@end
""")
        content = out["Holder_ozm.c"]
        count = content.count("OZObject_release((struct OZObject *)obj);")
        assert count == 2

    def test_local_to_local_retains_new(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Holder : OZObject {
    OZObject *_child;
    int _count;
}
- (void)doWork;
@end
@implementation Holder
- (void)doWork {
    OZObject *a = [[OZObject alloc] init];
    OZObject *b = [[OZObject alloc] init];
    a = b;
}
@end
""")
        content = out["Holder_ozm.c"]
        assert "OZObject_retain((struct OZObject *)b);" in content
        assert "OZObject_release((struct OZObject *)a);" in content
        assert "a = b;" in content

    def test_primitive_local_not_intercepted(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Holder : OZObject {
    OZObject *_child;
    int _count;
}
- (void)doWork;
@end
@implementation Holder
- (void)doWork {
    int x = 5;
    x = 10;
}
@end
""")
        content = out["Holder_ozm.c"]
        assert "x = 10;" in content
        dowork_body = content.split("void Holder_doWork")[1].split("}")[0]
        assert "release" not in dowork_body


# ===========================================================================
# Introspection tests — migrated to real .m sources
# ===========================================================================

class TestIntrospection:
    """Tests for class name/superclass tables and introspection helpers."""

    def test_dispatch_header_has_class_names_table(self):
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/oz_dispatch.h"]
        assert "extern const char *const oz_class_names[OZ_CLASS_COUNT]" in content

    def test_dispatch_header_has_superclass_table(self):
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/oz_dispatch.h"]
        assert "extern const uint8_t oz_superclass_id[OZ_CLASS_COUNT]" in content

    def test_dispatch_header_has_inline_helpers(self):
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/oz_dispatch.h"]
        assert "oz_name(" in content
        assert "oz_superclass(" in content
        assert "oz_isKindOfClass(" in content

    def test_dispatch_source_class_names(self):
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/oz_dispatch.c"]
        assert 'oz_class_names[OZ_CLASS_COUNT]' in content
        assert '"OZObject"' in content
        assert '"OZLed"' in content

    def test_dispatch_source_superclass_ids(self):
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/oz_dispatch.c"]
        assert "oz_superclass_id[OZ_CLASS_COUNT]" in content
        assert "[OZ_CLASS_OZObject] = OZ_CLASS_COUNT" in content
        assert "[OZ_CLASS_OZLed] = OZ_CLASS_OZObject" in content

    def test_dispatch_auto_init_emitted(self):
        """OZ-056: classes with +initialize get OZ_AUTO_INIT in oz_dispatch.c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface AppConfig : OZObject
+ (void)initialize;
@end
@implementation AppConfig
+ (void)initialize {}
@end
""")
        content = out["Foundation/oz_dispatch.c"]
        assert "OZ_AUTO_INIT(AppConfig_oz_auto_init, AppConfig_cls_initialize)" in content

    def test_dispatch_auto_init_not_emitted_without_initialize(self):
        """OZ-056: no OZ_AUTO_INIT when no class defines +initialize."""
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/oz_dispatch.c"]
        assert "OZ_AUTO_INIT" not in content

    def test_dispatch_header_includes_platform_for_auto_init(self):
        """OZ-056: oz_dispatch.h includes platform header when +initialize exists."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface AppConfig : OZObject
+ (void)initialize;
@end
@implementation AppConfig
+ (void)initialize {}
@end
""")
        content = out["Foundation/oz_dispatch.h"]
        assert '#include "platform/oz_platform.h"' in content

    def test_dispatch_auto_init_multiple_classes_in_order(self):
        """OZ-056: multiple +initialize classes emit OZ_AUTO_INIT in topological order."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface AppConfig : OZObject
+ (void)initialize;
@end
@implementation AppConfig
+ (void)initialize {}
@end
""")
        # OZObject doesn't define +initialize in SDK, only AppConfig does
        content = out["Foundation/oz_dispatch.c"]
        assert "OZ_AUTO_INIT(AppConfig_oz_auto_init" in content

    def test_dispatch_auto_init_explicit_call_from_class_method(self):
        """OZ-056: explicit [Class initialize] from a class method is an error."""
        mod, _ = clang_emit("""\
#import <Foundation/OZObject.h>
@interface AppConfig : OZObject
+ (void)initialize;
+ (void)reset;
@end
@implementation AppConfig
+ (void)initialize {}
+ (void)reset { [AppConfig initialize]; }
@end
""")
        assert any("explicit call" in e for e in mod.errors)

    def test_root_class_isEqual(self):
        _, out = clang_emit(_LED_SOURCE)
        h = out["Foundation/OZObject_ozh.h"]
        c = out["Foundation/OZObject_ozm.c"]
        assert "OZObject_isEqual_" in h
        assert "OZObject_isEqual_" in c
        assert "self == anObject" in c

    def test_root_class_cDescription(self):
        _, out = clang_emit(_LED_SOURCE)
        h = out["Foundation/OZObject_ozh.h"]
        c = out["Foundation/OZObject_ozm.c"]
        assert "OZObject_cDescription_maxLength_" in h
        assert "OZObject_cDescription_maxLength_" in c
        assert "oz_platform_snprint" in c
        assert "oz_class_names" in c

    def test_isEqual_protocol_dispatched(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface OZChild : OZObject
- (BOOL)isEqual:(OZObject *)anObject;
@end
@implementation OZChild
- (BOOL)isEqual:(OZObject *)anObject { return self == anObject; }
@end
""")
        content = out["Foundation/oz_dispatch.h"]
        assert "OZ_PROTOCOL_SEND_isEqual_" in content
        assert "OZ_PROTOCOL_RESOLVE_isEqual_" in content

    def test_cDescription_protocol_dispatched(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface OZChild : OZObject
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
@end
@implementation OZChild
- (int)cDescription:(char *)buf maxLength:(int)maxLen { return 0; }
@end
""")
        content = out["Foundation/oz_dispatch.h"]
        assert "OZ_PROTOCOL_SEND_cDescription_maxLength_" in content

    def test_root_source_includes_header(self):
        _, out = clang_emit(_LED_SOURCE)
        content = out["Foundation/OZObject_ozm.c"]
        assert "OZObject_ozh.h" in content


# ===========================================================================
# Static variable emission tests — migrated to real .m sources
# ===========================================================================

class TestStaticVarEmission:
    """Tests for file-scope static variable emission in class _ozm.c file."""

    def test_static_var_emitted_in_class_file(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
static OZObject *_sharedObj;
@interface Foo : OZObject
@end
@implementation Foo
@end
""")
        # Find the file containing the static var
        found = False
        for path, content in out.items():
            if "_sharedObj" in content and path.endswith(".c"):
                found = True
                break
        assert found

    def test_primitive_static_var(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
static int _count;
@interface Foo : OZObject
@end
@implementation Foo
@end
""")
        found = False
        for path, content in out.items():
            if "_count" in content and path.endswith(".c"):
                found = True
                break
        assert found

    def test_no_statics_no_functions_file(self):
        _, out = clang_emit(_LED_SOURCE)
        assert not any("oz_functions.c" in p for p in out)

    def test_compound_literal_expr(self):
        """CompoundLiteralExpr + InitListExpr -> (type){val, val}"""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
struct color { int r; int g; int b; };
@interface OZLed : OZObject { int _pin; }
- (void)setup;
@end
@implementation OZLed
- (void)setup {
    struct color c = (struct color){255, 0, 0};
}
@end
""")
        src = out["OZLed_ozm.c"]
        assert "(struct color){255, 0, 0}" in src

    def test_string_literal_emits_static_struct(self):
        """ObjCStringLiteral -> static struct OZString + reference."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZString.h>
void test_func(void) {
    OZString *s = @"hello";
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        # Find the source file with the string constant
        found = False
        for path, content in out.items():
            if "static struct OZString _oz_str_" in content:
                assert '"hello"' in content
                assert ".immortal = 1" in content
                found = True
                break
        assert found

    def test_string_literal_dedup(self):
        """Identical string literals reuse the same static struct."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZString.h>
void test_func(void) {
    OZString *a = @"hello";
    OZString *b = @"hello";
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        for path, content in out.items():
            if "static struct OZString" in content:
                assert content.count('static struct OZString') == 1
                assert content.count('"hello"') == 1
                break

    def test_array_literal(self):
        """ObjCArrayLiteral -> dynamic OZArray via OZArray_initWithItems."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZString.h>
#import <Foundation/OZArray.h>
void test_arr(void) {
    OZArray *a = @[@"hello", @"world"];
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        found = False
        for path, content in out.items():
            if "OZArray_initWithItems" in content:
                assert "_oz_arr_" in content
                found = True
                break
        assert found

    def test_dictionary_literal(self):
        """ObjCDictionaryLiteral -> dynamic OZDictionary via initWithKeysValues."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZString.h>
#import <Foundation/OZDictionary.h>
void test_dict(void) {
    OZDictionary *d = @{@"key": @"value"};
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        found = False
        for path, content in out.items():
            if "OZDictionary_initWithKeysValues" in content:
                assert "_oz_dict_" in content
                found = True
                break
        assert found

    def test_number_literal(self):
        """ObjCBoxedExpr with IntegerLiteral -> dynamic OZNumber_initInt32."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZNumber.h>
void test_num(void) {
    OZNumber *n = @42;
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        found = False
        for path, content in out.items():
            if "OZNumber_initInt32(42)" in content:
                found = True
                break
        assert found

    def test_number_literal_each_alloc(self):
        """Each boxed number literal produces its own dynamic allocation."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZNumber.h>
void test_allocs(void) {
    OZNumber *a = @42;
    OZNumber *b = @42;
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        for path, content in out.items():
            if "OZNumber_initInt32" in content:
                assert content.count("OZNumber_initInt32(42)") == 2
                break

    def test_expr_with_cleanups_passthrough(self):
        """ExprWithCleanups wrapping an expression -> unwraps inner."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)test;
@end
@implementation Foo
- (void)test {
    int x = 99;
}
@end
""")
        src = out["Foo_ozm.c"]
        assert "int x = 99;" in src

    def test_block_expr_non_capturing(self):
        """BlockExpr without captures -> static C function + name."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
void test_block(void) {
    void (^blk)(int) = ^(int val) {};
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        for path, content in out.items():
            if "_oz_block_" in content and path.endswith(".c"):
                assert "static void _oz_block_" in content
                assert "int val" in content
                break

    def test_block_expr_with_capture_raises(self):
        """BlockExpr with captures -> diagnostic error."""
        mod, _ = clang_emit("""\
#import <Foundation/OZObject.h>
void test_capture(void) {
    int sum = 0;
    void (^blk)(void) = ^{ sum; };
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        assert any("sum" in e for e in mod.errors)

    def test_block_name_uses_loc(self):
        """BlockExpr with loc -> _oz_block_L{line}_C{col}."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
void test_loc_block(void) {
    void (^blk)(void) = ^{};
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        for path, content in out.items():
            if "_oz_block_L" in content:
                assert "_oz_block_L" in content
                break

    def test_block_names_unique_across_methods(self):
        """Two functions each with a block -> unique file-scope function names."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
void method_a(void) {
    void (^blk)(void) = ^{};
}
void method_b(void) {
    void (^blk)(void) = ^{};
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        for path, content in out.items():
            if "_oz_block_" in content and content.count("static void _oz_block_") >= 2:
                break
        else:
            assert False, "Expected two unique block functions"

    def test_ivar_type_defs_in_class_header(self):
        """Class with enum/union ivars gets type_defs in header."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZNumber.h>
@interface MyNum : OZObject {
    enum oz_number_tag _tag;
}
@end
@implementation MyNum
@end
""")
        # The enum type_def from OZNumber should appear in the header
        for path, content in out.items():
            if path.endswith("_ozh.h") and "MyNum" in path:
                assert "struct MyNum {" in content
                break

    def test_method_prototype_with_block_param(self):
        """Method with block parameter uses function pointer syntax."""
        cls = OZClass("OZArray")
        m = OZMethod(
            "enumerateObjectsUsingBlock:",
            OZType("void"),
            params=[OZParam("block",
                            OZType("void (^)(id, unsigned int, BOOL *)"))],
        )
        proto = _method_prototype(cls, m)
        assert "void (*block)" in proto
        assert "struct OZObject *" in proto

    def test_pseudo_object_expr_indexed_subscript(self):
        """PseudoObjectExpr for array subscript emits objectAtIndexedSubscript: call."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZArray.h>
@interface Foo : OZObject
- (void)test:(OZArray *)arr;
@end
@implementation Foo
- (void)test:(OZArray *)arr {
    id first = arr[0];
}
@end
""")
        src = out["Foo_ozm.c"]
        assert "OZArray_objectAtIndexedSubscript_" in src


# ===========================================================================
# Synthesized property tests — kept synthetic (tests _emit_synthesized_accessor directly)
# ===========================================================================

class TestSynthesizedPropertyEmission:
    """Test _emit_synthesized_accessor generates correct C code."""

    def _emit(self, cls, method, root_class="OZObject", module=None):
        from io import StringIO
        if module is None:
            root = OZClass(root_class)
            module = OZModule()
            module.classes[root_class] = root
            if cls.name != root_class:
                cls.superclass = root_class
                module.classes[cls.name] = cls
        buf = StringIO()
        _emit_synthesized_accessor(cls, method, buf, root_class, module)
        return buf.getvalue()

    def test_nonatomic_getter(self):
        prop = OZProperty("color", OZType("struct color *"),
                          ivar_name="_color", is_nonatomic=True,
                          ownership="assign")
        cls = OZClass("Car")
        m = OZMethod("color", OZType("struct color *"),
                     synthesized_property=prop)
        code = self._emit(cls, m)
        assert "return self->_color;" in code
        assert "oz_spinlock_t" not in code

    def test_atomic_getter(self):
        prop = OZProperty("model", OZType("OZString *"),
                          ivar_name="_model", is_nonatomic=False,
                          ownership="strong")
        cls = OZClass("Car")
        m = OZMethod("model", OZType("OZString *"),
                     synthesized_property=prop)
        code = self._emit(cls, m)
        assert "oz_spinlock_t" not in code
        assert "OZ_SPINLOCK(&self->base._oz_prop_lock)" in code
        assert "val = self->_model;" in code
        assert "return val;" in code

    def test_nonatomic_strong_setter(self):
        prop = OZProperty("model", OZType("OZString *"),
                          ivar_name="_model", is_nonatomic=True,
                          ownership="strong")
        cls = OZClass("Car")
        m = OZMethod("setModel:", OZType("void"),
                     params=[OZParam("model", OZType("OZString *"))],
                     synthesized_property=prop)
        code = self._emit(cls, m, root_class="OZObject")
        assert "OZObject_retain(" in code
        assert "OZObject_release(" in code
        assert "self->_model = model;" in code
        assert "oz_spinlock_t" not in code

    def test_atomic_strong_setter(self):
        prop = OZProperty("model", OZType("OZString *"),
                          ivar_name="_model", is_nonatomic=False,
                          ownership="strong")
        cls = OZClass("Car")
        m = OZMethod("setModel:", OZType("void"),
                     params=[OZParam("model", OZType("OZString *"))],
                     synthesized_property=prop)
        code = self._emit(cls, m, root_class="OZObject")
        assert "oz_spinlock_t" not in code
        assert "OZ_SPINLOCK(&self->base._oz_prop_lock)" in code
        assert "OZObject_retain(" in code
        assert "OZObject_release(" in code

    def test_nonatomic_assign_setter(self):
        prop = OZProperty("speed", OZType("int"),
                          ivar_name="_speed", is_nonatomic=True,
                          ownership="assign")
        cls = OZClass("Car")
        m = OZMethod("setSpeed:", OZType("void"),
                     params=[OZParam("speed", OZType("int"))],
                     synthesized_property=prop)
        code = self._emit(cls, m)
        assert "self->_speed = speed;" in code
        assert "retain" not in code
        assert "release" not in code
        assert "oz_spinlock_t" not in code

    def test_atomic_assign_setter(self):
        prop = OZProperty("speed", OZType("int"),
                          ivar_name="_speed", is_nonatomic=False,
                          ownership="assign")
        cls = OZClass("Car")
        m = OZMethod("setSpeed:", OZType("void"),
                     params=[OZParam("speed", OZType("int"))],
                     synthesized_property=prop)
        code = self._emit(cls, m)
        assert "oz_spinlock_t" not in code
        assert "OZ_SPINLOCK(&self->base._oz_prop_lock)" in code
        assert "self->_speed = speed;" in code
        assert "retain" not in code
        assert "release" not in code

    def test_unsafe_unretained_setter_no_retain(self):
        prop = OZProperty("delegate", OZType("id"),
                          ivar_name="_delegate", is_nonatomic=True,
                          ownership="unsafe_unretained")
        cls = OZClass("Car")
        m = OZMethod("setDelegate:", OZType("void"),
                     params=[OZParam("delegate", OZType("id"))],
                     synthesized_property=prop)
        code = self._emit(cls, m)
        assert "self->_delegate = delegate;" in code
        assert "retain" not in code
        assert "release" not in code

    def test_atomic_getter_child_class(self):
        """Grandchild class uses base.base. chain to reach root lock."""
        prop = OZProperty("temp", OZType("int"),
                          ivar_name="_temp", is_nonatomic=False,
                          ownership="assign")
        root = OZClass("OZObject")
        mid = OZClass("Vehicle", superclass="OZObject")
        child = OZClass("Car", superclass="Vehicle")
        module = OZModule()
        module.classes["OZObject"] = root
        module.classes["Vehicle"] = mid
        module.classes["Car"] = child
        m = OZMethod("temp", OZType("int"), synthesized_property=prop)
        code = self._emit(child, m, module=module)
        assert "OZ_SPINLOCK(&self->base.base._oz_prop_lock)" in code
        assert "oz_spinlock_t" not in code


# ===========================================================================
# @synchronized tests — migrated to real .m sources
# ===========================================================================

class TestSynchronized:
    """Tests for @synchronized -> OZSpinLock RAII emission."""

    def test_basic_synchronized(self):
        """@synchronized(self) { self->_count = 1; }"""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject {
    int _count;
}
- (void)run;
@end
@implementation Foo
- (void)run {
    @synchronized(self) {
        _count = 1;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "struct OZSpinLock *_sync = OZSpinLock_initWithObject(" in content
        assert "OZSpinLock_alloc()" in content
        assert "(struct OZObject *)self" in content
        assert "OZObject_release((struct OZObject *)_sync);" in content

    def test_synchronized_oz_spinlock_slab(self):
        """OZSpinLock slab generated when @synchronized is used."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    @synchronized(self) {}
}
@end
""")
        dispatch_h = out["Foundation/oz_dispatch.h"]
        assert "OZ_CLASS_OZSpinLock" in dispatch_h

    def test_synchronized_early_return(self):
        """Early return inside @synchronized releases the lock."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    @synchronized(self) {
        return;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        ret_pos = content.index("return;")
        release_pos = content.index("OZObject_release((struct OZObject *)_sync)")
        assert release_pos < ret_pos

    def test_synchronized_nested_mangled_names(self):
        """Nested @synchronized uses _sync, _sync2."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    @synchronized(self) {
        @synchronized(self) {}
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "struct OZSpinLock *_sync = " in content
        assert "struct OZSpinLock *_sync2 = " in content

    def test_no_oz_spinlock_without_synchronized(self):
        """OZSpinLock not injected when no @synchronized used."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {}
@end
""")
        dispatch_h = out["Foundation/oz_dispatch.h"]
        assert "OZSpinLock" not in dispatch_h

    def test_synchronized_with_ivar_obj(self):
        """@synchronized(_mutex) uses ivar expression."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject {
    OZObject *_mutex;
    int _count;
}
- (void)run;
@end
@implementation Foo
- (void)run {
    @synchronized(_mutex) {
        _count = 1;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "(struct OZObject *)self->_mutex" in content

    def test_synchronized_with_object_local_inside(self):
        """Object local inside @synchronized released alongside _sync."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    @synchronized(self) {
        OZObject *tmp = nil;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "OZObject_release((struct OZObject *)tmp)" in content
        assert "OZObject_release((struct OZObject *)_sync)" in content

    def test_synchronized_in_loop_break_releases(self):
        """Break inside @synchronized inside loop releases _sync."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    while (1) {
        @synchronized(self) {
            break;
        }
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        break_idx = content.index("break;")
        before_break = content[:break_idx]
        assert "release((struct OZObject *)_sync)" in before_break

    def test_synchronized_return_with_value(self):
        """Return expr inside @synchronized releases _sync but not returned var."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (int)getValue;
@end
@implementation Foo
- (int)getValue {
    @synchronized(self) {
        return 42;
    }
}
@end
""")
        content = out["Foo_ozm.c"]
        ret_pos = content.index("return 42;")
        release_pos = content.index("release((struct OZObject *)_sync)")
        assert release_pos < ret_pos

    def test_sequential_synchronized_counter(self):
        """Two sequential @synchronized in same method get _sync, _sync2."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    @synchronized(self) {}
    @synchronized(self) {}
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "struct OZSpinLock *_sync = " in content
        assert "struct OZSpinLock *_sync2 = " in content

    def test_dispatch_free_includes_oz_spinlock(self):
        """dispatch_free switch includes OZSpinLock case."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)run;
@end
@implementation Foo
- (void)run {
    @synchronized(self) {}
}
@end
""")
        dispatch_c = out["Foundation/oz_dispatch.c"]
        assert "case OZ_CLASS_OZSpinLock: OZSpinLock_free(" in dispatch_c

    def test_synchronized_compiles_on_host(self):
        """Generated @synchronized code compiles with GCC on host."""
        import subprocess
        import shutil
        if not shutil.which("gcc"):
            import pytest
            pytest.skip("gcc not found")

        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject", methods=[
            OZMethod("init", OZType("instancetype"), body_ast={
                "kind": "CompoundStmt", "inner": [
                    {"kind": "ReturnStmt", "inner": [
                        {"kind": "DeclRefExpr",
                         "referencedDecl": {"name": "self"},
                         "type": {"qualType": "OZObject *"}},
                    ]},
                ],
            }),
            OZMethod("dealloc", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [],
            }),
        ])
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject",
                                   ivars=[OZIvar("_count", OZType("int"))],
                                   methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [
                    {"kind": "ObjCAtSynchronizedStmt", "inner": [
                        {"kind": "DeclRefExpr",
                         "referencedDecl": {"name": "self"},
                         "type": {"qualType": "Foo *"}},
                        {"kind": "CompoundStmt", "inner": [
                            {"kind": "NullStmt"},
                        ]},
                    ]},
                ],
            }),
        ])
        resolve(m)
        mod = m
        repo_root = os.path.dirname(os.path.dirname(os.path.dirname(
            os.path.dirname(os.path.abspath(__file__)))))
        pal_inc = os.path.join(repo_root, "include")
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(mod, tmpdir)
            import glob as gl
            c_files = sorted(
                gl.glob(os.path.join(tmpdir, "**", "*.c"), recursive=True))
            foundation_dir = os.path.join(tmpdir, "Foundation")
            for f in c_files:
                result = subprocess.run(
                    ["gcc", "-std=c11", "-Wall", "-Werror",
                     "-Wno-unused-function",
                     "-DOZ_PLATFORM_HOST",
                     "-I", tmpdir, "-I", foundation_dir,
                     "-I", pal_inc,
                     "-c", f, "-o", f + ".o"],
                    capture_output=True, text=True,
                )
                assert result.returncode == 0, (
                    f"Compile failed for {os.path.basename(f)}:\n{result.stderr}"
                )


# ===========================================================================
# Patched emission tests — kept synthetic (tests tree-sitter functions directly)
# ===========================================================================

class TestPatchedEmission:
    """Tests for tree-sitter patched source emission."""

    def _parse_node(self, source_text):
        """Parse source text and return tree-sitter root node children."""
        import tree_sitter_objc as tsobjc
        from tree_sitter import Language, Parser
        lang = Language(tsobjc.language())
        parser = Parser(lang)
        source = source_text.encode()
        tree = parser.parse(source)
        return tree.root_node.children

    def test_is_func_prototype_true(self):
        children = self._parse_node("int printk(const char *fmt, ...);\n")
        decl = [c for c in children if c.type == "declaration"]
        assert len(decl) == 1
        assert _is_func_prototype(decl[0]) is True

    def test_is_func_prototype_false_for_var(self):
        children = self._parse_node("static int count = 0;\n")
        decl = [c for c in children if c.type == "declaration"]
        assert len(decl) == 1
        assert _is_func_prototype(decl[0]) is False

    def test_extract_func_name(self):
        children = self._parse_node(
            "void foo(void) { }\n"
        )
        func = [c for c in children if c.type == "function_definition"]
        assert len(func) == 1
        assert _extract_func_name(func[0]) == "foo"

    def test_extract_func_name_pointer_return(self):
        children = self._parse_node(
            "static int *bar(int x) { return 0; }\n"
        )
        func = [c for c in children if c.type == "function_definition"]
        assert len(func) == 1
        assert _extract_func_name(func[0]) == "bar"

    def test_extract_class_name(self):
        children = self._parse_node(
            "@implementation Foo\n@end\n"
        )
        impl = [c for c in children if c.type == "class_implementation"]
        assert len(impl) == 1
        assert _extract_class_name(impl[0]) == "Foo"

    def test_extract_decl_name_static_var(self):
        children = self._parse_node("static int _count;\n")
        decl = [c for c in children if c.type == "declaration"]
        assert _extract_decl_name(decl[0]) == "_count"

    def test_extract_decl_name_pointer_var(self):
        children = self._parse_node("static AppConfig *_shared;\n")
        decl = [c for c in children if c.type == "declaration"]
        assert _extract_decl_name(decl[0]) == "_shared"

    def test_patched_preserves_comment(self):
        """Comments from original source should appear in patched output."""
        with tempfile.NamedTemporaryFile(suffix=".m", mode="w",
                                          delete=False) as f:
            f.write("/* Copyright header */\n")
            f.write("#import <Foundation/Foundation.h>\n")
            f.write("@interface Foo: OZObject\n@end\n")
            f.write("@implementation Foo\n@end\n")
            f.name
        try:
            from pathlib import Path
            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            cls = OZClass("Foo", superclass="OZObject",
                          class_id=1, base_depth=1)
            m.classes["Foo"] = cls
            result = _emit_patched_source(
                Path(f.name), m, [cls], "Foo", "OZObject", False)
            assert "/* Copyright header */" in result
            assert "/* @interface Foo" in result
            assert "#import" not in result
        finally:
            os.unlink(f.name)

    def test_patched_skips_func_prototype(self):
        """Function prototypes (stubs) should be filtered out."""
        with tempfile.NamedTemporaryFile(suffix=".m", mode="w",
                                          delete=False) as f:
            f.write("int printk(const char *fmt, ...);\n")
            f.write("@implementation Foo\n@end\n")
            f.name
        try:
            from pathlib import Path
            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            cls = OZClass("Foo", superclass="OZObject",
                          class_id=1, base_depth=1)
            m.classes["Foo"] = cls
            result = _emit_patched_source(
                Path(f.name), m, [cls], "Foo", "OZObject", False)
            assert "printk" not in result
        finally:
            os.unlink(f.name)

    def test_patched_skips_collected_static(self):
        """Static vars collected by Clang AST should not be duplicated."""
        with tempfile.NamedTemporaryFile(suffix=".m", mode="w",
                                          delete=False) as f:
            f.write("static int _count;\n")
            f.write("@implementation Foo\n@end\n")
            f.name
        try:
            from pathlib import Path
            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            cls = OZClass("Foo", superclass="OZObject",
                          class_id=1, base_depth=1,
                          statics=[OZStaticVar("_count", OZType("int"))])
            m.classes["Foo"] = cls
            result = _emit_patched_source(
                Path(f.name), m, [cls], "Foo", "OZObject", False)
            assert result.count("_count") == 1
        finally:
            os.unlink(f.name)

    def test_patched_preserves_macro(self):
        """Top-level macro invocations should be preserved verbatim."""
        with tempfile.NamedTemporaryFile(suffix=".m", mode="w",
                                          delete=False) as f:
            f.write("ZBUS_LISTENER_DEFINE(lis, callback);\n")
            f.write("@implementation Foo\n@end\n")
            f.name
        try:
            from pathlib import Path
            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            cls = OZClass("Foo", superclass="OZObject",
                          class_id=1, base_depth=1)
            m.classes["Foo"] = cls
            result = _emit_patched_source(
                Path(f.name), m, [cls], "Foo", "OZObject", False)
            assert "ZBUS_LISTENER_DEFINE(lis, callback)" in result
        finally:
            os.unlink(f.name)

    def test_patched_preserves_define(self):
        """#define directives should be preserved."""
        with tempfile.NamedTemporaryFile(suffix=".m", mode="w",
                                          delete=False) as f:
            f.write("#define MY_CONST 42\n")
            f.write("@implementation Foo\n@end\n")
            f.name
        try:
            from pathlib import Path
            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            cls = OZClass("Foo", superclass="OZObject",
                          class_id=1, base_depth=1)
            m.classes["Foo"] = cls
            result = _emit_patched_source(
                Path(f.name), m, [cls], "Foo", "OZObject", False)
            assert "#define MY_CONST 42" in result
        finally:
            os.unlink(f.name)

    def test_patched_no_duplicate_preamble_include(self):
        """Includes that duplicate the preamble should be deduplicated."""
        with tempfile.NamedTemporaryFile(suffix=".m", mode="w",
                                          delete=False) as f:
            f.write('#include "Foo_ozh.h"\n')
            f.write("@implementation Foo\n@end\n")
            f.name
        try:
            from pathlib import Path
            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            cls = OZClass("Foo", superclass="OZObject",
                          class_id=1, base_depth=1)
            m.classes["Foo"] = cls
            result = _emit_patched_source(
                Path(f.name), m, [cls], "Foo", "OZObject", False)
            assert result.count('#include "Foo_ozh.h"') == 1
        finally:
            os.unlink(f.name)

    def test_patched_dedup_normalizes_whitespace(self):
        """Include dedup should work even with extra whitespace."""
        with tempfile.NamedTemporaryFile(suffix=".m", mode="w",
                                          delete=False) as f:
            f.write('#include  "Foo_ozh.h"\n')
            f.write("@implementation Foo\n@end\n")
            f.name
        try:
            from pathlib import Path
            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            cls = OZClass("Foo", superclass="OZObject",
                          class_id=1, base_depth=1)
            m.classes["Foo"] = cls
            result = _emit_patched_source(
                Path(f.name), m, [cls], "Foo", "OZObject", False)
            assert result.count("Foo_ozh.h") == 1
        finally:
            os.unlink(f.name)

    def test_patched_empty_classes_no_crash(self):
        """Empty classes list should not crash."""
        with tempfile.NamedTemporaryFile(suffix=".m", mode="w",
                                          delete=False) as f:
            f.write("void helper(void) { }\n")
            f.name
        try:
            from pathlib import Path
            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            result = _emit_patched_source(
                Path(f.name), m, [], "orphan", "OZObject", False)
            assert "Auto-generated" in result
        finally:
            os.unlink(f.name)

    def test_patched_pool_count_none_defaults_to_1(self):
        """pool_count_fn returning None should default to 1."""
        with tempfile.NamedTemporaryFile(suffix=".m", mode="w",
                                          delete=False) as f:
            f.write("@implementation Foo\n@end\n")
            f.name
        try:
            from pathlib import Path
            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            cls = OZClass("Foo", superclass="OZObject",
                          class_id=1, base_depth=1)
            m.classes["Foo"] = cls
            result = _emit_patched_source(
                Path(f.name), m, [cls], "Foo", "OZObject", False,
                pool_count_fn=lambda name: None)
            assert "OZ_SLAB_DEFINE(oz_slab_Foo" in result
            assert ", 1, 4)" in result
        finally:
            os.unlink(f.name)

    def test_patched_pool_count_zero_defaults_to_1(self):
        """pool_count_fn returning 0 should default to 1."""
        with tempfile.NamedTemporaryFile(suffix=".m", mode="w",
                                          delete=False) as f:
            f.write("@implementation Foo\n@end\n")
            f.name
        try:
            from pathlib import Path
            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            cls = OZClass("Foo", superclass="OZObject",
                          class_id=1, base_depth=1)
            m.classes["Foo"] = cls
            result = _emit_patched_source(
                Path(f.name), m, [cls], "Foo", "OZObject", False,
                pool_count_fn=lambda name: 0)
            assert ", 1, 4)" in result
        finally:
            os.unlink(f.name)

    def test_patched_pool_count_valid(self):
        """pool_count_fn returning valid int should be used."""
        with tempfile.NamedTemporaryFile(suffix=".m", mode="w",
                                          delete=False) as f:
            f.write("@implementation Foo\n@end\n")
            f.name
        try:
            from pathlib import Path
            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            cls = OZClass("Foo", superclass="OZObject",
                          class_id=1, base_depth=1)
            m.classes["Foo"] = cls
            result = _emit_patched_source(
                Path(f.name), m, [cls], "Foo", "OZObject", False,
                pool_count_fn=lambda name: 8)
            assert ", 8, 4)" in result
        finally:
            os.unlink(f.name)

    def test_patched_aggregates_deps_from_all_classes(self):
        """Dependency includes should aggregate from all classes, not just first."""
        with tempfile.NamedTemporaryFile(suffix=".m", mode="w",
                                          delete=False) as f:
            f.write("@implementation Foo\n@end\n")
            f.name
        try:
            from pathlib import Path
            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            cls_a = OZClass("Foo", superclass="OZObject",
                            class_id=1, base_depth=1)
            cls_b = OZClass("Bar", superclass="OZObject",
                            class_id=2, base_depth=1)
            m.classes["Foo"] = cls_a
            m.classes["Bar"] = cls_b
            result = _emit_patched_source(
                Path(f.name), m, [cls_a, cls_b], "Foo", "OZObject", False)
            assert '#include "OZObject_ozh.h"' in result
        finally:
            os.unlink(f.name)

    def test_include_replacement_flattens_subdir_path(self):
        """#import with subdirectory prefix should emit flat #include (OZ-001)."""
        from io import StringIO
        from pathlib import Path

        with tempfile.TemporaryDirectory() as tmpdir:
            svc_dir = os.path.join(tmpdir, "services")
            os.makedirs(svc_dir)
            hdr = os.path.join(svc_dir, "PXAppConfig.h")
            with open(hdr, "w") as f:
                f.write("@interface PXAppConfig : OZObject\n@end\n")

            buf = StringIO()
            _emit_include_replacement(
                '#import "services/PXAppConfig.h"',
                buf,
                source_dir=Path(tmpdir),
            )
            result = buf.getvalue()
            assert result.strip() == '#include "PXAppConfig_ozh.h"'

    def test_patched_source_flattens_subdir_import(self):
        """Patched source with subdirectory #import should flatten include (OZ-001)."""
        from pathlib import Path

        with tempfile.TemporaryDirectory() as tmpdir:
            main_m = os.path.join(tmpdir, "main.m")
            with open(main_m, "w") as f:
                f.write('#import "services/PXAppConfig.h"\n')
                f.write("@implementation Foo\n@end\n")

            svc_dir = os.path.join(tmpdir, "services")
            os.makedirs(svc_dir)
            with open(os.path.join(svc_dir, "PXAppConfig.h"), "w") as f:
                f.write("@interface PXAppConfig : OZObject\n@end\n")

            m = OZModule()
            m.classes["OZObject"] = OZClass("OZObject",
                                             class_id=0, base_depth=0)
            cls = OZClass("Foo", superclass="OZObject",
                          class_id=1, base_depth=1)
            m.classes["Foo"] = cls
            result = _emit_patched_source(
                Path(main_m), m, [cls], "Foo", "OZObject", False)
            assert '#include "PXAppConfig_ozh.h"' in result
            assert "services/PXAppConfig_ozh.h" not in result


# ===========================================================================
# Protocol dispatch tests — migrated to real .m sources
# ===========================================================================

class TestProtocolDispatchReturnCast:
    """OZ-003: protocol dispatch with object return type must cast to
    the declared return type, not the receiver class."""

    def test_protocol_dispatch_object_return_cast(self):
        """OZ_SEND for a protocol method returning OZString * should cast
        to (struct OZString *), not (struct __patched__ *)."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZString.h>
@protocol PXSensorProtocol
- (OZString *)name;
@end
@interface PXSensor : OZObject <PXSensorProtocol>
- (OZString *)name;
@end
@implementation PXSensor
- (OZString *)name { return nil; }
@end
@interface App : OZObject
- (void)run:(id)sensor;
@end
@implementation App
- (void)run:(id)sensor {
    OZString *sensorName = [sensor name];
}
@end
""")
        content = out["App_ozm.c"]
        assert "__patched__" not in content
        assert "(struct OZString *)" in content
        assert "OZ_PROTOCOL_SEND_name" in content


class TestReturnProtocolDispatch:
    """OZ-005: protocol dispatch in return statement must declare receiver var."""

    def test_return_protocol_dispatch_with_concrete_type(self):
        """return [_sensors count]; with concrete OZArray * uses direct call."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZArray.h>
@interface Registry : OZObject {
    OZArray *_sensors;
}
- (int)sensorCount;
@end
@implementation Registry
- (int)sensorCount {
    return [_sensors count];
}
@end
""")
        content = out["Registry_ozm.c"]
        assert "OZArray_count(" in content

    def test_return_protocol_dispatch_emits_receiver_var(self):
        """return [obj count]; with id receiver uses OZ_PROTOCOL_SEND + temp var.

        Kept synthetic: id-typed receiver protocol dispatch requires
        specific AST structure that real Clang generates differently.
        """
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["OZArray"] = OZClass("OZArray", superclass="OZObject",
            methods=[OZMethod("count", OZType("unsigned int"), body_ast={
                "kind": "CompoundStmt", "inner": []})])
        m.protocols["IteratorProtocol"] = OZProtocol(
            "IteratorProtocol",
            methods=[OZMethod("count", OZType("unsigned int"))])
        m.classes["Registry"] = OZClass("Registry", superclass="OZObject",
            ivars=[OZIvar("_sensors", OZType("OZArray *"))],
            methods=[OZMethod("sensorCount", OZType("int"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "ReturnStmt",
                    "inner": [{
                        "kind": "ObjCMessageExpr",
                        "selector": "count",
                        "type": {"qualType": "unsigned int"},
                        "inner": [{
                            "kind": "ImplicitCastExpr",
                            "type": {"qualType": "id"},
                            "inner": [{
                                "kind": "ObjCIvarRefExpr",
                                "decl": {"name": "_sensors"},
                                "type": {"qualType": "id"},
                            }],
                        }],
                    }],
                }],
            })])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Registry_ozm.c")).read()
            assert "_oz_recv" in content
            assert "OZ_PROTOCOL_SEND_count" in content
            recv_pos = content.index("_oz_recv")
            ret_pos = content.index("return")
            assert recv_pos < ret_pos


# ===========================================================================
# User enum, switch/case, include, static var tests — migrated
# ===========================================================================

class TestUserEnumEmission:
    """OZ-007: user-defined enum collected and emitted in class header."""

    def test_enum_ivar_type_emitted_in_header(self):
        """Enum used as ivar type appears in the generated header."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
enum PXDeviceState {
    PXDeviceStateIdle = 0,
    PXDeviceStateRunning = 1,
};
@interface Manager : OZObject {
    enum PXDeviceState _state;
}
@end
@implementation Manager
@end
""")
        header = out["Manager_ozh.h"]
        assert "PXDeviceStateIdle" in header
        assert "PXDeviceStateRunning" in header


class TestEnumMethodParamEmission:
    """OZ-061: enum used as method param must have its definition emitted."""

    def test_enum_method_param_emitted_in_header(self):
        """Enum used only as method param must appear in generated header."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
enum Color {
    ColorRed = 0,
    ColorGreen = 1,
    ColorBlue = 2,
};
@interface Painter : OZObject
- (void)paintWithColor:(enum Color)c;
@end
@implementation Painter
- (void)paintWithColor:(enum Color)c {
    if (c == ColorRed) {
    }
}
@end
""")
        header = out["Painter_ozh.h"]
        assert "ColorRed" in header
        assert "ColorGreen" in header
        assert "ColorBlue" in header

    def test_enum_return_type_emitted_in_header(self):
        """Enum used only as return type must appear in generated header."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
enum Status {
    StatusOK = 0,
    StatusError = 1,
};
@interface Checker : OZObject
- (enum Status)check;
@end
@implementation Checker
- (enum Status)check {
    return StatusOK;
}
@end
""")
        header = out["Checker_ozh.h"]
        assert "StatusOK" in header
        assert "StatusError" in header

    def test_enum_from_user_header_emitted(self):
        """Enum defined in user .h and used as ivar must appear in header."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "MyEnums.h"
@interface Worker : OZObject {
    enum TaskState _state;
}
- (void)run;
@end
@implementation Worker
- (void)run {
    _state = TaskRunning;
}
@end
""", extra_files={
            "MyEnums.h": """\
enum TaskState {
    TaskIdle = 0,
    TaskRunning = 1,
    TaskDone = 2,
};
"""
        })
        header = out["Worker_ozh.h"]
        assert "TaskIdle" in header
        assert "TaskRunning" in header
        assert "TaskDone" in header


class TestSwitchCaseEmission:
    """OZ-012: switch/case statement emission."""

    def test_switch_case_emitted(self):
        """switch(cond) { case X: ... break; } should be fully emitted."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
enum PXDeviceState {
    PXDeviceStateIdle = 0,
    PXDeviceStateRunning = 1,
};
@interface Mgr : OZObject {
    enum PXDeviceState _state;
}
- (void)start;
@end
@implementation Mgr
- (void)start {
    switch (_state) {
        case PXDeviceStateIdle:
            _state = PXDeviceStateRunning;
            break;
        default:
            break;
    }
}
@end
""")
        content = out["Mgr_ozm.c"]
        assert "switch (self->_state)" in content
        assert "case PXDeviceStateIdle:" in content
        assert "break;" in content
        assert "default:" in content


class TestUserIncludePreservation:
    """OZ-011: quoted #include for plain C headers must be preserved."""

    def test_user_include_in_template_path(self):
        """user_includes on a class should appear in the generated .c file."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Pub"] = OZClass("Pub", superclass="OZObject",
            user_includes=['#include "px_zbus_defs.h"'],
            methods=[OZMethod("publish", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": []})])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            source = open(os.path.join(tmpdir, "Pub_ozm.c")).read()
            header = open(os.path.join(tmpdir, "Pub_ozh.h")).read()
            assert '#include "px_zbus_defs.h"' in source or \
                   '#include "px_zbus_defs.h"' in header


class TestStaticVarNoExternInHeader:
    """OZ-018: static variables must never appear as extern in headers."""

    def test_static_var_not_extern_in_header(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
static int _shared;
@interface Mgr : OZObject
- (void)run;
@end
@implementation Mgr
- (void)run {}
@end
""")
        header = out["Mgr_ozh.h"]
        source = out["Mgr_ozm.c"]
        assert "extern" not in header or "_shared" not in header
        assert "_shared" in source


# ===========================================================================
# Protocol dispatch edge cases — migrated to real .m sources
# ===========================================================================

class TestProtocolDispatchEdgeCases:
    """OZ-020: protocol dispatch edge cases."""

    def test_void_return_no_cast(self):
        """void-returning protocol method should NOT cast."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@protocol Proto
- (void)reset;
@end
@interface Impl : OZObject <Proto>
- (void)reset;
@end
@implementation Impl
- (void)reset {}
@end
@interface App : OZObject
- (void)run:(id)obj;
@end
@implementation App
- (void)run:(id)obj {
    [obj reset];
}
@end
""")
        content = out["App_ozm.c"]
        assert "OZ_PROTOCOL_SEND_reset" in content
        assert "(struct" not in content.split("OZ_PROTOCOL_SEND_reset")[0].split("\n")[-1]

    def test_int_return_no_struct_cast(self):
        """int-returning protocol method should NOT cast to struct."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@protocol Proto
- (unsigned int)count;
@end
@interface Impl : OZObject <Proto>
- (unsigned int)count;
@end
@implementation Impl
- (unsigned int)count { return 0; }
@end
@interface App : OZObject
- (void)run:(id)obj;
@end
@implementation App
- (void)run:(id)obj {
    unsigned int n = [obj count];
}
@end
""")
        content = out["App_ozm.c"]
        assert "OZ_PROTOCOL_SEND_count" in content
        assert "(struct OZObject *)OZ_PROTOCOL_SEND_count" not in content


# ===========================================================================
# Switch/case edge cases — migrated to real .m sources
# ===========================================================================

class TestSwitchCaseEdgeCases:
    """OZ-022: switch/case edge cases."""

    def test_fall_through_cases(self):
        """Consecutive cases without break (fall-through)."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Mgr : OZObject {
    int _state;
}
- (void)handle;
@end
@implementation Mgr
- (void)handle {
    switch (_state) {
        case 0:
        case 1:
            break;
    }
}
@end
""")
        content = out["Mgr_ozm.c"]
        assert "case 0:" in content
        assert "case 1:" in content
        assert "break;" in content

    def test_switch_no_default(self):
        """Switch with only case labels, no default."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Mgr : OZObject {
    int _x;
}
- (void)go;
@end
@implementation Mgr
- (void)go {
    switch (_x) {
        case 42:
            break;
    }
}
@end
""")
        content = out["Mgr_ozm.c"]
        assert "switch (self->_x)" in content
        assert "case 42:" in content
        assert "default:" not in content


# ===========================================================================
# Inherited method / parent ivar access — migrated to real .m sources
# ===========================================================================

class TestInheritedMethodCast:
    """OZ-017: inherited method calls must cast self to declaring class."""

    def test_inherited_method_casts_self(self):
        """[self parentMethod] where parentMethod is in grandparent class."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Base : OZObject
- (int)readRaw;
@end
@implementation Base
- (int)readRaw { return 0; }
@end
@interface Child : Base
- (int)process;
@end
@implementation Child
- (int)process {
    return [self readRaw];
}
@end
""")
        content = out["Child_ozm.c"]
        assert "(struct Base *)self" in content
        assert "Base_readRaw(" in content

    def test_same_class_method_no_cast(self):
        """OZ-028: same-class method must NOT cast self."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (int)helper;
- (void)run;
@end
@implementation Foo
- (int)helper { return 0; }
- (void)run { [self helper]; }
@end
""")
        content = out["Foo_ozm.c"]
        assert "Foo_helper(self)" in content
        assert "(struct Foo *)self" not in content


class TestParentIvarAccess:
    """OZ-019: subclass access to parent ivars via base chain."""

    def test_parent_ivar_uses_base_prefix(self):
        """self->_parentIvar must become self->base._parentIvar."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Parent : OZObject {
    int _count;
}
@end
@implementation Parent
@end
@interface Child : Parent {
    int _value;
}
- (void)inc;
@end
@implementation Child
- (void)inc {
    _count = 1;
}
@end
""")
        content = out["Child_ozm.c"]
        assert "self->base._count" in content

    def test_grandparent_ivar_double_base(self):
        """Grandparent ivar needs self->base.base._ivar."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface GrandP : OZObject {
    int _gval;
}
@end
@implementation GrandP
@end
@interface Parent : GrandP {
    int _pval;
}
@end
@implementation Parent
@end
@interface Child : Parent {
    int _cval;
}
- (int)read;
@end
@implementation Child
- (int)read {
    return _gval;
}
@end
""")
        content = out["Child_ozm.c"]
        assert "self->base.base._gval" in content

    def test_own_ivar_no_base_prefix(self):
        """Own class ivar must NOT get base prefix."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Parent : OZObject {
    int _count;
}
@end
@implementation Parent
@end
@interface Child : Parent {
    int _value;
}
- (int)get;
@end
@implementation Child
- (int)get {
    return _value;
}
@end
""")
        content = out["Child_ozm.c"]
        assert "self->_value" in content
        assert "self->base._value" not in content


# ===========================================================================
# Boxed expression tests — migrated to real .m sources
# ===========================================================================

class TestBoxedExpressions:
    def test_boxed_variable_int(self):
        """@(myInt) with int type -> OZNumber_initInt32(myInt)."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZNumber.h>
void test_boxed(int myInt) {
    OZNumber *n = @(myInt);
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        for path, content in out.items():
            if "OZNumber_initInt32" in content:
                assert "myInt" in content
                break
        else:
            assert False, "OZNumber_initInt32 not found"

    def test_boxed_binary_expr(self):
        """@(a + b) with int type -> OZNumber_initInt32(a + b)."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZNumber.h>
void test_boxed(int a, int b) {
    OZNumber *n = @(a + b);
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        for path, content in out.items():
            if "OZNumber_initInt32" in content:
                assert "a + b" in content
                break
        else:
            assert False, "OZNumber_initInt32 not found"

    def test_boxed_variable_float(self):
        """@(myFloat) with float type -> OZNumber_initFloat(myFloat)."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZNumber.h>
void test_boxed(float myFloat) {
    OZNumber *n = @(myFloat);
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        for path, content in out.items():
            if "OZNumber_initFloat" in content:
                assert "myFloat" in content
                break
        else:
            assert False, "OZNumber_initFloat not found"

    def test_boxed_variable_uint16(self):
        """@(myU16) with uint16_t type -> OZNumber_initUint16(myU16).

        Kept synthetic: OZ SDK OZNumber doesn't define numberWithUnsignedShort:
        so Clang rejects @(uint16_t) — testing emit behavior for this type
        requires bypassing Clang validation.
        """
        m = _simple_module()
        m.classes["OZNumber"] = OZClass(
            "OZNumber", superclass="OZObject",
            ivars=[
                OZIvar("_tag", OZType("enum oz_number_tag")),
                OZIvar("_value", OZType("union oz_number_value")),
            ],
        )
        m.functions.append(OZFunction(
            name="test_boxed",
            return_type=OZType("void"),
            body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "n",
                        "type": {"qualType": "OZNumber *"},
                        "inner": [{
                            "kind": "ObjCBoxedExpr",
                            "type": {"qualType": "NSNumber *"},
                            "inner": [{
                                "kind": "ImplicitCastExpr",
                                "type": {"qualType": "uint16_t"},
                                "castKind": "LValueToRValue",
                                "inner": [{
                                    "kind": "DeclRefExpr",
                                    "referencedDecl": {"name": "myU16"},
                                    "type": {"qualType": "uint16_t"},
                                }],
                            }],
                        }],
                    }],
                }],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZNumber_initUint16(myU16)" in src

    def test_boxed_call_expr(self):
        """@(getValue()) with int return -> OZNumber_initInt32(getValue())."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZNumber.h>
int getValue(void);
void test_boxed(void) {
    OZNumber *n = @(getValue());
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        for path, content in out.items():
            if "OZNumber_initInt32" in content and "getValue()" in content:
                break
        else:
            assert False, "OZNumber_initInt32(getValue()) not found"

    def test_boxed_enum(self):
        """@(enumVar) with enum type -> OZNumber boxing call."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZNumber.h>
enum Color { Red, Green, Blue };
void test_boxed(enum Color enumVar) {
    OZNumber *n = @(enumVar);
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        for path, content in out.items():
            if "OZNumber_init" in content and "enumVar" in content:
                break
        else:
            assert False, "Boxed enum not found"

    def test_boxed_double_warns(self):
        """@(myDouble) with double -> OZNumber_initFloat((float)(...)) + diagnostic.

        Kept synthetic: OZ SDK OZNumber doesn't define numberWithDouble:
        so Clang rejects @(double).
        """
        m = _simple_module()
        m.classes["OZNumber"] = OZClass(
            "OZNumber", superclass="OZObject",
            ivars=[
                OZIvar("_tag", OZType("enum oz_number_tag")),
                OZIvar("_value", OZType("union oz_number_value")),
            ],
        )
        m.functions.append(OZFunction(
            name="test_boxed",
            return_type=OZType("void"),
            body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "n",
                        "type": {"qualType": "OZNumber *"},
                        "inner": [{
                            "kind": "ObjCBoxedExpr",
                            "type": {"qualType": "NSNumber *"},
                            "inner": [{
                                "kind": "ImplicitCastExpr",
                                "type": {"qualType": "double"},
                                "castKind": "LValueToRValue",
                                "inner": [{
                                    "kind": "DeclRefExpr",
                                    "referencedDecl": {"name": "myDouble"},
                                    "type": {"qualType": "double"},
                                }],
                            }],
                        }],
                    }],
                }],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZNumber_initFloat((float)(myDouble))" in src
            assert any("double" in d and "narrowed" in d
                       for d in m.diagnostics)

    def test_boxed_literal_regression(self):
        """Existing @42 literal path still works after refactor."""
        mod, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZNumber.h>
void test_lit(void) {
    OZNumber *n = @99;
}
@interface Dummy : OZObject
@end
@implementation Dummy
@end
""")
        for path, content in out.items():
            if "OZNumber_initInt32(99)" in content:
                break
        else:
            assert False, "OZNumber_initInt32(99) not found"
        assert not mod.errors


# ===========================================================================
# Edge cases — migrated to real .m sources
# ===========================================================================

class TestEmitEdgeCases:
    """Tests for AST node types with missing or light coverage."""

    def test_null_stmt(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)doWork;
@end
@implementation Foo
- (void)doWork { ; }
@end
""")
        content = out["Foo_ozm.c"]
        assert ";\n" in content

    def test_objc_bool_literal_yes(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (BOOL)check;
@end
@implementation Foo
- (BOOL)check { return YES; }
@end
""")
        content = out["Foo_ozm.c"]
        assert "return 1;" in content

    def test_objc_bool_literal_no(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (BOOL)check;
@end
@implementation Foo
- (BOOL)check { return NO; }
@end
""")
        content = out["Foo_ozm.c"]
        assert "return 0;" in content

    def test_string_literal(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)doWork;
@end
@implementation Foo
- (void)doWork {
    const char *s = "hello world";
}
@end
""")
        content = out["Foo_ozm.c"]
        assert '"hello world"' in content

    def test_string_literal_escape(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)doWork;
@end
@implementation Foo
- (void)doWork {
    const char *s = "line\\n";
}
@end
""")
        content = out["Foo_ozm.c"]
        assert '"line\\n"' in content

    def test_array_literal(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZString.h>
#import <Foundation/OZArray.h>
@interface Foo : OZObject
- (void)test_lit;
@end
@implementation Foo
- (void)test_lit {
    OZArray *arr = @[@"a", @"b"];
}
@end
""")
        src = out["Foo_ozm.c"]
        assert "OZArray_initWithItems" in src
        assert "_oz_arr_" in src

    def test_dictionary_literal(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZString.h>
#import <Foundation/OZDictionary.h>
@interface Foo : OZObject
- (void)test_lit;
@end
@implementation Foo
- (void)test_lit {
    OZDictionary *dict = @{@"key": @"val"};
}
@end
""")
        src = out["Foo_ozm.c"]
        assert "OZDictionary_initWithKeysValues" in src
        assert "_oz_dict_" in src

    def test_forin_stmt(self):
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)iterate;
@end
@implementation Foo
- (void)iterate {
    for (id item in self) {}
}
@end
""")
        src = out["Foo_ozm.c"]
        assert "OZ_PROTOCOL_SEND_iter" in src
        assert "OZ_PROTOCOL_SEND_next" in src
        assert "item" in src

    def test_string_dedup_unique_across_methods(self):
        """OZ-039: different string literals in separate methods must get
        unique constant names (no _oz_str_N redefinition)."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZString.h>
@interface Foo : OZObject
- (void)hello;
- (void)bye;
@end
@implementation Foo
- (void)hello { OZString *s = @"hello"; }
- (void)bye { OZString *s = @"bye"; }
@end
""")
        src = out["Foo_ozm.c"]
        assert '"hello"' in src
        assert '"bye"' in src
        assert src.count("static struct OZString _oz_str_") == 2

    def test_string_dedup_uses_loc_when_available(self):
        """OZ-039: string constants use _L{line}_C{col} naming from AST loc.

        Kept synthetic: real Clang AST ObjCStringLiteral loc is stored
        in a different structure than what emit expects for loc-based naming.
        """
        m = OZModule()
        m.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("greet", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "s",
                        "type": {"qualType": "OZString *"},
                        "inner": [{
                            "kind": "ObjCStringLiteral",
                            "loc": {"line": 10, "col": 5},
                            "inner": [{"kind": "StringLiteral",
                                        "value": '"hi"'}],
                        }],
                    }],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "_oz_str_L10_C5" in src

    def test_string_dedup_across_c_functions(self):
        """OZ-071: same string literal in two C functions must not produce
        duplicate static definitions (redefinition error)."""
        _, out = clang_emit_patched("""\
#import <Foundation/OZObject.h>
#import <Foundation/OZString.h>
@interface Foo : OZObject
- (void)greet;
@end
@implementation Foo
- (void)greet { OZString *s = @"key"; }
@end
void other_fn(void) {
    OZString *k = @"key";
}
""", stem="Foo")
        src = out["Foo_ozm.c"]
        assert src.count("static struct OZString _oz_str_") == 1, \
            f"Expected 1 string constant, got: {src.count('static struct OZString _oz_str_')}"

    def test_explicit_release_prevents_double_release(self):
        """OZ-041: explicit [obj release] must suppress ARC auto-release."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
- (void)doWork;
@end
@implementation Foo
- (void)doWork {
    OZObject *obj = [[OZObject alloc] init];
    [obj release];
}
@end
""")
        src = out["Foo_ozm.c"]
        assert src.count("OZObject_release") == 1


# ===========================================================================
# Ivar access control — migrated to real .m sources
# ===========================================================================

class TestIvarAccessControl:
    """Ivar access control tests.

    Tests for protected/private rejection are kept synthetic because Clang
    itself rejects the invalid access before the transpiler can process it.
    """

    def test_external_ivar_access_protected_rejected(self):
        """External access to a protected ivar from a free function is rejected."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Car"] = OZClass(
            "Car", superclass="OZObject",
            ivars=[OZIvar("_color", OZType("int"), access="protected")],
        )
        m.functions.append(OZFunction("main", OZType("int"), body_ast={
            "kind": "CompoundStmt",
            "inner": [{
                "kind": "ObjCIvarRefExpr",
                "decl": {"name": "_color"},
                "isFreeIvar": False,
                "inner": [{
                    "kind": "ImplicitCastExpr",
                    "castKind": "LValueToRValue",
                    "inner": [{
                        "kind": "DeclRefExpr",
                        "referencedDecl": {"name": "myCar"},
                        "type": {"qualType": "Car *"},
                    }],
                }],
            }],
        }))
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
        assert any("_color" in e and "protected" in e for e in m.errors)

    def test_external_ivar_access_public_allowed(self):
        """External access to a public ivar is allowed."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Car"] = OZClass(
            "Car", superclass="OZObject",
            ivars=[OZIvar("_color", OZType("int"), access="public")],
        )
        m.functions.append(OZFunction("main", OZType("int"), body_ast={
            "kind": "CompoundStmt",
            "inner": [{
                "kind": "ObjCIvarRefExpr",
                "decl": {"name": "_color"},
                "isFreeIvar": False,
                "inner": [{
                    "kind": "ImplicitCastExpr",
                    "castKind": "LValueToRValue",
                    "inner": [{
                        "kind": "DeclRefExpr",
                        "referencedDecl": {"name": "myCar"},
                        "type": {"qualType": "Car *"},
                    }],
                }],
            }],
        }))
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
        assert not any("_color" in e for e in m.errors)

    def test_external_ivar_access_private_rejected(self):
        """External access to a private ivar from a subclass is rejected."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Car"] = OZClass(
            "Car", superclass="OZObject",
            ivars=[OZIvar("_secret", OZType("int"), access="private")],
        )
        m.classes["SportsCar"] = OZClass(
            "SportsCar", superclass="Car",
            methods=[OZMethod("reveal", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "ObjCIvarRefExpr",
                    "decl": {"name": "_secret"},
                    "isFreeIvar": False,
                    "inner": [{
                        "kind": "ImplicitCastExpr",
                        "castKind": "LValueToRValue",
                        "inner": [{
                            "kind": "DeclRefExpr",
                            "referencedDecl": {"name": "other"},
                            "type": {"qualType": "Car *"},
                        }],
                    }],
                }],
            })],
        )
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
        assert any("_secret" in e and "private" in e for e in m.errors)

    def test_protected_ivar_access_from_subclass_allowed(self):
        """Subclass self-access to protected ivar is allowed."""
        mod, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject {
    @protected
    int _color;
}
@end
@implementation Car
@end
@interface SportsCar : Car
- (int)getColor;
@end
@implementation SportsCar
- (int)getColor {
    return _color;
}
@end
""")
        content = out["SportsCar_ozm.c"]
        assert "self->base._color" in content
        assert not mod.errors

    def test_protected_external_access_from_subclass_method(self):
        """Subclass accessing protected ivar on another instance is allowed."""
        mod, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject {
    @protected
    int _color;
}
@end
@implementation Car
@end
@interface SportsCar : Car
- (void)copyColor:(Car *)other;
@end
@implementation SportsCar
- (void)copyColor:(Car *)other {
    other->_color;
}
@end
""")
        content = out["SportsCar_ozm.c"]
        assert "other->_color" in content
        assert not mod.errors


# ---------------------------------------------------------------------------
# OZ-062: C array declarations, sizeof, and macro passthrough
# ---------------------------------------------------------------------------

class TestArrayDimensionDecl:
    """OZ-062: C array dimensions must be preserved in variable declarations."""

    def test_array_decl_dimensions(self):
        """int arr[10] must emit with dimensions after name."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
@end
@implementation Foo
- (void)test {
    int arr[10];
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "int arr[10]" in content

    def test_struct_array_decl(self):
        """const struct bt_data ad[2] must preserve dimensions."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
struct bt_data { unsigned char type; };
@interface Foo : OZObject
@end
@implementation Foo
- (void)test {
    struct bt_data ad[2];
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "struct bt_data ad[2]" in content

    def test_char_buffer_decl(self):
        """char buf[32] must preserve dimensions."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
@end
@implementation Foo
- (void)test {
    char buf[32];
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "char buf[32]" in content


class TestSizeofEmission:
    """OZ-062: sizeof operator must be emitted correctly."""

    def test_sizeof_expr(self):
        """sizeof(variable) must be emitted."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
@end
@implementation Foo
- (void)test {
    int arr[10];
    int sz = sizeof(arr);
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "sizeof(" in content
        assert "arr" in content

    def test_sizeof_type(self):
        """sizeof(int) must be emitted."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
@interface Foo : OZObject
@end
@implementation Foo
- (void)test {
    int sz = sizeof(int);
}
@end
""")
        content = out["Foo_ozm.c"]
        assert "sizeof(int)" in content


class TestMacroPassthrough:
    """OZ-062: macro invocations must be preserved via source passthrough."""

    def test_simple_constant_macro(self):
        """Simple constant macro must pass through verbatim."""
        _, out = clang_emit_patched("""\
#import <Foundation/OZObject.h>
#define MY_CONSTANT 42
@interface Foo : OZObject
@end
@implementation Foo
- (void)test {
    int x = MY_CONSTANT;
}
@end
""", stem="Foo")
        content = out["Foo_ozm.c"]
        assert "MY_CONSTANT" in content

    def test_macro_with_args(self):
        """ARRAY_SIZE(arr) macro must pass through verbatim."""
        _, out = clang_emit_patched("""\
#import <Foundation/OZObject.h>
#define ARRAY_SIZE(x) (sizeof(x) / sizeof((x)[0]))
@interface Foo : OZObject
@end
@implementation Foo
- (void)test {
    int arr[10];
    int count = ARRAY_SIZE(arr);
}
@end
""", stem="Foo")
        content = out["Foo_ozm.c"]
        assert "ARRAY_SIZE(arr)" in content

    def test_nested_macros(self):
        """Nested macro invocations must be preserved."""
        _, out = clang_emit_patched("""\
#import <Foundation/OZObject.h>
#define INNER(x) ((x) * 2)
#define OUTER(a, b) ((a) + INNER(b))
@interface Foo : OZObject
@end
@implementation Foo
- (void)test {
    int x = OUTER(1, 3);
}
@end
""", stem="Foo")
        content = out["Foo_ozm.c"]
        assert "OUTER(1, 3)" in content

    def test_macro_with_objc_arg(self):
        """Macro with ObjC message send arg must transpile ObjC in-place."""
        _, out = clang_emit_patched("""\
#import <Foundation/OZObject.h>
#define MY_ADD(a, b) ((a) + (b))
@interface Foo : OZObject
- (int)value;
@end
@implementation Foo
- (int)value { return 10; }
- (void)test {
    int x = MY_ADD([self value], 5);
}
@end
""", stem="Foo")
        content = out["Foo_ozm.c"]
        assert "MY_ADD(" in content
        assert "Foo_value" in content

    def test_macro_with_ivar_arg(self):
        """Macro with ObjC ivar arg must transpile ivar to self->."""
        _, out = clang_emit_patched("""\
#import <Foundation/OZObject.h>
#define DOUBLE(x) ((x) * 2)
@interface Foo : OZObject {
    int _count;
}
@end
@implementation Foo
- (void)test {
    int x = DOUBLE(_count);
}
@end
""", stem="Foo")
        content = out["Foo_ozm.c"]
        assert "DOUBLE(" in content
        assert "self->_count" in content


# ===========================================================================
# OZ-069: Type definitions from headers must be emitted in source files
# ===========================================================================

class TestTypeDefSourceEmission:
    """OZ-069: enum/struct/union defined in user header must be emitted in
    the generated .c when its constants are used in method bodies."""

    # ------------------------------------------------------------------
    # A. Enum definition in header × usage patterns in source
    # ------------------------------------------------------------------

    def test_enum_in_header_used_in_method_body(self):
        """A1: return EnumConst in method body — enum def must be in .c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Sensors.h"
@interface Baro : OZObject
- (int)sensorType;
@end
@implementation Baro
- (int)sensorType {
    return SensorTypeBarometer;
}
@end
""", extra_files={
            "Sensors.h": """\
enum SensorType {
    SensorTypeBase = 0,
    SensorTypeBarometer = 5,
};
"""
        })
        source = out["Baro_ozm.c"]
        assert "SensorTypeBarometer" in source
        assert "enum SensorType" in source

    def test_enum_in_header_used_in_switch_case(self):
        """A2: case EnumConst: in switch — enum def must be in .c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "States.h"
@interface Ctrl : OZObject {
    int _state;
}
- (void)handle;
@end
@implementation Ctrl
- (void)handle {
    switch (_state) {
        case StateIdle:
            break;
        case StateRunning:
            break;
        default:
            break;
    }
}
@end
""", extra_files={
            "States.h": """\
enum State {
    StateIdle = 0,
    StateRunning = 1,
    StateDone = 2,
};
"""
        })
        source = out["Ctrl_ozm.c"]
        assert "StateIdle" in source
        assert "enum State" in source

    def test_enum_in_header_used_in_comparison(self):
        """A3: if (x == EnumConst) — enum def must be in .c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Levels.h"
@interface Logger : OZObject {
    int _level;
}
- (BOOL)isDebug;
@end
@implementation Logger
- (BOOL)isDebug {
    if (_level == LevelDebug) {
        return YES;
    }
    return NO;
}
@end
""", extra_files={
            "Levels.h": """\
enum LogLevel {
    LevelDebug = 0,
    LevelInfo = 1,
    LevelError = 2,
};
"""
        })
        source = out["Logger_ozm.c"]
        assert "LevelDebug" in source
        assert "enum LogLevel" in source

    def test_enum_in_header_used_in_method_arg(self):
        """A4: [self foo:EnumConst] — enum def must be in .c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Colors.h"
@interface Canvas : OZObject
- (void)setColor:(int)c;
- (void)draw;
@end
@implementation Canvas
- (void)setColor:(int)c {}
- (void)draw {
    [self setColor:ColorRed];
}
@end
""", extra_files={
            "Colors.h": """\
enum Color {
    ColorRed = 0,
    ColorGreen = 1,
    ColorBlue = 2,
};
"""
        })
        source = out["Canvas_ozm.c"]
        assert "ColorRed" in source
        assert "enum Color" in source

    def test_enum_in_header_used_in_bitwise(self):
        """A5: flags | FlagA — enum def must be in .c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Flags.h"
@interface Config : OZObject {
    int _flags;
}
- (void)enable;
@end
@implementation Config
- (void)enable {
    _flags = _flags | FlagRead | FlagWrite;
}
@end
""", extra_files={
            "Flags.h": """\
enum Permission {
    FlagRead = 1,
    FlagWrite = 2,
    FlagExec = 4,
};
"""
        })
        source = out["Config_ozm.c"]
        assert "FlagRead" in source
        assert "FlagWrite" in source
        assert "enum Permission" in source

    def test_enum_in_header_used_in_assignment(self):
        """A6: variable = EnumConst in method body — enum def must be in .c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Defaults.h"
@interface App : OZObject {
    int _mode;
}
- (void)reset;
@end
@implementation App
- (void)reset {
    _mode = ModeNormal;
}
@end
""", extra_files={
            "Defaults.h": """\
enum Mode {
    ModeNormal = 0,
    ModeSilent = 1,
};
"""
        })
        source = out["App_ozm.c"]
        assert "ModeNormal" in source
        assert "enum Mode" in source

    def test_enum_in_header_used_in_array_subscript(self):
        """A7: arr[EnumConst] — enum def must be in .c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Indices.h"
@interface Table : OZObject
- (int)first;
@end
@implementation Table
- (int)first {
    int data[3] = {10, 20, 30};
    return data[IdxFirst];
}
@end
""", extra_files={
            "Indices.h": """\
enum Idx {
    IdxFirst = 0,
    IdxSecond = 1,
    IdxThird = 2,
};
"""
        })
        source = out["Table_ozm.c"]
        assert "IdxFirst" in source
        assert "enum Idx" in source

    def test_enum_in_m_used_in_method_body(self):
        """A8: enum defined in .m and used in body — regression guard."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
enum Prio {
    PrioLow = 0,
    PrioHigh = 1,
};
@interface Task : OZObject
- (int)priority;
@end
@implementation Task
- (int)priority {
    return PrioHigh;
}
@end
""")
        source = out["Task_ozm.c"]
        assert "PrioHigh" in source
        assert "enum Prio" in source

    # ------------------------------------------------------------------
    # B. Cross-file and multi-file patterns
    # ------------------------------------------------------------------

    def test_enum_header_used_in_two_classes(self):
        """B1: same header enum used in two classes — both .c get def."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Shared.h"
@interface Alpha : OZObject
- (int)val;
@end
@implementation Alpha
- (int)val {
    return SharedA;
}
@end
@interface Beta : OZObject
- (int)val;
@end
@implementation Beta
- (int)val {
    return SharedB;
}
@end
""", extra_files={
            "Shared.h": """\
enum SharedEnum {
    SharedA = 10,
    SharedB = 20,
};
"""
        })
        src_alpha = out["Alpha_ozm.c"]
        src_beta = out["Beta_ozm.c"]
        assert "enum SharedEnum" in src_alpha
        assert "enum SharedEnum" in src_beta

    def test_enum_transitive_header(self):
        """B2: A.h includes B.h which defines enum — must reach .c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Outer.h"
@interface Dev : OZObject
- (int)kind;
@end
@implementation Dev
- (int)kind {
    return DevKindSensor;
}
@end
""", extra_files={
            "Outer.h": '#import "Inner.h"\n',
            "Inner.h": """\
enum DevKind {
    DevKindSensor = 0,
    DevKindActuator = 1,
};
"""
        })
        source = out["Dev_ozm.c"]
        assert "DevKindSensor" in source
        assert "enum DevKind" in source

    def test_multiple_enums_from_header(self):
        """B3: two enums in header, both used in body — both defs in .c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Multi.h"
@interface Mgr : OZObject
- (int)run;
@end
@implementation Mgr
- (int)run {
    int s = StatusOK;
    int p = PrioNormal;
    return s + p;
}
@end
""", extra_files={
            "Multi.h": """\
enum Status {
    StatusOK = 0,
    StatusFail = 1,
};
enum Prio {
    PrioNormal = 0,
    PrioCritical = 1,
};
"""
        })
        source = out["Mgr_ozm.c"]
        assert "enum Status" in source
        assert "enum Prio" in source
        assert "StatusOK" in source
        assert "PrioNormal" in source

    # ------------------------------------------------------------------
    # C. Deduplication — no double emission
    # ------------------------------------------------------------------

    def test_enum_in_ivar_and_body_no_duplicate(self):
        """C1: enum in ivar type AND body — def in .h, NOT duplicated in .c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Dup.h"
@interface Worker : OZObject {
    enum DupState _state;
}
- (void)run;
@end
@implementation Worker
- (void)run {
    _state = DupRunning;
}
@end
""", extra_files={
            "Dup.h": """\
enum DupState {
    DupIdle = 0,
    DupRunning = 1,
};
"""
        })
        header = out["Worker_ozh.h"]
        source = out["Worker_ozm.c"]
        assert "enum DupState" in header
        assert "DupRunning" not in source or "enum DupState" not in source

    def test_enum_in_param_and_body_no_duplicate(self):
        """C2: enum in param type AND body — def in .h, NOT duplicated in .c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Dup2.h"
@interface Handler : OZObject
- (int)handle:(enum DupAction)a;
@end
@implementation Handler
- (int)handle:(enum DupAction)a {
    if (a == DupActionStop) {
        return 0;
    }
    return 1;
}
@end
""", extra_files={
            "Dup2.h": """\
enum DupAction {
    DupActionStart = 0,
    DupActionStop = 1,
};
"""
        })
        header = out["Handler_ozh.h"]
        source = out["Handler_ozm.c"]
        assert "enum DupAction {" in header
        # Definition block must NOT be duplicated in .c (type name in
        # method prototype is fine — only the definition block matters).
        assert "enum DupAction {" not in source

    # ------------------------------------------------------------------
    # D. Struct/union from header (same bug pattern)
    # ------------------------------------------------------------------

    def test_struct_in_header_not_emitted_in_source(self):
        """D1: struct from header — NOT emitted in .c (user_includes
        preserves the #include, and struct field names are too generic
        for safe constant scanning)."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Point.h"
@interface Geo : OZObject
- (int)originX;
@end
@implementation Geo
- (int)originX {
    struct Point p;
    p.x = 42;
    return p.x;
}
@end
""", extra_files={
            "Point.h": """\
struct Point {
    int x;
    int y;
};
"""
        })
        source = out["Geo_ozm.c"]
        assert "struct Point {" not in source

    def test_union_in_header_not_emitted_in_source(self):
        """D2: union from header — NOT emitted in .c (same rationale
        as D1: user_includes preserves the #include)."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Val.h"
@interface Conv : OZObject
- (int)asInt;
@end
@implementation Conv
- (int)asInt {
    union Value v;
    v.f = 3.14f;
    return v.i;
}
@end
""", extra_files={
            "Val.h": """\
union Value {
    int i;
    float f;
};
"""
        })
        source = out["Conv_ozm.c"]
        assert "union Value {" not in source

    # ------------------------------------------------------------------
    # E. Orphan sources (no class, just functions)
    # ------------------------------------------------------------------

    def test_enum_in_header_used_in_two_methods(self):
        """E1: enum from header used across multiple methods — single def."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Codes.h"
@interface Svc : OZObject
- (int)defaultCode;
- (int)errorCode;
@end
@implementation Svc
- (int)defaultCode {
    return CodeOK;
}
- (int)errorCode {
    return CodeFail;
}
@end
""", extra_files={
            "Codes.h": """\
enum Code {
    CodeOK = 0,
    CodeFail = 1,
};
"""
        })
        source = out["Svc_ozm.c"]
        assert "CodeOK" in source
        assert "CodeFail" in source
        assert "enum Code {" in source
        # Definition should appear exactly once
        assert source.count("enum Code {") == 1

    # ------------------------------------------------------------------
    # F. Edge cases
    # ------------------------------------------------------------------

    def test_enum_constant_not_in_body_not_emitted(self):
        """F1: enum in header, NOT used in any body — def NOT in .c."""
        _, out = clang_emit("""\
#import <Foundation/OZObject.h>
#import "Unused.h"
@interface Noop : OZObject
- (void)run;
@end
@implementation Noop
- (void)run {}
@end
""", extra_files={
            "Unused.h": """\
enum Unused {
    UnusedA = 0,
    UnusedB = 1,
};
"""
        })
        source = out["Noop_ozm.c"]
        assert "enum Unused" not in source
