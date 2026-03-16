# SPDX-License-Identifier: Apache-2.0

import os
import tempfile

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


class TestEmitFiles:
    def test_generates_expected_files(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            files = emit(m, tmpdir)
            basenames = {os.path.basename(f) for f in files}
            assert "oz_dispatch.h" in basenames
            assert "oz_dispatch.c" in basenames
            assert "OZObject_ozh.h" in basenames
            assert "OZObject_ozm.c" in basenames
            assert "OZLed_ozh.h" in basenames
            assert "OZLed_ozm.c" in basenames


class TestDispatchHeader:
    def test_class_id_enum(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            assert "OZ_CLASS_OZObject" in content
            assert "OZ_CLASS_OZLed" in content
            assert "OZ_CLASS_COUNT" in content

    def test_protocol_vtable(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            # init is overridden -> PROTOCOL dispatch
            assert "OZ_vtable_init" in content
            assert "OZ_SEND_init" in content

    def test_class_method_excluded_from_vtable(self):
        m = _simple_module()
        m.classes["OZLed"].methods.append(
            OZMethod("greet", OZType("void"), is_class_method=True,
                     body_ast={"kind": "CompoundStmt", "inner": []}))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            dispatch_h = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            dispatch_c = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.c")).read()
            assert "vtable_greet" not in dispatch_h
            assert "vtable_greet" not in dispatch_c
            # But the class method should appear in the class header
            led_h = open(os.path.join(tmpdir, "OZLed_ozh.h")).read()
            assert "OZLed_cls_greet(void)" in led_h


class TestClassHeader:
    def test_struct_with_base(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "OZLed_ozh.h")).read()
            assert "struct OZLed {" in content
            assert "struct OZObject base;" in content
            assert "int _pin;" in content

    def test_root_struct(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "OZObject_ozh.h")).read()
            assert "oz_class_id" in content
            assert "_refcount" in content


class TestClassSource:
    def test_method_body(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "OZLed_ozm.c")).read()
            assert "OZLed_turnOn" in content
            assert "struct OZLed *self" in content

    def test_root_retain_release(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZObject_retain" in content
            assert "OZObject_release" in content
            assert "oz_atomic_dec_and_test" in content
            assert "OZ_SEND_dealloc" in content


class TestBodyEmission:
    def test_ivar_access(self):
        m = OZModule()
        m.classes["OZLed"] = OZClass("OZLed", ivars=[OZIvar("_pin", OZType("int"))],
                                     methods=[
            OZMethod("pin", OZType("int"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "ReturnStmt",
                    "inner": [{
                        "kind": "ObjCIvarRefExpr",
                        "decl": {"name": "_pin"},
                    }],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "OZLed_ozm.c")).read()
            assert "self->_pin" in content

    def test_super_call(self):
        m = _simple_module()
        # Add a method that calls [super init]
        m.classes["OZLed"].methods.append(
            OZMethod("initWithPin:", OZType("instancetype"),
                     params=[OZParam("pin", OZType("int"))],
                     body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "ObjCMessageExpr",
                    "selector": "init",
                    "receiverKind": "super",
                    "inner": [],
                }],
            })
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "OZLed_ozm.c")).read()
            assert "OZObject_init((struct OZObject *)self)" in content

    def test_message_send_static(self):
        m = OZModule()
        m.classes["OZLed"] = OZClass("OZLed", methods=[
            OZMethod("turnOn", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [],
            }),
            OZMethod("test", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "ObjCMessageExpr",
                    "selector": "turnOn",
                    "receiverKind": "instance",
                    "inner": [{
                        "kind": "DeclRefExpr",
                        "referencedDecl": {"name": "self"},
                        "type": {"qualType": "OZLed *"},
                    }],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "OZLed_ozm.c")).read()
            assert "OZLed_turnOn(self)" in content

    def test_if_stmt(self):
        m = OZModule()
        m.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("check", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "IfStmt",
                    "hasElse": False,
                    "inner": [
                        {"kind": "IntegerLiteral", "value": "1"},
                        {"kind": "CompoundStmt", "inner": []},
                    ],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "if (1)" in content

    def test_binary_operator(self):
        m = OZModule()
        m.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("add", OZType("int"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "ReturnStmt",
                    "inner": [{
                        "kind": "BinaryOperator",
                        "opcode": "+",
                        "inner": [
                            {"kind": "IntegerLiteral", "value": "1"},
                            {"kind": "IntegerLiteral", "value": "2"},
                        ],
                    }],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "1 + 2" in content


def _module_with_obj_ivar():
    """OZObject(root) -> Holder(_child: OZObject*)"""
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
    m.classes["Holder"] = OZClass(
        "Holder", superclass="OZObject",
        ivars=[
            OZIvar("_child", OZType("OZObject *")),
            OZIvar("_count", OZType("int")),
        ],
    )
    resolve(m)
    return m


class TestARCLocalRelease:
    def test_local_object_released_at_scope_end(self):
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
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("doStuff", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "tmp",
                        "type": {"qualType": "OZObject *"},
                        "inner": [{"kind": "IntegerLiteral", "value": "0"}],
                    }],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "OZObject_release((struct OZObject *)tmp);" in content

    def test_primitive_not_released(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("doStuff", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "count",
                        "type": {"qualType": "int"},
                        "inner": [{"kind": "IntegerLiteral", "value": "0"}],
                    }],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "release" not in content

    def test_returned_object_not_released(self):
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
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("create", OZType("OZObject *"), body_ast={
                "kind": "CompoundStmt",
                "inner": [
                    {"kind": "DeclStmt", "inner": [{
                        "kind": "VarDecl",
                        "name": "obj",
                        "type": {"qualType": "OZObject *"},
                        "inner": [{"kind": "IntegerLiteral", "value": "0"}],
                    }]},
                    {"kind": "DeclStmt", "inner": [{
                        "kind": "VarDecl",
                        "name": "other",
                        "type": {"qualType": "OZObject *"},
                        "inner": [{"kind": "IntegerLiteral", "value": "0"}],
                    }]},
                    {"kind": "ReturnStmt", "inner": [{
                        "kind": "DeclRefExpr",
                        "referencedDecl": {"name": "obj"},
                        "type": {"qualType": "OZObject *"},
                    }]},
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            # 'other' should be released, 'obj' (returned) should not
            assert "OZObject_release((struct OZObject *)other);" in content
            assert "OZObject_release((struct OZObject *)obj);" not in content

    def test_self_never_released(self):
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
            # init returns self — self should never be released
            assert "OZObject_release((struct OZObject *)self);" not in content

    def test_nested_scope_inner_released_at_inner_exit(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("doStuff", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [
                    {"kind": "DeclStmt", "inner": [{
                        "kind": "VarDecl", "name": "outer",
                        "type": {"qualType": "OZObject *"},
                        "inner": [{"kind": "IntegerLiteral", "value": "0"}],
                    }]},
                    {"kind": "CompoundStmt", "inner": [
                        {"kind": "DeclStmt", "inner": [{
                            "kind": "VarDecl", "name": "inner",
                            "type": {"qualType": "OZObject *"},
                            "inner": [{"kind": "IntegerLiteral", "value": "0"}],
                        }]},
                    ]},
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            # inner released inside nested scope, outer released at method end
            assert content.count("OZObject_release") == 2


    def test_early_return_inside_if_releases_outer_vars(self):
        """Return inside an if-block should release vars from outer scopes."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("check", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [
                    {"kind": "DeclStmt", "inner": [{
                        "kind": "VarDecl", "name": "a",
                        "type": {"qualType": "OZObject *"},
                        "inner": [{"kind": "IntegerLiteral", "value": "0"}],
                    }]},
                    {"kind": "IfStmt", "hasElse": False, "inner": [
                        {"kind": "IntegerLiteral", "value": "1"},
                        {"kind": "CompoundStmt", "inner": [
                            {"kind": "ReturnStmt", "inner": []},
                        ]},
                    ]},
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            # 'a' should be released both by the early return AND at scope exit
            # (the scope-exit release is for the non-return path)
            assert content.count("OZObject_release((struct OZObject *)a)") == 2

    def test_alloc_count_determines_slab_size(self):
        """Slab pool size should match the number of alloc calls in source."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        # Create a function with 3 alloc calls for Foo
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject")
        alloc_msg = {
            "kind": "ObjCMessageExpr", "selector": "alloc",
            "receiverKind": "class",
            "classType": {"qualType": "Foo"},
            "type": {"qualType": "Foo *"},
            "inner": [],
        }
        m.functions.append(OZFunction(
            "test_fn", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [alloc_msg, alloc_msg, alloc_msg],
            }
        ))
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            foo_c = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "oz_slab_Foo, sizeof(struct Foo), 3, 4)" in foo_c
            # OZObject has 0 alloc calls -> minimum 1
            root_c = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "oz_slab_OZObject, sizeof(struct OZObject), 1, 4)" in root_c


class TestARCAutoDealloc:
    def test_auto_dealloc_with_object_ivar(self):
        m = _module_with_obj_ivar()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder_ozm.c")).read()
            assert "Holder_dealloc" in content
            assert "OZObject_release((struct OZObject *)self->_child)" in content
            assert "OZObject_dealloc((struct OZObject *)self)" in content

    def test_root_dealloc_calls_free(self):
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
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject", methods=[
            OZMethod("dealloc", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [],
            }),
        ])
        m.classes["Child"] = OZClass("Child", superclass="OZObject",
                                     ivars=[OZIvar("_val", OZType("int"))])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Child_ozm.c")).read()
            # Child has no object ivars but should still call parent dealloc
            assert "Child_dealloc" in content
            assert "OZObject_dealloc((struct OZObject *)self)" in content

    def test_user_defined_dealloc_prepends_ivar_releases(self):
        m = _module_with_obj_ivar()
        # Add user dealloc with a custom statement
        m.classes["Holder"].methods.append(
            OZMethod("dealloc", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [
                    {"kind": "IntegerLiteral", "value": "42"},  # placeholder user code
                    {"kind": "ObjCMessageExpr",
                     "selector": "dealloc",
                     "receiverKind": "super",
                     "inner": []},
                ],
            })
        )
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder_ozm.c")).read()
            # Ivar release should be prepended
            assert "OZObject_release((struct OZObject *)self->_child)" in content
            # Parent dealloc appended (super dealloc from user filtered out)
            assert "OZObject_dealloc((struct OZObject *)self)" in content
            # User code should still be present
            assert "42" in content


class TestARCBreakContinueCleanup:
    def _module_with_method(self, body_ast):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast=body_ast),
        ])
        resolve(m)
        return m

    def _while_with_break(self, body_inner, pre_loop=None):
        stmts = []
        if pre_loop:
            stmts.extend(pre_loop)
        stmts.append({
            "kind": "WhileStmt",
            "inner": [
                {"kind": "IntegerLiteral", "value": "1"},
                {"kind": "CompoundStmt", "inner": body_inner},
            ],
        })
        return {"kind": "CompoundStmt", "inner": stmts}

    def _obj_decl(self, name):
        return {"kind": "DeclStmt", "inner": [{
            "kind": "VarDecl", "name": name,
            "type": {"qualType": "OZObject *"},
            "inner": [{"kind": "IntegerLiteral", "value": "0"}],
        }]}

    def test_break_releases_loop_locals(self):
        body = self._while_with_break([
            self._obj_decl("a"),
            {"kind": "BreakStmt"},
        ])
        m = self._module_with_method(body)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            lines = content.split("\n")
            release_line = next(i for i, l in enumerate(lines) if "release" in l and "a)" in l)
            break_line = next(i for i, l in enumerate(lines) if "break;" in l)
            assert release_line < break_line

    def test_continue_releases_loop_locals(self):
        body = self._while_with_break([
            self._obj_decl("a"),
            {"kind": "ContinueStmt"},
        ])
        m = self._module_with_method(body)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            lines = content.split("\n")
            release_line = next(i for i, l in enumerate(lines) if "release" in l and "a)" in l)
            continue_line = next(i for i, l in enumerate(lines) if "continue;" in l)
            assert release_line < continue_line

    def test_break_releases_nested_scopes_in_loop(self):
        body = self._while_with_break([
            self._obj_decl("a"),
            {"kind": "IfStmt", "hasElse": False, "inner": [
                {"kind": "IntegerLiteral", "value": "1"},
                {"kind": "CompoundStmt", "inner": [
                    self._obj_decl("b"),
                    {"kind": "BreakStmt"},
                ]},
            ]},
        ])
        m = self._module_with_method(body)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            break_idx = content.index("break;")
            before_break = content[:break_idx]
            assert "release((struct OZObject *)b)" in before_break
            assert "release((struct OZObject *)a)" in before_break

    def test_break_does_not_release_outside_loop(self):
        body = self._while_with_break(
            [self._obj_decl("inner"), {"kind": "BreakStmt"}],
            pre_loop=[self._obj_decl("outer")],
        )
        m = self._module_with_method(body)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            break_idx = content.index("break;")
            before_break = content[:break_idx]
            assert "release((struct OZObject *)inner)" in before_break
            assert "release((struct OZObject *)outer)" not in before_break

    def test_nested_loops_break_only_releases_inner(self):
        body = {"kind": "CompoundStmt", "inner": [{
            "kind": "WhileStmt", "inner": [
                {"kind": "IntegerLiteral", "value": "1"},
                {"kind": "CompoundStmt", "inner": [
                    self._obj_decl("a"),
                    {"kind": "WhileStmt", "inner": [
                        {"kind": "IntegerLiteral", "value": "1"},
                        {"kind": "CompoundStmt", "inner": [
                            self._obj_decl("b"),
                            {"kind": "BreakStmt"},
                        ]},
                    ]},
                ]},
            ],
        }]}
        m = self._module_with_method(body)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            break_idx = content.index("break;")
            before_break = content[:break_idx]
            assert "release((struct OZObject *)b)" in before_break
            assert "release((struct OZObject *)a)" not in before_break

    def test_consumed_var_not_released_on_break(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Holder"] = OZClass("Holder", superclass="OZObject",
            ivars=[OZIvar("_child", OZType("OZObject *"))],
            methods=[OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "WhileStmt", "inner": [
                        {"kind": "IntegerLiteral", "value": "1"},
                        {"kind": "CompoundStmt", "inner": [
                            self._obj_decl("obj"),
                            {"kind": "BinaryOperator", "opcode": "=", "inner": [
                                {"kind": "ObjCIvarRefExpr", "decl": {"name": "_child"}},
                                {"kind": "DeclRefExpr",
                                 "referencedDecl": {"name": "obj"},
                                 "type": {"qualType": "OZObject *"}},
                            ]},
                            {"kind": "BreakStmt"},
                        ]},
                    ],
                }],
            })],
        )
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder_ozm.c")).read()
            break_idx = content.index("break;")
            before_break = content[:break_idx]
            assert "release((struct OZObject *)obj)" not in before_break

    def test_for_stmt_break_releases(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "ForStmt", "inner": [
                        {"kind": "DeclStmt", "inner": [{
                            "kind": "VarDecl", "name": "i",
                            "type": {"qualType": "int"},
                            "inner": [{"kind": "IntegerLiteral", "value": "0"}],
                        }]},
                        {"kind": "BinaryOperator", "opcode": "<", "inner": [
                            {"kind": "DeclRefExpr", "referencedDecl": {"name": "i"},
                             "type": {"qualType": "int"}},
                            {"kind": "IntegerLiteral", "value": "10"},
                        ]},
                        {"kind": "UnaryOperator", "opcode": "++",
                         "isPostfix": True, "inner": [
                            {"kind": "DeclRefExpr", "referencedDecl": {"name": "i"},
                             "type": {"qualType": "int"}},
                        ]},
                        {"kind": "CompoundStmt", "inner": [
                            self._obj_decl("a"),
                            {"kind": "BreakStmt"},
                        ]},
                    ],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            break_idx = content.index("break;")
            before_break = content[:break_idx]
            assert "release((struct OZObject *)a)" in before_break

    def test_do_while_break_releases(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "DoStmt", "inner": [
                        {"kind": "CompoundStmt", "inner": [
                            self._obj_decl("a"),
                            {"kind": "BreakStmt"},
                        ]},
                        {"kind": "IntegerLiteral", "value": "1"},
                    ],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "do {" in content
            assert "while (1);" in content
            break_idx = content.index("break;")
            before_break = content[:break_idx]
            assert "release((struct OZObject *)a)" in before_break


class TestARCAutoreleasePool:
    def _obj_decl(self, name):
        return {"kind": "DeclStmt", "inner": [{
            "kind": "VarDecl", "name": name,
            "type": {"qualType": "OZObject *"},
            "inner": [{"kind": "IntegerLiteral", "value": "0"}],
        }]}

    def test_autoreleasepool_releases_at_exit(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "ObjCAutoreleasePoolStmt", "inner": [{
                        "kind": "CompoundStmt", "inner": [
                            self._obj_decl("a"),
                        ],
                    }],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "OZObject_release((struct OZObject *)a);" in content

    def test_autoreleasepool_nested(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "ObjCAutoreleasePoolStmt", "inner": [{
                        "kind": "CompoundStmt", "inner": [
                            self._obj_decl("outer"),
                            {"kind": "ObjCAutoreleasePoolStmt", "inner": [{
                                "kind": "CompoundStmt", "inner": [
                                    self._obj_decl("inner"),
                                ],
                            }]},
                        ],
                    }],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert content.count("OZObject_release") == 2

    def test_autoreleasepool_does_not_release_outer_vars(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [
                    self._obj_decl("before"),
                    {"kind": "ObjCAutoreleasePoolStmt", "inner": [{
                        "kind": "CompoundStmt", "inner": [
                            self._obj_decl("inside"),
                        ],
                    }]},
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            # Find the pool block's closing brace area
            inside_release = content.index("release((struct OZObject *)inside)")
            before_release = content.index("release((struct OZObject *)before)")
            # inside released first (inner scope), before released at method end
            assert inside_release < before_release


class TestARCParameterRetain:
    def test_object_param_retained_at_entry(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("setItem:", OZType("void"),
                     params=[OZParam("item", OZType("OZObject *"))],
                     body_ast={"kind": "CompoundStmt", "inner": []}),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "OZObject_retain((struct OZObject *)item)" in content

    def test_object_param_released_at_scope_exit(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("setItem:", OZType("void"),
                     params=[OZParam("item", OZType("OZObject *"))],
                     body_ast={"kind": "CompoundStmt", "inner": []}),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "OZObject_release((struct OZObject *)item)" in content

    def test_primitive_param_not_retained(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("setCount:", OZType("void"),
                     params=[OZParam("count", OZType("int"))],
                     body_ast={"kind": "CompoundStmt", "inner": []}),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "retain" not in content

    def test_multiple_object_params_all_retained(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("set:with:", OZType("void"),
                     params=[
                         OZParam("a", OZType("OZObject *")),
                         OZParam("count", OZType("int")),
                         OZParam("b", OZType("OZObject *")),
                     ],
                     body_ast={"kind": "CompoundStmt", "inner": []}),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "OZObject_retain((struct OZObject *)a)" in content
            assert "OZObject_retain((struct OZObject *)b)" in content
            assert "retain((struct OZObject *)count)" not in content

    def test_object_param_released_on_return(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("process:", OZType("void"),
                     params=[OZParam("item", OZType("OZObject *"))],
                     body_ast={"kind": "CompoundStmt", "inner": [
                         {"kind": "ReturnStmt", "inner": []},
                     ]}),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "OZObject_retain((struct OZObject *)item)" in content
            ret_idx = content.index("return;")
            before_ret = content[:ret_idx]
            assert "OZObject_release((struct OZObject *)item)" in before_ret

    def test_param_assigned_to_ivar_not_consumed(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Holder"] = OZClass("Holder", superclass="OZObject",
            ivars=[OZIvar("_child", OZType("OZObject *"))],
            methods=[OZMethod("setChild:", OZType("void"),
                     params=[OZParam("child", OZType("OZObject *"))],
                     body_ast={"kind": "CompoundStmt", "inner": [{
                         "kind": "BinaryOperator", "opcode": "=", "inner": [
                             {"kind": "ObjCIvarRefExpr", "decl": {"name": "_child"}},
                             {"kind": "DeclRefExpr",
                              "referencedDecl": {"name": "child"},
                              "type": {"qualType": "OZObject *"}},
                         ],
                     }]}),
            ],
        )
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder_ozm.c")).read()
            # param retained at entry
            assert "OZObject_retain((struct OZObject *)child)" in content
            # ivar assign also retains
            assert "self->_child = child;" in content
            # param NOT consumed — should be released at scope exit
            assert "OZObject_release((struct OZObject *)child);" in content


class TestARCStrongIvarAssign:
    def test_ivar_assign_emits_retain_release(self):
        m = _module_with_obj_ivar()
        m.classes["Holder"].methods.append(
            OZMethod("setChild:", OZType("void"),
                     params=[OZParam("child", OZType("OZObject *"))],
                     body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "BinaryOperator",
                    "opcode": "=",
                    "inner": [
                        {"kind": "ObjCIvarRefExpr",
                         "decl": {"name": "_child"}},
                        {"kind": "DeclRefExpr",
                         "referencedDecl": {"name": "child"},
                         "type": {"qualType": "OZObject *"}},
                    ],
                }],
            })
        )
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder_ozm.c")).read()
            assert "OZObject_retain((struct OZObject *)child)" in content
            assert "OZObject_release((struct OZObject *)self->_child)" in content
            assert "self->_child = child;" in content

    def test_consumed_local_not_released_at_scope_exit(self):
        m = _module_with_obj_ivar()
        m.classes["Holder"].methods.append(
            OZMethod("setup", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [
                    {"kind": "DeclStmt", "inner": [{
                        "kind": "VarDecl",
                        "name": "obj",
                        "type": {"qualType": "OZObject *"},
                        "inner": [{"kind": "IntegerLiteral", "value": "0"}],
                    }]},
                    {"kind": "BinaryOperator",
                     "opcode": "=",
                     "inner": [
                         {"kind": "ObjCIvarRefExpr",
                          "decl": {"name": "_child"}},
                         {"kind": "DeclRefExpr",
                          "referencedDecl": {"name": "obj"},
                          "type": {"qualType": "OZObject *"}},
                     ]},
                ],
            })
        )
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder_ozm.c")).read()
            # obj was consumed by ivar assign — should NOT be released at scope exit
            assert "OZObject_release((struct OZObject *)obj);" not in content
            # Consumed local transfers ownership — no extra retain needed
            assert "OZObject_retain((struct OZObject *)obj)" not in content
            # Old ivar value should still be released
            assert "OZObject_release((struct OZObject *)self->_child)" in content


class TestARCLocalReassign:
    """ARC for local object variable reassignment and nil assignment."""

    def _method_with_body(self, body_inner):
        """Create a Holder method 'doWork' with given body statements."""
        m = _module_with_obj_ivar()
        m.classes["Holder"].methods.append(
            OZMethod("doWork", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": body_inner,
            })
        )
        resolve(m)
        return m

    def _alloc_msg(self, cls="OZObject"):
        """AST for [ClassName alloc] — class message send."""
        return {
            "kind": "ObjCMessageExpr",
            "selector": "alloc",
            "receiverKind": "class",
            "classType": {"qualType": cls},
            "type": {"qualType": f"{cls} *"},
            "inner": [],
        }

    def _decl_obj(self, name, init_expr=None):
        """AST for: OZObject *name = init_expr;"""
        decl = {
            "kind": "VarDecl",
            "name": name,
            "type": {"qualType": "OZObject *"},
        }
        if init_expr:
            decl["inner"] = [init_expr]
        else:
            decl["inner"] = [{"kind": "IntegerLiteral", "value": "0"}]
        return {"kind": "DeclStmt", "inner": [decl]}

    def _assign(self, var_name, rhs):
        """AST for: var_name = rhs;"""
        return {
            "kind": "BinaryOperator",
            "opcode": "=",
            "inner": [
                {"kind": "ImplicitCastExpr", "inner": [
                    {"kind": "DeclRefExpr",
                     "referencedDecl": {"name": var_name},
                     "type": {"qualType": "OZObject *"}},
                ]},
                rhs,
            ],
        }

    def _nil_expr(self):
        """AST for nil / (id)0."""
        return {
            "kind": "CStyleCastExpr",
            "castKind": "NullToPointer",
            "type": {"qualType": "OZObject *"},
            "inner": [{"kind": "IntegerLiteral", "value": "0"}],
        }

    def _declref(self, name):
        """AST for a DeclRefExpr."""
        return {
            "kind": "DeclRefExpr",
            "referencedDecl": {"name": name},
            "type": {"qualType": "OZObject *"},
        }

    def test_reassign_releases_old(self):
        m = self._method_with_body([
            self._decl_obj("f", self._alloc_msg()),
            self._assign("f", self._alloc_msg()),
        ])
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder_ozm.c")).read()
            # Release old f before reassignment
            assert "OZObject_release((struct OZObject *)f);" in content
            # No retain for alloc result (already +1)
            # retain only appears in ivar-related code, not for this reassignment
            body = content.split("void Holder_doWork")[1]
            assert "OZObject_retain" not in body

    def test_nil_assign_releases_old(self):
        m = self._method_with_body([
            self._decl_obj("f", self._alloc_msg()),
            self._assign("f", self._nil_expr()),
        ])
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder_ozm.c")).read()
            assert "OZObject_release((struct OZObject *)f);" in content
            assert "f = ((void *)0);" in content

    def test_scope_exit_still_releases_after_reassign(self):
        m = self._method_with_body([
            self._decl_obj("f", self._alloc_msg()),
            self._assign("f", self._alloc_msg()),
        ])
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder_ozm.c")).read()
            # release appears twice: once for reassignment, once at scope exit
            count = content.count("OZObject_release((struct OZObject *)f);")
            assert count == 2

    def test_reassign_after_consume_clears_consumed(self):
        m = self._method_with_body([
            self._decl_obj("obj", self._alloc_msg()),
            # self->_child = obj (ivar assign, consumes obj)
            {"kind": "BinaryOperator",
             "opcode": "=",
             "inner": [
                 {"kind": "ObjCIvarRefExpr",
                  "decl": {"name": "_child"}},
                 self._declref("obj"),
             ]},
            # obj = [[OZObject alloc]] (reassign after consume)
            self._assign("obj", self._alloc_msg()),
        ])
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder_ozm.c")).read()
            # obj should be released at scope exit (no longer consumed)
            # 1 release from reassignment + 1 release from scope exit
            count = content.count("OZObject_release((struct OZObject *)obj);")
            assert count == 2

    def test_local_to_local_retains_new(self):
        m = self._method_with_body([
            self._decl_obj("a", self._alloc_msg()),
            self._decl_obj("b", self._alloc_msg()),
            self._assign("a", self._declref("b")),
        ])
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder_ozm.c")).read()
            # retain(b) before release(a) and assign
            assert "OZObject_retain((struct OZObject *)b);" in content
            assert "OZObject_release((struct OZObject *)a);" in content
            assert "a = b;" in content

    def test_primitive_local_not_intercepted(self):
        m = _module_with_obj_ivar()
        m.classes["Holder"].methods.append(
            OZMethod("doWork", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [
                    {"kind": "DeclStmt", "inner": [{
                        "kind": "VarDecl",
                        "name": "x",
                        "type": {"qualType": "int"},
                        "inner": [{"kind": "IntegerLiteral", "value": "5"}],
                    }]},
                    {"kind": "BinaryOperator",
                     "opcode": "=",
                     "inner": [
                         {"kind": "DeclRefExpr",
                          "referencedDecl": {"name": "x"},
                          "type": {"qualType": "int"}},
                         {"kind": "IntegerLiteral", "value": "10"},
                     ]},
                ],
            })
        )
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder_ozm.c")).read()
            assert "x = 10;" in content
            # No release for primitive types in doWork body
            dowork_body = content.split("void Holder_doWork")[1].split("}")[0]
            assert "release" not in dowork_body


class TestIntrospection:
    """Tests for class name/superclass tables and introspection helpers."""

    def test_dispatch_header_has_class_names_table(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            assert "extern const char *const oz_class_names[OZ_CLASS_COUNT]" in content

    def test_dispatch_header_has_superclass_table(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            assert "extern const uint8_t oz_superclass_id[OZ_CLASS_COUNT]" in content

    def test_dispatch_header_has_inline_helpers(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            assert "oz_name(" in content
            assert "oz_superclass(" in content
            assert "oz_isKindOfClass(" in content

    def test_dispatch_source_class_names(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.c")).read()
            assert 'oz_class_names[OZ_CLASS_COUNT]' in content
            assert '"OZObject"' in content
            assert '"OZLed"' in content

    def test_dispatch_source_superclass_ids(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.c")).read()
            assert "oz_superclass_id[OZ_CLASS_COUNT]" in content
            assert "[OZ_CLASS_OZObject] = OZ_CLASS_COUNT" in content
            assert "[OZ_CLASS_OZLed] = OZ_CLASS_OZObject" in content

    def test_root_class_isEqual(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            h = open(os.path.join(tmpdir, "Foundation", "OZObject_ozh.h")).read()
            c = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZObject_isEqual_" in h
            assert "OZObject_isEqual_" in c
            assert "self == anObject" in c

    def test_root_class_cDescription(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            h = open(os.path.join(tmpdir, "Foundation", "OZObject_ozh.h")).read()
            c = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZObject_cDescription_maxLength_" in h
            assert "OZObject_cDescription_maxLength_" in c
            assert "oz_platform_snprint" in c
            assert "oz_class_names" in c

    def test_isEqual_protocol_dispatched(self):
        m = _simple_module()
        m.classes["OZObject"].methods.append(
            OZMethod("isEqual:", OZType("BOOL"),
                     params=[OZParam("anObject", OZType("OZObject *"))],
                     body_ast={"kind": "CompoundStmt", "inner": []})
        )
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            assert "OZ_SEND_isEqual_" in content
            assert "OZ_vtable_isEqual_" in content

    def test_cDescription_protocol_dispatched(self):
        m = _simple_module()
        m.classes["OZObject"].methods.append(
            OZMethod("cDescription:maxLength:", OZType("int"),
                     params=[OZParam("buf", OZType("char *")),
                             OZParam("maxLen", OZType("int"))],
                     body_ast={"kind": "CompoundStmt", "inner": []})
        )
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            assert "OZ_SEND_cDescription_maxLength_" in content

    def test_root_source_includes_header(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZObject_ozh.h" in content


class TestStaticVarEmission:
    """Tests for file-scope static variable emission in class _ozm.c file."""

    def test_static_var_emitted_in_class_file(self):
        m = _simple_module()
        m.statics.append(OZStaticVar("_sharedConfig", OZType("AppConfig *")))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "struct AppConfig * _sharedConfig;" in content

    def test_primitive_static_var(self):
        m = _simple_module()
        m.statics.append(OZStaticVar("_count", OZType("int")))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "int _count;" in content

    def test_no_statics_no_functions_file(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            assert not os.path.exists(os.path.join(tmpdir, "oz_functions.c"))

    def test_compound_literal_expr(self):
        """CompoundLiteralExpr + InitListExpr → (type){val, val}"""
        m = _simple_module()
        m.classes["OZLed"].methods.append(
            OZMethod("setup", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "c",
                        "type": {"qualType": "struct color"},
                        "inner": [{
                            "kind": "CompoundLiteralExpr",
                            "type": {"qualType": "struct color"},
                            "inner": [{
                                "kind": "InitListExpr",
                                "inner": [
                                    {"kind": "IntegerLiteral", "value": "255"},
                                    {"kind": "IntegerLiteral", "value": "0"},
                                    {"kind": "IntegerLiteral", "value": "0"},
                                ],
                            }],
                        }],
                    }],
                }],
            })
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "OZLed_ozm.c")).read()
            assert "(struct color){255, 0, 0}" in src

    def test_string_literal_emits_static_struct(self):
        """ObjCStringLiteral → static struct OZString + reference."""
        m = _simple_module()
        m.classes["OZString"] = OZClass(
            "OZString", superclass="OZObject",
            ivars=[
                OZIvar("_length", OZType("unsigned int")),
                OZIvar("_hash", OZType("unsigned int")),
                OZIvar("_data", OZType("const char *")),
            ],
            methods=[
                OZMethod("cStr", OZType("const char *"), body_ast={
                    "kind": "CompoundStmt",
                    "inner": [{"kind": "ReturnStmt", "inner": [
                        {"kind": "MemberExpr",
                         "name": "_data",
                         "type": {"qualType": "const char *"},
                         "inner": [{"kind": "DeclRefExpr",
                                     "referencedDecl": {"name": "self"},
                                     "type": {"qualType": "OZString *"}}]},
                    ]}],
                }),
            ],
        )
        m.functions.append(OZFunction(
            name="test_func",
            return_type=OZType("void"),
            body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "s",
                        "type": {"qualType": "OZString *"},
                        "inner": [{
                            "kind": "ObjCStringLiteral",
                            "inner": [{"kind": "StringLiteral",
                                        "value": '"hello"'}],
                        }],
                    }],
                }],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "static struct OZString _oz_str_" in src
            assert '"hello"' in src
            assert "2147483647" in src

    def test_string_literal_dedup(self):
        """Identical string literals reuse the same static struct."""
        m = _simple_module()
        m.classes["OZString"] = OZClass(
            "OZString", superclass="OZObject",
            ivars=[
                OZIvar("_length", OZType("unsigned int")),
                OZIvar("_hash", OZType("unsigned int")),
                OZIvar("_data", OZType("const char *")),
            ],
        )
        # Two uses of @"hello" in the same function
        m.functions.append(OZFunction(
            name="test_func",
            return_type=OZType("void"),
            body_ast={
                "kind": "CompoundStmt",
                "inner": [
                    {"kind": "DeclStmt", "inner": [{
                        "kind": "VarDecl", "name": "a",
                        "type": {"qualType": "OZString *"},
                        "inner": [{"kind": "ObjCStringLiteral",
                                   "inner": [{"kind": "StringLiteral",
                                              "value": '"hello"'}]}],
                    }]},
                    {"kind": "DeclStmt", "inner": [{
                        "kind": "VarDecl", "name": "b",
                        "type": {"qualType": "OZString *"},
                        "inner": [{"kind": "ObjCStringLiteral",
                                   "inner": [{"kind": "StringLiteral",
                                              "value": '"hello"'}]}],
                    }]},
                ],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert src.count('static struct OZString') == 1
            assert src.count('"hello"') == 1

    def test_array_literal(self):
        """ObjCArrayLiteral → dynamic OZArray via OZArray_initWithItems."""
        m = _simple_module()
        m.classes["OZString"] = OZClass(
            "OZString", superclass="OZObject",
            ivars=[
                OZIvar("_length", OZType("unsigned int")),
                OZIvar("_hash", OZType("unsigned int")),
                OZIvar("_data", OZType("const char *")),
            ],
        )
        m.classes["OZArray"] = OZClass(
            "OZArray", superclass="OZObject",
            ivars=[
                OZIvar("_items", OZType("id *")),
                OZIvar("_count", OZType("unsigned int")),
            ],
        )
        m.functions.append(OZFunction(
            name="test_arr",
            return_type=OZType("void"),
            body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "a",
                        "type": {"qualType": "OZArray *"},
                        "inner": [{
                            "kind": "ObjCArrayLiteral",
                            "type": {"qualType": "NSArray *"},
                            "inner": [
                                {"kind": "ObjCStringLiteral",
                                 "inner": [{"kind": "StringLiteral",
                                            "value": '"hello"'}]},
                                {"kind": "ObjCStringLiteral",
                                 "inner": [{"kind": "StringLiteral",
                                            "value": '"world"'}]},
                            ],
                        }],
                    }],
                }],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZArray_initWithItems" in src
            assert "_oz_arr_" in src
            assert "_buf[]" in src

    def test_dictionary_literal(self):
        """ObjCDictionaryLiteral → dynamic OZDictionary via initWithKeysValues."""
        m = _simple_module()
        m.classes["OZString"] = OZClass(
            "OZString", superclass="OZObject",
            ivars=[
                OZIvar("_length", OZType("unsigned int")),
                OZIvar("_hash", OZType("unsigned int")),
                OZIvar("_data", OZType("const char *")),
            ],
        )
        m.classes["OZDictionary"] = OZClass(
            "OZDictionary", superclass="OZObject",
            ivars=[
                OZIvar("_keys", OZType("id *")),
                OZIvar("_values", OZType("id *")),
                OZIvar("_count", OZType("unsigned int")),
            ],
        )
        m.functions.append(OZFunction(
            name="test_dict",
            return_type=OZType("void"),
            body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "d",
                        "type": {"qualType": "OZDictionary *"},
                        "inner": [{
                            "kind": "ObjCDictionaryLiteral",
                            "type": {"qualType": "NSDictionary *"},
                            "inner": [
                                {"kind": "ObjCStringLiteral",
                                 "inner": [{"kind": "StringLiteral",
                                            "value": '"key"'}]},
                                {"kind": "ObjCStringLiteral",
                                 "inner": [{"kind": "StringLiteral",
                                            "value": '"value"'}]},
                            ],
                        }],
                    }],
                }],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZDictionary_initWithKeysValues" in src
            assert "_oz_dict_" in src
            assert "_kv[]" in src

    def test_number_literal(self):
        """ObjCBoxedExpr with IntegerLiteral → dynamic OZNumber_initInt32."""
        m = _simple_module()
        m.classes["OZNumber"] = OZClass(
            "OZNumber", superclass="OZObject",
            ivars=[
                OZIvar("_tag", OZType("enum oz_number_tag")),
                OZIvar("_value", OZType("union oz_number_value")),
            ],
        )
        m.functions.append(OZFunction(
            name="test_num",
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
                                "kind": "IntegerLiteral",
                                "value": "42",
                            }],
                        }],
                    }],
                }],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZNumber_initInt32(42)" in src

    def test_number_literal_each_alloc(self):
        """Each boxed number literal produces its own dynamic allocation."""
        m = _simple_module()
        m.classes["OZNumber"] = OZClass(
            "OZNumber", superclass="OZObject",
            ivars=[
                OZIvar("_tag", OZType("enum oz_number_tag")),
                OZIvar("_value", OZType("union oz_number_value")),
            ],
        )
        m.functions.append(OZFunction(
            name="test_allocs",
            return_type=OZType("void"),
            body_ast={
                "kind": "CompoundStmt",
                "inner": [
                    {"kind": "DeclStmt", "inner": [{
                        "kind": "VarDecl", "name": "a",
                        "type": {"qualType": "OZNumber *"},
                        "inner": [{"kind": "ObjCBoxedExpr",
                                   "type": {"qualType": "NSNumber *"},
                                   "inner": [{"kind": "IntegerLiteral",
                                              "value": "42"}]}],
                    }]},
                    {"kind": "DeclStmt", "inner": [{
                        "kind": "VarDecl", "name": "b",
                        "type": {"qualType": "OZNumber *"},
                        "inner": [{"kind": "ObjCBoxedExpr",
                                   "type": {"qualType": "NSNumber *"},
                                   "inner": [{"kind": "IntegerLiteral",
                                              "value": "42"}]}],
                    }]},
                ],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert src.count("OZNumber_initInt32(42)") == 2

    def test_expr_with_cleanups_passthrough(self):
        """ExprWithCleanups wrapping an expression → unwraps inner."""
        m = _simple_module()
        m.functions.append(OZFunction(
            name="test_cleanup",
            return_type=OZType("void"),
            body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "x",
                        "type": {"qualType": "int"},
                        "inner": [{
                            "kind": "ExprWithCleanups",
                            "type": {"qualType": "int"},
                            "inner": [{
                                "kind": "IntegerLiteral",
                                "value": "99",
                            }],
                        }],
                    }],
                }],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "int x = 99;" in src

    def test_block_expr_non_capturing(self):
        """BlockExpr without captures → static C function + name."""
        m = _simple_module()
        m.functions.append(OZFunction(
            name="test_block",
            return_type=OZType("void"),
            body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "blk",
                        "type": {"qualType": "void (^)(int)"},
                        "inner": [{
                            "kind": "ExprWithCleanups",
                            "inner": [{
                                "kind": "BlockExpr",
                                "type": {"qualType": "void (^)(int)"},
                                "inner": [{
                                    "kind": "BlockDecl",
                                    "inner": [
                                        {
                                            "kind": "ParmVarDecl",
                                            "name": "val",
                                            "type": {"qualType": "int"},
                                        },
                                        {
                                            "kind": "CompoundStmt",
                                            "inner": [],
                                        },
                                    ],
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
            assert "static void _oz_block_" in src
            assert "int val" in src

    def test_block_expr_with_capture_raises(self):
        """BlockExpr with captures → diagnostic error."""
        m = _simple_module()
        m.functions.append(OZFunction(
            name="test_capture",
            return_type=OZType("void"),
            body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "blk",
                        "type": {"qualType": "void (^)(void)"},
                        "inner": [{
                            "kind": "BlockExpr",
                            "type": {"qualType": "void (^)(void)"},
                            "inner": [{
                                "kind": "BlockDecl",
                                "inner": [
                                    {"kind": "Capture",
                                     "var": {"name": "sum"}},
                                    {"kind": "CompoundStmt",
                                     "inner": []},
                                ],
                            }],
                        }],
                    }],
                }],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            assert any("sum" in e for e in m.errors)

    def test_ivar_type_defs_in_class_header(self):
        """Class with enum/union ivars gets type_defs in header."""
        m = _simple_module()
        m.classes["OZNumber"] = OZClass(
            "OZNumber", superclass="OZObject",
            ivars=[
                OZIvar("_tag", OZType("enum oz_number_tag")),
                OZIvar("_value", OZType("union oz_number_value")),
            ],
        )
        m.type_defs["enum oz_number_tag"] = (
            "enum oz_number_tag {\n\tOZ_NUM_INT32 = 0,\n\tOZ_NUM_UINT32,\n};"
        )
        m.type_defs["union oz_number_value"] = (
            "union oz_number_value {\n\tint i32;\n\tfloat f32;\n};"
        )
        from oz_transpile.resolve import resolve
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            hdr = open(os.path.join(tmpdir, "Foundation", "OZNumber_ozh.h")).read()
            assert "enum oz_number_tag {" in hdr
            assert "OZ_NUM_INT32 = 0," in hdr
            assert "union oz_number_value {" in hdr
            assert "struct OZNumber {" in hdr
            # Type defs must appear before struct
            enum_pos = hdr.index("enum oz_number_tag {")
            struct_pos = hdr.index("struct OZNumber {")
            assert enum_pos < struct_pos

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
        m = _simple_module()
        m.classes["OZArray"] = OZClass(
            "OZArray", superclass="OZObject",
            methods=[
                OZMethod("objectAtIndexedSubscript:", OZType("id"),
                         params=[OZParam("index", OZType("unsigned int"))],
                         body_ast={"kind": "CompoundStmt", "inner": []}),
            ],
        )
        m.classes["OZLed"].methods.append(
            OZMethod("test", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "first",
                        "type": {"qualType": "id"},
                        "inner": [{
                            "kind": "PseudoObjectExpr",
                            "type": {"qualType": "id"},
                            "inner": [
                                {"kind": "ObjCSubscriptRefExpr",
                                 "type": {"qualType": "id"},
                                 "inner": [
                                     {"kind": "OpaqueValueExpr",
                                      "type": {"qualType": "OZArray *"},
                                      "inner": [
                                          {"kind": "ImplicitCastExpr",
                                           "type": {"qualType": "OZArray *"},
                                           "inner": [
                                               {"kind": "DeclRefExpr",
                                                "referencedDecl": {"name": "arr"},
                                                "type": {"qualType": "OZArray *"}},
                                           ]},
                                      ]},
                                     {"kind": "OpaqueValueExpr",
                                      "type": {"qualType": "unsigned int"},
                                      "inner": [
                                          {"kind": "IntegerLiteral",
                                           "value": "0",
                                           "type": {"qualType": "unsigned int"}},
                                      ]},
                                 ]},
                                {"kind": "OpaqueValueExpr",
                                 "type": {"qualType": "OZArray *"},
                                 "inner": [
                                     {"kind": "ImplicitCastExpr",
                                      "type": {"qualType": "OZArray *"},
                                      "inner": [
                                          {"kind": "DeclRefExpr",
                                           "referencedDecl": {"name": "arr"},
                                           "type": {"qualType": "OZArray *"}},
                                      ]},
                                 ]},
                                {"kind": "OpaqueValueExpr",
                                 "type": {"qualType": "unsigned int"},
                                 "inner": [
                                     {"kind": "IntegerLiteral",
                                      "value": "0",
                                      "type": {"qualType": "unsigned int"}},
                                 ]},
                                {"kind": "ObjCMessageExpr",
                                 "selector": "objectAtIndexedSubscript:",
                                 "type": {"qualType": "id"},
                                 "inner": [
                                     {"kind": "OpaqueValueExpr",
                                      "type": {"qualType": "OZArray *"},
                                      "inner": [
                                          {"kind": "ImplicitCastExpr",
                                           "type": {"qualType": "OZArray *"},
                                           "inner": [
                                               {"kind": "DeclRefExpr",
                                                "referencedDecl": {"name": "arr"},
                                                "type": {"qualType": "OZArray *"}},
                                           ]},
                                      ]},
                                     {"kind": "ImplicitCastExpr",
                                      "type": {"qualType": "unsigned int"},
                                      "inner": [
                                          {"kind": "OpaqueValueExpr",
                                           "type": {"qualType": "unsigned int"},
                                           "inner": [
                                               {"kind": "IntegerLiteral",
                                                "value": "0",
                                                "type": {"qualType": "unsigned int"}},
                                           ]},
                                      ]},
                                 ]},
                            ],
                        }],
                    }],
                }],
            })
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "OZLed_ozm.c")).read()
            assert "OZArray_objectAtIndexedSubscript_" in src


class TestSynthesizedPropertyEmission:
    """Test _emit_synthesized_accessor generates correct C code."""

    def _emit(self, cls, method, root_class="OZObject"):
        from io import StringIO
        buf = StringIO()
        _emit_synthesized_accessor(cls, method, buf, root_class)
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
        assert "oz_spinlock_t lck;" in code
        assert "OZ_SPINLOCK(&lck)" in code
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
        assert "oz_spinlock_t lck;" in code
        assert "OZ_SPINLOCK(&lck)" in code
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
        assert "oz_spinlock_t lck;" in code
        assert "OZ_SPINLOCK(&lck)" in code
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


class TestSynchronized:
    """Tests for @synchronized -> OZLock RAII emission."""

    @staticmethod
    def _sync_ast(obj_expr, body_stmts):
        """Build an ObjCAtSynchronizedStmt AST node."""
        return {
            "kind": "ObjCAtSynchronizedStmt",
            "inner": [
                obj_expr,
                {"kind": "CompoundStmt", "inner": body_stmts},
            ],
        }

    @staticmethod
    def _self_ref():
        return {"kind": "DeclRefExpr",
                "referencedDecl": {"name": "self"},
                "type": {"qualType": "Foo *"}}

    @staticmethod
    def _int_assign(val):
        return {"kind": "BinaryOperator", "opcode": "=", "inner": [
            {"kind": "MemberExpr", "name": "_count",
             "type": {"qualType": "int"}, "inner": [
                {"kind": "DeclRefExpr",
                 "referencedDecl": {"name": "self"},
                 "type": {"qualType": "Foo *"}}
             ]},
            {"kind": "IntegerLiteral", "value": str(val),
             "type": {"qualType": "int"}},
        ], "type": {"qualType": "int"}}

    def test_basic_synchronized(self):
        """@synchronized(self) { self->_count = 1; }"""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject",
                                   ivars=[OZIvar("_count", OZType("int"))],
                                   methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [
                    self._sync_ast(self._self_ref(), [self._int_assign(1)]),
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "struct OZLock *_sync = OZLock_initWithObject(" in content
            assert "OZLock_alloc()" in content
            assert "(struct OZObject *)self" in content
            assert "OZObject_release((struct OZObject *)_sync);" in content

    def test_synchronized_oz_lock_slab(self):
        """OZLock slab generated when @synchronized is used."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [
                    self._sync_ast(self._self_ref(), []),
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            dispatch_h = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            assert "OZ_CLASS_OZLock" in dispatch_h

    def test_synchronized_early_return(self):
        """Early return inside @synchronized releases the lock."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [
                    self._sync_ast(self._self_ref(), [
                        {"kind": "ReturnStmt", "inner": []},
                    ]),
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            ret_pos = content.index("return;")
            release_pos = content.index("OZObject_release((struct OZObject *)_sync)")
            assert release_pos < ret_pos

    def test_synchronized_nested_mangled_names(self):
        """Nested @synchronized uses _sync, _sync2."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [
                    self._sync_ast(self._self_ref(), [
                        self._sync_ast(self._self_ref(), []),
                    ]),
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "struct OZLock *_sync = " in content
            assert "struct OZLock *_sync2 = " in content

    def test_no_oz_lock_without_synchronized(self):
        """OZLock not injected when no @synchronized used."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            dispatch_h = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            assert "OZLock" not in dispatch_h

    def test_synchronized_with_ivar_obj(self):
        """@synchronized(_mutex) uses ivar expression."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject",
                                   ivars=[OZIvar("_mutex", OZType("OZObject *"))],
                                   methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [
                    self._sync_ast(
                        {"kind": "MemberExpr", "name": "_mutex",
                         "type": {"qualType": "OZObject *"}, "inner": [
                            {"kind": "DeclRefExpr",
                             "referencedDecl": {"name": "self"},
                             "type": {"qualType": "Foo *"}}
                         ]},
                        [self._int_assign(1)]),
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "(struct OZObject *)self->_mutex" in content

    def test_synchronized_with_object_local_inside(self):
        """Object local inside @synchronized released alongside _sync."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [
                    self._sync_ast(self._self_ref(), [
                        {"kind": "DeclStmt", "inner": [{
                            "kind": "VarDecl", "name": "tmp",
                            "type": {"qualType": "OZObject *"},
                            "inner": [{"kind": "IntegerLiteral", "value": "0"}],
                        }]},
                    ]),
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "OZObject_release((struct OZObject *)tmp)" in content
            assert "OZObject_release((struct OZObject *)_sync)" in content

    def test_synchronized_in_loop_break_releases(self):
        """Break inside @synchronized inside loop releases _sync."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "WhileStmt", "inner": [
                        {"kind": "IntegerLiteral", "value": "1"},
                        {"kind": "CompoundStmt", "inner": [
                            self._sync_ast(self._self_ref(), [
                                {"kind": "BreakStmt"},
                            ]),
                        ]},
                    ],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            break_idx = content.index("break;")
            before_break = content[:break_idx]
            assert "release((struct OZObject *)_sync)" in before_break

    def test_synchronized_return_with_value(self):
        """Return expr inside @synchronized releases _sync but not returned var."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("getValue", OZType("int"), body_ast={
                "kind": "CompoundStmt", "inner": [
                    self._sync_ast(self._self_ref(), [
                        {"kind": "ReturnStmt", "inner": [
                            {"kind": "IntegerLiteral", "value": "42",
                             "type": {"qualType": "int"}},
                        ]},
                    ]),
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            ret_pos = content.index("return 42;")
            release_pos = content.index("release((struct OZObject *)_sync)")
            assert release_pos < ret_pos

    def test_sequential_synchronized_counter(self):
        """Two sequential @synchronized in same method get _sync, _sync2."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [
                    self._sync_ast(self._self_ref(), []),
                    self._sync_ast(self._self_ref(), []),
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "struct OZLock *_sync = " in content
            assert "struct OZLock *_sync2 = " in content

    def test_dispatch_free_includes_oz_lock(self):
        """dispatch_free switch includes OZLock case."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [
                    self._sync_ast(self._self_ref(), []),
                ],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            dispatch_c = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.c")).read()
            assert "case OZ_CLASS_OZLock: OZLock_free(" in dispatch_c

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
                    self._sync_ast(
                        {"kind": "DeclRefExpr",
                         "referencedDecl": {"name": "self"},
                         "type": {"qualType": "Foo *"}},
                        [{"kind": "NullStmt"}]),
                ],
            }),
        ])
        resolve(m)
        repo_root = os.path.dirname(os.path.dirname(os.path.dirname(
            os.path.dirname(os.path.abspath(__file__)))))
        pal_inc = os.path.join(repo_root, "include")
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
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
            # Should appear once (from _emit_class_methods), not twice
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
            # Extra space between #include and path
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
            # The preamble has the canonical include; extra-space one should be deduped
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
            # Both classes share the same stem — deps should include Bar's deps
            result = _emit_patched_source(
                Path(f.name), m, [cls_a, cls_b], "Foo", "OZObject", False)
            # OZObject is a dep of both; its header should be included
            assert '#include "OZObject_ozh.h"' in result
        finally:
            os.unlink(f.name)

    def test_include_replacement_flattens_subdir_path(self):
        """#import with subdirectory prefix should emit flat #include (OZ-001)."""
        from io import StringIO
        from pathlib import Path

        with tempfile.TemporaryDirectory() as tmpdir:
            # Create services/PXAppConfig.h as an ObjC header
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
            # Must be flat — no "services/" prefix
            assert result.strip() == '#include "PXAppConfig_ozh.h"'

    def test_patched_source_flattens_subdir_import(self):
        """Patched source with subdirectory #import should flatten include (OZ-001)."""
        from pathlib import Path

        with tempfile.TemporaryDirectory() as tmpdir:
            # Create main.m that imports a subdirectory header
            main_m = os.path.join(tmpdir, "main.m")
            with open(main_m, "w") as f:
                f.write('#import "services/PXAppConfig.h"\n')
                f.write("@implementation Foo\n@end\n")

            # Create the subdirectory header so _find_header can locate it
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
            # Generated include must be flat, not "services/PXAppConfig_ozh.h"
            assert '#include "PXAppConfig_ozh.h"' in result
            assert "services/PXAppConfig_ozh.h" not in result


class TestProtocolDispatchReturnCast:
    """OZ-003: protocol dispatch with object return type must cast to
    the declared return type, not the receiver class."""

    def test_protocol_dispatch_object_return_cast(self):
        """OZ_SEND for a protocol method returning OZString * should cast
        to (struct OZString *), not (struct __patched__ *)."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["OZString"] = OZClass("OZString", superclass="OZObject")
        m.protocols["PXSensorProtocol"] = OZProtocol(
            "PXSensorProtocol",
            methods=[OZMethod("name", OZType("OZString *"))])
        # Sensor class implements the protocol method (needed for PROTOCOL dispatch)
        m.classes["PXSensor"] = OZClass("PXSensor", superclass="OZObject",
            protocols=["PXSensorProtocol"],
            methods=[OZMethod("name", OZType("OZString *"), body_ast={
                "kind": "CompoundStmt", "inner": []})])
        # Class with a method that calls [sensor name] via protocol dispatch
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "sensorName",
                        "type": {"qualType": "OZString *"},
                        "inner": [{
                            "kind": "ObjCMessageExpr",
                            "selector": "name",
                            "type": {"qualType": "OZString *"},
                            "inner": [{
                                "kind": "ImplicitCastExpr",
                                "type": {"qualType": "id"},
                                "inner": [{
                                    "kind": "DeclRefExpr",
                                    "referencedDecl": {"name": "sensor"},
                                    "type": {"qualType": "id"},
                                }],
                            }],
                        }],
                    }],
                }],
            })])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "App_ozm.c")).read()
            assert "__patched__" not in content
            assert "(struct OZString *)" in content
            assert "OZ_SEND_name" in content


class TestReturnProtocolDispatch:
    """OZ-005: protocol dispatch in return statement must declare receiver var."""

    def test_return_protocol_dispatch_emits_receiver_var(self):
        """return [_sensors count]; must declare _oz_recv before the return."""
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
                            "type": {"qualType": "OZArray *"},
                            "inner": [{
                                "kind": "ObjCIvarRefExpr",
                                "decl": {"name": "_sensors"},
                                "type": {"qualType": "OZArray *"},
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
            assert "OZ_SEND_count" in content
            # The receiver var must appear BEFORE the return statement
            recv_pos = content.index("_oz_recv")
            ret_pos = content.index("return")
            assert recv_pos < ret_pos


class TestUserEnumEmission:
    """OZ-007: user-defined enum collected and emitted in class header."""

    def test_enum_ivar_type_emitted_in_header(self):
        """Enum used as ivar type appears in the generated header."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.type_defs["enum PXDeviceState"] = (
            "enum PXDeviceState {\n"
            "\tPXDeviceStateIdle = 0,\n"
            "\tPXDeviceStateRunning = 1,\n"
            "};"
        )
        m.classes["Manager"] = OZClass("Manager", superclass="OZObject",
            ivars=[OZIvar("_state", OZType("enum PXDeviceState"))])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            header = open(os.path.join(tmpdir, "Manager_ozh.h")).read()
            assert "PXDeviceStateIdle" in header
            assert "PXDeviceStateRunning" in header


class TestSwitchCaseEmission:
    """OZ-012: switch/case statement emission."""

    def test_switch_case_emitted(self):
        """switch(cond) { case X: ... break; } should be fully emitted."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Mgr"] = OZClass("Mgr", superclass="OZObject",
            ivars=[OZIvar("_state", OZType("int"))],
            methods=[OZMethod("start", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "SwitchStmt",
                    "inner": [
                        {"kind": "ImplicitCastExpr",
                         "castKind": "LValueToRValue",
                         "type": {"qualType": "int"},
                         "inner": [{"kind": "ObjCIvarRefExpr",
                                    "decl": {"name": "_state"},
                                    "type": {"qualType": "int"}}]},
                        {"kind": "CompoundStmt", "inner": [
                            {"kind": "CaseStmt", "inner": [
                                {"kind": "ConstantExpr", "value": "0",
                                 "inner": [{"kind": "DeclRefExpr",
                                            "referencedDecl": {"name": "PXDeviceStateIdle"},
                                            "type": {"qualType": "int"}}]},
                                {"kind": "BinaryOperator", "opcode": "=",
                                 "type": {"qualType": "int"},
                                 "inner": [
                                     {"kind": "ObjCIvarRefExpr",
                                      "decl": {"name": "_state"},
                                      "type": {"qualType": "int"}},
                                     {"kind": "IntegerLiteral", "value": "1",
                                      "type": {"qualType": "int"}},
                                 ]},
                            ]},
                            {"kind": "BreakStmt"},
                            {"kind": "DefaultStmt", "inner": [
                                {"kind": "BreakStmt"},
                            ]},
                        ]},
                    ],
                }],
            })])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Mgr_ozm.c")).read()
            assert "switch (self->_state)" in content
            assert "case PXDeviceStateIdle:" in content
            assert "self->_state = 1;" in content
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
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Mgr"] = OZClass("Mgr", superclass="OZObject",
            statics=[OZStaticVar("_shared", OZType("int"))],
            methods=[OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": []})])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            header = open(os.path.join(tmpdir, "Mgr_ozh.h")).read()
            source = open(os.path.join(tmpdir, "Mgr_ozm.c")).read()
            assert "extern" not in header or "_shared" not in header
            assert "static" in source and "_shared" in source


class TestProtocolDispatchEdgeCases:
    """OZ-020: protocol dispatch edge cases."""

    def _make_protocol_module(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["OZString"] = OZClass("OZString", superclass="OZObject")
        m.classes["OZArray"] = OZClass("OZArray", superclass="OZObject")
        m.protocols["Proto"] = OZProtocol("Proto", methods=[
            OZMethod("name", OZType("OZString *")),
            OZMethod("count", OZType("unsigned int")),
            OZMethod("reset", OZType("void")),
        ])
        m.classes["Impl"] = OZClass("Impl", superclass="OZObject",
            protocols=["Proto"],
            methods=[
                OZMethod("name", OZType("OZString *"), body_ast={
                    "kind": "CompoundStmt", "inner": []}),
                OZMethod("count", OZType("unsigned int"), body_ast={
                    "kind": "CompoundStmt", "inner": []}),
                OZMethod("reset", OZType("void"), body_ast={
                    "kind": "CompoundStmt", "inner": []}),
            ])
        return m

    def test_void_return_no_cast(self):
        """void-returning protocol method should NOT cast."""
        m = self._make_protocol_module()
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "ObjCMessageExpr", "selector": "reset",
                    "type": {"qualType": "void"},
                    "inner": [{"kind": "DeclRefExpr",
                               "referencedDecl": {"name": "obj"},
                               "type": {"qualType": "id"}}],
                }]})])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "App_ozm.c")).read()
            assert "OZ_SEND_reset" in content
            assert "(struct" not in content.split("OZ_SEND_reset")[0].split("\n")[-1]

    def test_int_return_no_struct_cast(self):
        """int-returning protocol method should NOT cast to struct."""
        m = self._make_protocol_module()
        m.classes["App"] = OZClass("App", superclass="OZObject",
            methods=[OZMethod("run", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "DeclStmt", "inner": [{
                        "kind": "VarDecl", "name": "n",
                        "type": {"qualType": "unsigned int"},
                        "inner": [{
                            "kind": "ObjCMessageExpr", "selector": "count",
                            "type": {"qualType": "unsigned int"},
                            "inner": [{"kind": "DeclRefExpr",
                                       "referencedDecl": {"name": "obj"},
                                       "type": {"qualType": "id"}}],
                        }],
                    }],
                }]})])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "App_ozm.c")).read()
            assert "OZ_SEND_count" in content
            assert "(struct OZObject *)OZ_SEND_count" not in content


class TestSwitchCaseEdgeCases:
    """OZ-022: switch/case edge cases."""

    def test_fall_through_cases(self):
        """Consecutive cases without break (fall-through)."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Mgr"] = OZClass("Mgr", superclass="OZObject",
            ivars=[OZIvar("_state", OZType("int"))],
            methods=[OZMethod("handle", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "SwitchStmt", "inner": [
                        {"kind": "ImplicitCastExpr", "castKind": "LValueToRValue",
                         "type": {"qualType": "int"},
                         "inner": [{"kind": "ObjCIvarRefExpr",
                                    "decl": {"name": "_state"},
                                    "type": {"qualType": "int"}}]},
                        {"kind": "CompoundStmt", "inner": [
                            {"kind": "CaseStmt", "inner": [
                                {"kind": "IntegerLiteral", "value": "0",
                                 "type": {"qualType": "int"}},
                            ]},
                            {"kind": "CaseStmt", "inner": [
                                {"kind": "IntegerLiteral", "value": "1",
                                 "type": {"qualType": "int"}},
                                {"kind": "BreakStmt"},
                            ]},
                        ]},
                    ],
                }]})])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Mgr_ozm.c")).read()
            assert "case 0:" in content
            assert "case 1:" in content
            assert "break;" in content

    def test_switch_no_default(self):
        """Switch with only case labels, no default."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Mgr"] = OZClass("Mgr", superclass="OZObject",
            ivars=[OZIvar("_x", OZType("int"))],
            methods=[OZMethod("go", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "SwitchStmt", "inner": [
                        {"kind": "ImplicitCastExpr", "castKind": "LValueToRValue",
                         "type": {"qualType": "int"},
                         "inner": [{"kind": "ObjCIvarRefExpr",
                                    "decl": {"name": "_x"},
                                    "type": {"qualType": "int"}}]},
                        {"kind": "CompoundStmt", "inner": [
                            {"kind": "CaseStmt", "inner": [
                                {"kind": "IntegerLiteral", "value": "42",
                                 "type": {"qualType": "int"}},
                                {"kind": "BreakStmt"},
                            ]},
                        ]},
                    ],
                }]})])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Mgr_ozm.c")).read()
            assert "switch (self->_x)" in content
            assert "case 42:" in content
            assert "default:" not in content


class TestInheritedMethodCast:
    """OZ-017: inherited method calls must cast self to declaring class."""

    def test_inherited_method_casts_self(self):
        """[self parentMethod] where parentMethod is in grandparent class."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Base"] = OZClass("Base", superclass="OZObject",
            methods=[OZMethod("readRaw", OZType("int"), body_ast={
                "kind": "CompoundStmt", "inner": []})])
        m.classes["Child"] = OZClass("Child", superclass="Base",
            methods=[OZMethod("process", OZType("int"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "ReturnStmt", "inner": [{
                        "kind": "ObjCMessageExpr",
                        "selector": "readRaw",
                        "type": {"qualType": "int"},
                        "inner": [{
                            "kind": "DeclRefExpr",
                            "referencedDecl": {"name": "self"},
                            "type": {"qualType": "Child *"},
                        }],
                    }],
                }],
            })])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Child_ozm.c")).read()
            assert "(struct Base *)self" in content
            assert "Base_readRaw(" in content

    def test_same_class_method_no_cast(self):
        """OZ-028: same-class method must NOT cast self."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject",
            methods=[
                OZMethod("helper", OZType("int"), body_ast={
                    "kind": "CompoundStmt", "inner": []}),
                OZMethod("run", OZType("void"), body_ast={
                    "kind": "CompoundStmt", "inner": [{
                        "kind": "ObjCMessageExpr", "selector": "helper",
                        "type": {"qualType": "int"},
                        "inner": [{"kind": "DeclRefExpr",
                                   "referencedDecl": {"name": "self"},
                                   "type": {"qualType": "Foo *"}}],
                    }]}),
            ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "Foo_helper(self)" in content
            assert "(struct Foo *)self" not in content


class TestParentIvarAccess:
    """OZ-019: subclass access to parent ivars via base chain."""

    def test_parent_ivar_uses_base_prefix(self):
        """self->_parentIvar must become self->base._parentIvar."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Parent"] = OZClass("Parent", superclass="OZObject",
            ivars=[OZIvar("_count", OZType("int"))])
        m.classes["Child"] = OZClass("Child", superclass="Parent",
            ivars=[OZIvar("_value", OZType("int"))],
            methods=[OZMethod("inc", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "BinaryOperator", "opcode": "=",
                    "type": {"qualType": "int"},
                    "inner": [
                        {"kind": "ObjCIvarRefExpr",
                         "decl": {"name": "_count"},
                         "type": {"qualType": "int"}},
                        {"kind": "IntegerLiteral", "value": "1",
                         "type": {"qualType": "int"}},
                    ],
                }]})])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Child_ozm.c")).read()
            assert "self->base._count" in content

    def test_grandparent_ivar_double_base(self):
        """Grandparent ivar needs self->base.base._ivar."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["GrandP"] = OZClass("GrandP", superclass="OZObject",
            ivars=[OZIvar("_gval", OZType("int"))])
        m.classes["Parent"] = OZClass("Parent", superclass="GrandP",
            ivars=[OZIvar("_pval", OZType("int"))])
        m.classes["Child"] = OZClass("Child", superclass="Parent",
            ivars=[OZIvar("_cval", OZType("int"))],
            methods=[OZMethod("read", OZType("int"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "ReturnStmt", "inner": [{
                        "kind": "ObjCIvarRefExpr",
                        "decl": {"name": "_gval"},
                        "type": {"qualType": "int"},
                    }],
                }]})])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Child_ozm.c")).read()
            assert "self->base.base._gval" in content

    def test_own_ivar_no_base_prefix(self):
        """Own class ivar must NOT get base prefix."""
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject")
        m.classes["Parent"] = OZClass("Parent", superclass="OZObject",
            ivars=[OZIvar("_count", OZType("int"))])
        m.classes["Child"] = OZClass("Child", superclass="Parent",
            ivars=[OZIvar("_value", OZType("int"))],
            methods=[OZMethod("get", OZType("int"), body_ast={
                "kind": "CompoundStmt", "inner": [{
                    "kind": "ReturnStmt", "inner": [{
                        "kind": "ObjCIvarRefExpr",
                        "decl": {"name": "_value"},
                        "type": {"qualType": "int"},
                    }],
                }]})])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Child_ozm.c")).read()
            assert "self->_value" in content
            assert "self->base._value" not in content

    # -------------------------------------------------------------------
    # Boxed expression @(expr) tests
    # -------------------------------------------------------------------

    def _boxed_expr_module(self, inner_ast, inner_qt="int"):
        """Helper: build a module with a function containing @(expr)."""
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
                                "type": {"qualType": inner_qt},
                                "castKind": "LValueToRValue",
                                "inner": [inner_ast],
                            }],
                        }],
                    }],
                }],
            },
        ))
        return m

    def test_boxed_variable_int(self):
        """@(myInt) with int type → OZNumber_initInt32(myInt)."""
        m = self._boxed_expr_module(
            {"kind": "DeclRefExpr",
             "referencedDecl": {"name": "myInt"},
             "type": {"qualType": "int"}},
            inner_qt="int",
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZNumber_initInt32(myInt)" in src

    def test_boxed_binary_expr(self):
        """@(a + b) with int type → OZNumber_initInt32(a + b)."""
        inner_ast = {
            "kind": "BinaryOperator",
            "opcode": "+",
            "type": {"qualType": "int"},
            "inner": [
                {"kind": "ImplicitCastExpr",
                 "type": {"qualType": "int"},
                 "castKind": "LValueToRValue",
                 "inner": [{"kind": "DeclRefExpr",
                            "referencedDecl": {"name": "a"},
                            "type": {"qualType": "int"}}]},
                {"kind": "ImplicitCastExpr",
                 "type": {"qualType": "int"},
                 "castKind": "LValueToRValue",
                 "inner": [{"kind": "DeclRefExpr",
                            "referencedDecl": {"name": "b"},
                            "type": {"qualType": "int"}}]},
            ],
        }
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
                            "inner": [inner_ast],
                        }],
                    }],
                }],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZNumber_initInt32(a + b)" in src

    def test_boxed_variable_float(self):
        """@(myFloat) with float type → OZNumber_initFloat(myFloat)."""
        m = self._boxed_expr_module(
            {"kind": "DeclRefExpr",
             "referencedDecl": {"name": "myFloat"},
             "type": {"qualType": "float"}},
            inner_qt="float",
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZNumber_initFloat(myFloat)" in src

    def test_boxed_variable_uint16(self):
        """@(myU16) with uint16_t type → OZNumber_initUint16(myU16)."""
        m = self._boxed_expr_module(
            {"kind": "DeclRefExpr",
             "referencedDecl": {"name": "myU16"},
             "type": {"qualType": "uint16_t"}},
            inner_qt="uint16_t",
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZNumber_initUint16(myU16)" in src

    def test_boxed_call_expr(self):
        """@(getValue()) with int return → OZNumber_initInt32(getValue())."""
        inner_ast = {
            "kind": "CallExpr",
            "type": {"qualType": "int"},
            "inner": [
                {"kind": "ImplicitCastExpr",
                 "type": {"qualType": "int (*)(void)"},
                 "castKind": "FunctionToPointerDecay",
                 "inner": [{"kind": "DeclRefExpr",
                            "referencedDecl": {"name": "getValue"},
                            "type": {"qualType": "int (void)"}}]},
            ],
        }
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
                            "inner": [inner_ast],
                        }],
                    }],
                }],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZNumber_initInt32(getValue())" in src

    def test_boxed_enum(self):
        """@(enumVar) with enum type → OZNumber_initInt32((int32_t)(enumVar))."""
        m = self._boxed_expr_module(
            {"kind": "DeclRefExpr",
             "referencedDecl": {"name": "enumVar"},
             "type": {"qualType": "enum Color"}},
            inner_qt="enum Color",
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZNumber_initInt32((int32_t)(enumVar))" in src

    def test_boxed_double_warns(self):
        """@(myDouble) with double → OZNumber_initFloat((float)(...)) + diagnostic."""
        m = self._boxed_expr_module(
            {"kind": "DeclRefExpr",
             "referencedDecl": {"name": "myDouble"},
             "type": {"qualType": "double"}},
            inner_qt="double",
        )
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZNumber_initFloat((float)(myDouble))" in src
            assert any("double" in d and "narrowed" in d
                       for d in m.diagnostics)

    def test_boxed_literal_regression(self):
        """Existing @42 literal path still works after refactor."""
        m = _simple_module()
        m.classes["OZNumber"] = OZClass(
            "OZNumber", superclass="OZObject",
            ivars=[
                OZIvar("_tag", OZType("enum oz_number_tag")),
                OZIvar("_value", OZType("union oz_number_value")),
            ],
        )
        m.functions.append(OZFunction(
            name="test_lit",
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
                                "kind": "IntegerLiteral",
                                "value": "99",
                            }],
                        }],
                    }],
                }],
            },
        ))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZNumber_initInt32(99)" in src
            assert not m.errors


class TestEmitEdgeCases:
    """Tests for AST node types with missing or light coverage."""

    def _emit_method(self, body_ast, class_name="Foo",
                     method_name="doWork", ret="void"):
        """Helper: emit a single method body and return the .c content."""
        m = OZModule()
        m.classes[class_name] = OZClass(class_name, methods=[
            OZMethod(method_name, OZType(ret), body_ast=body_ast),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            return open(os.path.join(tmpdir, f"{class_name}_ozm.c")).read()

    def test_null_stmt(self):
        content = self._emit_method({
            "kind": "CompoundStmt",
            "inner": [{"kind": "NullStmt"}],
        })
        assert ";\n" in content

    def test_objc_bool_literal_yes(self):
        content = self._emit_method({
            "kind": "CompoundStmt",
            "inner": [{
                "kind": "ReturnStmt",
                "inner": [{
                    "kind": "ObjCBoolLiteralExpr",
                    "value": True,
                }],
            }],
        }, ret="BOOL")
        assert "return 1;" in content

    def test_objc_bool_literal_no(self):
        content = self._emit_method({
            "kind": "CompoundStmt",
            "inner": [{
                "kind": "ReturnStmt",
                "inner": [{
                    "kind": "ObjCBoolLiteralExpr",
                    "value": False,
                }],
            }],
        }, ret="BOOL")
        assert "return 0;" in content

    def test_objc_bool_literal_string_yes(self):
        """Clang sometimes encodes YES/NO as string values."""
        content = self._emit_method({
            "kind": "CompoundStmt",
            "inner": [{
                "kind": "ReturnStmt",
                "inner": [{
                    "kind": "ObjCBoolLiteralExpr",
                    "value": "__objc_yes",
                }],
            }],
        }, ret="BOOL")
        assert "return 1;" in content

    def test_objc_bool_literal_string_no(self):
        content = self._emit_method({
            "kind": "CompoundStmt",
            "inner": [{
                "kind": "ReturnStmt",
                "inner": [{
                    "kind": "ObjCBoolLiteralExpr",
                    "value": "__objc_no",
                }],
            }],
        }, ret="BOOL")
        assert "return 0;" in content

    def test_string_literal(self):
        content = self._emit_method({
            "kind": "CompoundStmt",
            "inner": [{
                "kind": "DeclStmt",
                "inner": [{
                    "kind": "VarDecl",
                    "name": "s",
                    "type": {"qualType": "const char *"},
                    "inner": [{
                        "kind": "StringLiteral",
                        "value": '"hello world"',
                    }],
                }],
            }],
        })
        assert '"hello world"' in content

    def test_string_literal_escape(self):
        content = self._emit_method({
            "kind": "CompoundStmt",
            "inner": [{
                "kind": "DeclStmt",
                "inner": [{
                    "kind": "VarDecl",
                    "name": "s",
                    "type": {"qualType": "const char *"},
                    "inner": [{
                        "kind": "StringLiteral",
                        "value": '"line\\n"',
                    }],
                }],
            }],
        })
        assert '"line\\n"' in content

    def test_array_literal(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject", methods=[
            OZMethod("init", OZType("instancetype"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{"kind": "ReturnStmt", "inner": [
                    {"kind": "DeclRefExpr",
                     "referencedDecl": {"name": "self"},
                     "type": {"qualType": "OZObject *"}},
                ]}],
            }),
            OZMethod("dealloc", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [],
            }),
            OZMethod("test_lit", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "arr",
                        "type": {"qualType": "OZArray *"},
                        "inner": [{
                            "kind": "ObjCArrayLiteral",
                            "type": {"qualType": "NSArray *"},
                            "inner": [
                                {"kind": "ObjCStringLiteral",
                                 "inner": [{"kind": "StringLiteral",
                                            "value": '"a"'}]},
                                {"kind": "ObjCStringLiteral",
                                 "inner": [{"kind": "StringLiteral",
                                            "value": '"b"'}]},
                            ],
                        }],
                    }],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation",
                                    "OZObject_ozm.c")).read()
            assert "OZArray_initWithItems" in src
            assert "_oz_arr_" in src
            assert not m.errors

    def test_dictionary_literal(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject", methods=[
            OZMethod("init", OZType("instancetype"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{"kind": "ReturnStmt", "inner": [
                    {"kind": "DeclRefExpr",
                     "referencedDecl": {"name": "self"},
                     "type": {"qualType": "OZObject *"}},
                ]}],
            }),
            OZMethod("dealloc", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [],
            }),
            OZMethod("test_lit", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "dict",
                        "type": {"qualType": "OZDictionary *"},
                        "inner": [{
                            "kind": "ObjCDictionaryLiteral",
                            "type": {"qualType": "NSDictionary *"},
                            "inner": [
                                {"kind": "ObjCStringLiteral",
                                 "inner": [{"kind": "StringLiteral",
                                            "value": '"key"'}]},
                                {"kind": "ObjCStringLiteral",
                                 "inner": [{"kind": "StringLiteral",
                                            "value": '"val"'}]},
                            ],
                        }],
                    }],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foundation",
                                    "OZObject_ozm.c")).read()
            assert "OZDictionary_initWithKeysValues" in src
            assert "_oz_dict_" in src
            assert not m.errors

    def test_forin_stmt(self):
        m = OZModule()
        m.classes["OZObject"] = OZClass("OZObject", methods=[
            OZMethod("init", OZType("instancetype"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{"kind": "ReturnStmt", "inner": [
                    {"kind": "DeclRefExpr",
                     "referencedDecl": {"name": "self"},
                     "type": {"qualType": "OZObject *"}},
                ]}],
            }),
            OZMethod("dealloc", OZType("void"), body_ast={
                "kind": "CompoundStmt", "inner": [],
            }),
        ])
        m.classes["Foo"] = OZClass("Foo", superclass="OZObject", methods=[
            OZMethod("iterate", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "ObjCForCollectionStmt",
                    "inner": [
                        {"kind": "DeclStmt", "inner": [{
                            "kind": "VarDecl",
                            "name": "item",
                            "type": {"qualType": "id"},
                        }]},
                        {"kind": "DeclRefExpr",
                         "referencedDecl": {"name": "self"},
                         "type": {"qualType": "Foo *"}},
                        {"kind": "CompoundStmt", "inner": []},
                    ],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert "OZ_SEND_iter" in src
            assert "OZ_SEND_next" in src
            assert "item" in src

    def test_string_dedup_unique_across_methods(self):
        """OZ-039: different string literals in separate methods must get
        unique constant names (no _oz_str_N redefinition)."""
        m = OZModule()
        m.classes["Foo"] = OZClass("Foo", methods=[
            OZMethod("hello", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "s",
                        "type": {"qualType": "OZString *"},
                        "inner": [{
                            "kind": "ObjCStringLiteral",
                            "inner": [{"kind": "StringLiteral",
                                        "value": '"hello"'}],
                        }],
                    }],
                }],
            }),
            OZMethod("bye", OZType("void"), body_ast={
                "kind": "CompoundStmt",
                "inner": [{
                    "kind": "DeclStmt",
                    "inner": [{
                        "kind": "VarDecl",
                        "name": "s",
                        "type": {"qualType": "OZString *"},
                        "inner": [{
                            "kind": "ObjCStringLiteral",
                            "inner": [{"kind": "StringLiteral",
                                        "value": '"bye"'}],
                        }],
                    }],
                }],
            }),
        ])
        resolve(m)
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            src = open(os.path.join(tmpdir, "Foo_ozm.c")).read()
            assert '"hello"' in src
            assert '"bye"' in src
            # Each string should have a unique constant name
            assert src.count("static struct OZString _oz_str_") == 2
            assert not m.errors

    def test_string_dedup_uses_loc_when_available(self):
        """OZ-039: string constants use _L{line}_C{col} naming from AST loc."""
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
