# SPDX-License-Identifier: Apache-2.0

import os
import tempfile

from oz_transpile.emit import emit, _selector_to_c, _base_chain, _method_prototype
from oz_transpile.model import (
    DispatchKind,
    OZClass,
    OZIvar,
    OZMethod,
    OZModule,
    OZParam,
    OZProtocol,
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
