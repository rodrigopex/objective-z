# SPDX-License-Identifier: Apache-2.0

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


class TestOZType:
    def test_void(self):
        t = OZType("void")
        assert t.is_void
        assert not t.is_object
        assert t.c_type == "void"

    def test_int(self):
        t = OZType("int")
        assert not t.is_void
        assert not t.is_object
        assert t.c_type == "int"

    def test_id(self):
        t = OZType("id")
        assert t.is_object
        assert t.c_type == "struct OZObject *"

    def test_instancetype(self):
        t = OZType("instancetype")
        assert t.is_object
        assert t.c_type == "struct OZObject *"

    def test_object_pointer(self):
        t = OZType("OZLed *")
        assert t.is_object
        assert t.c_type == "struct OZLed *"

    def test_strong_qualifier(self):
        t = OZType("OZLed *__strong")
        assert t.is_object
        assert t.c_type == "struct OZLed *"

    def test_c_pointer_not_object(self):
        t = OZType("char *")
        assert not t.is_object

    def test_bool(self):
        t = OZType("BOOL")
        assert not t.is_object
        assert t.c_type == "BOOL"

    def test_bool_pointer_not_object(self):
        t = OZType("BOOL *")
        assert not t.is_object

    def test_block_type(self):
        t = OZType("void (^)(id, unsigned int, BOOL *)")
        assert t.is_block
        assert not t.is_object
        assert t.c_type == "void (*)(struct OZObject *, unsigned int, BOOL *)"

    def test_block_c_param_decl(self):
        t = OZType("void (^)(id, unsigned int, BOOL *)")
        decl = t.c_param_decl("callback")
        assert decl == "void (*callback)(struct OZObject *, unsigned int, BOOL *)"

    def test_non_block_c_param_decl(self):
        t = OZType("int")
        assert t.c_param_decl("x") == "int x"

    def test_id_star_to_object_ptr_ptr(self):
        t = OZType("id *")
        assert t.c_type == "struct OZObject **"

    def test_protocol_qualified_id(self):
        t = OZType("id<IteratorProtocol>")
        assert t.is_object
        assert t.c_type == "struct OZObject *"

    def test_protocol_qualified_id_strong(self):
        t = OZType("__strong id<IteratorProtocol>")
        assert t.is_object
        assert t.c_type == "struct OZObject *"

    def test_unsafe_unretained_id(self):
        t = OZType("__unsafe_unretained id")
        assert t.is_object
        assert t.is_unretained
        assert t.c_type == "struct OZObject *"

    def test_unsafe_unretained_class_pointer(self):
        t = OZType("__unsafe_unretained OZObject *")
        assert t.is_object
        assert t.is_unretained
        assert t.c_type == "struct OZObject *"

    def test_strong_id_not_unretained(self):
        t = OZType("id")
        assert t.is_object
        assert not t.is_unretained

    def test_strong_qualified_not_unretained(self):
        t = OZType("__strong OZLed *")
        assert t.is_object
        assert not t.is_unretained

    def test_generic_params_empty_for_plain_type(self):
        assert OZType("int").generic_params == []
        assert OZType("OZObject *").generic_params == []
        assert OZType("id").generic_params == []

    def test_generic_params_single(self):
        t = OZType("OZArray<OZQ31 *> *")
        assert t.generic_params == ["OZQ31 *"]

    def test_generic_params_protocol_qualified(self):
        t = OZType("OZArray<id<PXDataProcessor>> *")
        assert t.generic_params == ["id<PXDataProcessor>"]

    def test_generic_params_two_params(self):
        t = OZType("OZDictionary<OZString *, OZQ31 *> *")
        assert t.generic_params == ["OZString *", "OZQ31 *"]

    def test_generic_params_dict_with_protocol(self):
        t = OZType("OZDictionary<OZString *, id<Foo>> *")
        assert t.generic_params == ["OZString *", "id<Foo>"]

    def test_generic_params_plain_id(self):
        t = OZType("OZArray<id> *")
        assert t.generic_params == ["id"]

    def test_generic_params_nested(self):
        t = OZType("OZArray<OZArray<OZQ31 *> *> *")
        assert t.generic_params == ["OZArray<OZQ31 *> *"]

    def test_generic_params_strips_qualifiers(self):
        t = OZType("__strong OZArray<OZString *> *")
        assert t.generic_params == ["OZString *"]


class TestOZParam:
    def test_construction(self):
        p = OZParam("pin", OZType("int"))
        assert p.name == "pin"
        assert p.oz_type.c_type == "int"


class TestOZIvar:
    def test_construction(self):
        iv = OZIvar("_pin", OZType("int"))
        assert iv.name == "_pin"


class TestOZMethod:
    def test_defaults(self):
        m = OZMethod("init", OZType("instancetype"))
        assert not m.is_class_method
        assert m.dispatch == DispatchKind.STATIC
        assert m.body_ast is None
        assert m.params == []

    def test_class_method(self):
        m = OZMethod("alloc", OZType("instancetype"), is_class_method=True)
        assert m.is_class_method


class TestOZProtocol:
    def test_construction(self):
        p = OZProtocol("OZToggleable", [
            OZMethod("toggle", OZType("void")),
        ])
        assert p.name == "OZToggleable"
        assert len(p.methods) == 1


class TestOZClass:
    def test_defaults(self):
        c = OZClass("OZLed")
        assert c.superclass is None
        assert c.class_id == -1
        assert c.base_depth == 0
        assert c.ivars == []
        assert c.methods == []
        assert c.protocols == []

    def test_with_superclass(self):
        c = OZClass("OZLed", superclass="OZObject")
        assert c.superclass == "OZObject"


class TestOZModule:
    def test_defaults(self):
        m = OZModule()
        assert m.classes == {}
        assert m.protocols == {}
        assert m.diagnostics == []

    def test_add_class(self):
        m = OZModule()
        m.classes["OZLed"] = OZClass("OZLed")
        assert "OZLed" in m.classes
