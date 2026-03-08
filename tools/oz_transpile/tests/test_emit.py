# SPDX-License-Identifier: Apache-2.0

import os
import tempfile

from oz_transpile.emit import (
    emit, _selector_to_c, _base_chain, _method_prototype, _EmitCtx,
)
from oz_transpile.model import (
    DispatchKind,
    OZClass,
    OZFunction,
    OZIvar,
    OZMethod,
    OZModule,
    OZParam,
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
            assert "oz_mem_slabs.h" in basenames
            assert "oz_mem_slabs.c" in basenames
            assert "OZObject.h" in basenames
            assert "OZObject.c" in basenames
            assert "OZLed.h" in basenames
            assert "OZLed.c" in basenames


class TestDispatchHeader:
    def test_class_id_enum(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "oz_dispatch.h")).read()
            assert "OZ_CLASS_OZObject" in content
            assert "OZ_CLASS_OZLed" in content
            assert "OZ_CLASS_COUNT" in content

    def test_protocol_vtable(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "oz_dispatch.h")).read()
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
            dispatch_h = open(os.path.join(tmpdir, "oz_dispatch.h")).read()
            dispatch_c = open(os.path.join(tmpdir, "oz_dispatch.c")).read()
            assert "vtable_greet" not in dispatch_h
            assert "vtable_greet" not in dispatch_c
            # But the class method should appear in the class header
            led_h = open(os.path.join(tmpdir, "OZLed.h")).read()
            assert "OZLed_cls_greet(void)" in led_h


class TestClassHeader:
    def test_struct_with_base(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "OZLed.h")).read()
            assert "struct OZLed {" in content
            assert "struct OZObject base;" in content
            assert "int _pin;" in content

    def test_root_struct(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "OZObject.h")).read()
            assert "oz_class_id" in content
            assert "_refcount" in content


class TestClassSource:
    def test_method_body(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "OZLed.c")).read()
            assert "OZLed_turnOn" in content
            assert "struct OZLed *self" in content

    def test_root_retain_release(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "OZObject.c")).read()
            assert "OZObject_retain" in content
            assert "OZObject_release" in content
            assert "atomic_dec" in content
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
            content = open(os.path.join(tmpdir, "OZLed.c")).read()
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
            content = open(os.path.join(tmpdir, "OZLed.c")).read()
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
            content = open(os.path.join(tmpdir, "OZLed.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "OZObject.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            slabs_c = open(os.path.join(tmpdir, "oz_mem_slabs.c")).read()
            assert "oz_slab_Foo, sizeof(struct Foo), 3, 4)" in slabs_c
            # OZObject has 0 alloc calls -> minimum 1
            assert "oz_slab_OZObject, sizeof(struct OZObject), 1, 4)" in slabs_c


class TestARCAutoDealloc:
    def test_auto_dealloc_with_object_ivar(self):
        m = _module_with_obj_ivar()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder.c")).read()
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
            content = open(os.path.join(tmpdir, "OZObject.c")).read()
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
            content = open(os.path.join(tmpdir, "OZObject.c")).read()
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
            content = open(os.path.join(tmpdir, "Child.c")).read()
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
            content = open(os.path.join(tmpdir, "Holder.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Holder.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Foo.c")).read()
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
            content = open(os.path.join(tmpdir, "Holder.c")).read()
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
            content = open(os.path.join(tmpdir, "Holder.c")).read()
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
            content = open(os.path.join(tmpdir, "Holder.c")).read()
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
            content = open(os.path.join(tmpdir, "Holder.c")).read()
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
            content = open(os.path.join(tmpdir, "Holder.c")).read()
            assert "OZObject_release((struct OZObject *)f);" in content
            assert "f = ((void *)0);" in content

    def test_scope_exit_still_releases_after_reassign(self):
        m = self._method_with_body([
            self._decl_obj("f", self._alloc_msg()),
            self._assign("f", self._alloc_msg()),
        ])
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "Holder.c")).read()
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
            content = open(os.path.join(tmpdir, "Holder.c")).read()
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
            content = open(os.path.join(tmpdir, "Holder.c")).read()
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
            content = open(os.path.join(tmpdir, "Holder.c")).read()
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
            content = open(os.path.join(tmpdir, "oz_dispatch.h")).read()
            assert "extern const char *const oz_class_names[OZ_CLASS_COUNT]" in content

    def test_dispatch_header_has_superclass_table(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "oz_dispatch.h")).read()
            assert "extern const uint8_t oz_superclass_id[OZ_CLASS_COUNT]" in content

    def test_dispatch_header_has_inline_helpers(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "oz_dispatch.h")).read()
            assert "oz_name(" in content
            assert "oz_superclass(" in content
            assert "oz_isKindOfClass(" in content

    def test_dispatch_source_class_names(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "oz_dispatch.c")).read()
            assert 'oz_class_names[OZ_CLASS_COUNT]' in content
            assert '"OZObject"' in content
            assert '"OZLed"' in content

    def test_dispatch_source_superclass_ids(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "oz_dispatch.c")).read()
            assert "oz_superclass_id[OZ_CLASS_COUNT]" in content
            assert "[OZ_CLASS_OZObject] = OZ_CLASS_COUNT" in content
            assert "[OZ_CLASS_OZLed] = OZ_CLASS_OZObject" in content

    def test_root_class_isEqual(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            h = open(os.path.join(tmpdir, "OZObject.h")).read()
            c = open(os.path.join(tmpdir, "OZObject.c")).read()
            assert "OZObject_isEqual_" in h
            assert "OZObject_isEqual_" in c
            assert "self == anObject" in c

    def test_root_class_cDescription(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            h = open(os.path.join(tmpdir, "OZObject.h")).read()
            c = open(os.path.join(tmpdir, "OZObject.c")).read()
            assert "OZObject_cDescription_maxLength_" in h
            assert "OZObject_cDescription_maxLength_" in c
            assert "snprintk" in c
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
            content = open(os.path.join(tmpdir, "oz_dispatch.h")).read()
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
            content = open(os.path.join(tmpdir, "oz_dispatch.h")).read()
            assert "OZ_SEND_cDescription_maxLength_" in content

    def test_root_source_includes_printk(self):
        m = _simple_module()
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "OZObject.c")).read()
            assert "#include <zephyr/sys/printk.h>" in content


class TestStaticVarEmission:
    """Tests for file-scope static variable emission in oz_functions.c."""

    def test_static_var_emitted_in_functions_file(self):
        m = _simple_module()
        m.statics.append(OZStaticVar("_sharedConfig", OZType("AppConfig *")))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "oz_functions.c")).read()
            assert "struct AppConfig * _sharedConfig;" in content

    def test_primitive_static_var(self):
        m = _simple_module()
        m.statics.append(OZStaticVar("_count", OZType("int")))
        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            content = open(os.path.join(tmpdir, "oz_functions.c")).read()
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
            src = open(os.path.join(tmpdir, "OZLed.c")).read()
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
            src = open(os.path.join(tmpdir, "oz_functions.c")).read()
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
            src = open(os.path.join(tmpdir, "oz_functions.c")).read()
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
            src = open(os.path.join(tmpdir, "oz_functions.c")).read()
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
            src = open(os.path.join(tmpdir, "oz_functions.c")).read()
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
            src = open(os.path.join(tmpdir, "oz_functions.c")).read()
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
            src = open(os.path.join(tmpdir, "oz_functions.c")).read()
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
            src = open(os.path.join(tmpdir, "oz_functions.c")).read()
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
            src = open(os.path.join(tmpdir, "oz_functions.c")).read()
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
            assert any("sum" in d for d in m.diagnostics)

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
            hdr = open(os.path.join(tmpdir, "OZNumber.h")).read()
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
            src = open(os.path.join(tmpdir, "OZLed.c")).read()
            assert "OZArray_objectAtIndexedSubscript_" in src
