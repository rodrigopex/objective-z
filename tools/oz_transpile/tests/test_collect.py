# SPDX-License-Identifier: Apache-2.0

from oz_transpile.collect import collect


def _make_ast(*inner):
    """Wrap nodes in a TranslationUnitDecl."""
    return {"kind": "TranslationUnitDecl", "inner": list(inner)}


def _interface(name, super_name=None, ivars=None, protocols=None, inner=None):
    node = {"kind": "ObjCInterfaceDecl", "name": name, "inner": []}
    if super_name:
        node["super"] = {"name": super_name}
    for iv_name, iv_type in (ivars or []):
        node["inner"].append({
            "kind": "ObjCIvarDecl",
            "name": iv_name,
            "type": {"qualType": iv_type},
        })
    for proto in (protocols or []):
        node["inner"].append({"kind": "ObjCProtocol", "name": proto})
    if inner:
        node["inner"].extend(inner)
    return node


def _impl(name, super_name=None, methods=None, ivars=None):
    node = {"kind": "ObjCImplementationDecl", "name": name, "inner": []}
    if super_name:
        node["super"] = {"name": super_name}
    for iv_name, iv_type in (ivars or []):
        node["inner"].append({
            "kind": "ObjCIvarDecl",
            "name": iv_name,
            "type": {"qualType": iv_type},
        })
    for m in (methods or []):
        node["inner"].append(m)
    return node


def _method(selector, ret="void", params=None, is_class=False, body=None):
    node = {
        "kind": "ObjCMethodDecl",
        "name": selector,
        "returnType": {"qualType": ret},
        "inner": [],
    }
    if is_class:
        node["instance"] = False
    for pname, ptype in (params or []):
        node["inner"].append({
            "kind": "ParmVarDecl",
            "name": pname,
            "type": {"qualType": ptype},
        })
    if body is not None:
        node["inner"].append(body)
    return node


def _compound(*stmts):
    return {"kind": "CompoundStmt", "inner": list(stmts)}


class TestCollectInterface:
    def test_basic_class(self):
        ast = _make_ast(
            _interface("OZObject"),
            _interface("OZLed", "OZObject", [("_pin", "int")]),
        )
        mod = collect(ast)
        assert "OZObject" in mod.classes
        assert "OZLed" in mod.classes
        assert mod.classes["OZLed"].superclass == "OZObject"
        assert len(mod.classes["OZLed"].ivars) == 1
        assert mod.classes["OZLed"].ivars[0].name == "_pin"
        assert mod.classes["OZLed"].ivars[0].oz_type.c_type == "int"

    def test_protocols(self):
        ast = _make_ast(
            _interface("OZLed", "OZObject", protocols=["OZToggleable"]),
        )
        mod = collect(ast)
        assert mod.classes["OZLed"].protocols == ["OZToggleable"]

    def test_no_superclass(self):
        ast = _make_ast(_interface("OZObject"))
        mod = collect(ast)
        assert mod.classes["OZObject"].superclass is None


class TestCollectImplementation:
    def test_methods(self):
        body = _compound({"kind": "ReturnStmt", "inner": []})
        ast = _make_ast(
            _impl("OZLed", "OZObject", methods=[
                _method("init", ret="instancetype", body=body),
                _method("turnOn", ret="void"),
            ]),
        )
        mod = collect(ast)
        cls = mod.classes["OZLed"]
        assert len(cls.methods) == 2
        assert cls.methods[0].selector == "init"
        assert cls.methods[0].return_type.is_object
        assert cls.methods[0].body_ast is not None
        assert cls.methods[1].selector == "turnOn"

    def test_class_method(self):
        ast = _make_ast(
            _impl("OZLed", methods=[
                _method("alloc", ret="instancetype", is_class=True),
            ]),
        )
        mod = collect(ast)
        assert mod.classes["OZLed"].methods[0].is_class_method

    def test_method_params(self):
        ast = _make_ast(
            _impl("OZLed", methods=[
                _method("setPin:", ret="void", params=[("pin", "int")]),
            ]),
        )
        mod = collect(ast)
        m = mod.classes["OZLed"].methods[0]
        assert m.selector == "setPin:"
        assert len(m.params) == 1
        assert m.params[0].name == "pin"
        assert m.params[0].oz_type.c_type == "int"

    def test_interface_then_impl_merge(self):
        ast = _make_ast(
            _interface("OZLed", "OZObject", [("_pin", "int")]),
            _impl("OZLed", methods=[
                _method("init", ret="instancetype"),
            ]),
        )
        mod = collect(ast)
        cls = mod.classes["OZLed"]
        assert cls.superclass == "OZObject"
        assert len(cls.ivars) == 1
        assert len(cls.methods) == 1


class TestCollectProtocol:
    def test_protocol(self):
        ast = _make_ast({
            "kind": "ObjCProtocolDecl",
            "name": "OZToggleable",
            "inner": [
                _method("toggle", ret="void"),
            ],
        })
        mod = collect(ast)
        assert "OZToggleable" in mod.protocols
        proto = mod.protocols["OZToggleable"]
        assert len(proto.methods) == 1
        assert proto.methods[0].selector == "toggle"


class TestCollectCategory:
    def test_category_merges(self):
        ast = _make_ast(
            _interface("OZLed", "OZObject"),
            {
                "kind": "ObjCCategoryDecl",
                "interface": {"name": "OZLed"},
                "inner": [
                    _method("blink", ret="void"),
                ],
            },
        )
        mod = collect(ast)
        assert len(mod.classes["OZLed"].methods) == 1
        assert mod.classes["OZLed"].methods[0].selector == "blink"

    def test_category_creates_class(self):
        ast = _make_ast({
            "kind": "ObjCCategoryDecl",
            "interface": {"name": "OZFoo"},
            "inner": [
                _method("bar", ret="void"),
            ],
        })
        mod = collect(ast)
        assert "OZFoo" in mod.classes
        assert mod.classes["OZFoo"].methods[0].selector == "bar"


class TestCollectCategoryImpl:
    def test_category_impl_merges_with_body(self):
        ast = _make_ast(
            _interface("Car", "OZObject"),
            {
                "kind": "ObjCCategoryImplDecl",
                "interface": {"name": "Car"},
                "inner": [
                    _method("milage", ret="int",
                            body=_compound({"kind": "ReturnStmt",
                                            "inner": [{"kind": "IntegerLiteral",
                                                        "value": "100"}]})),
                ],
            },
        )
        mod = collect(ast)
        assert len(mod.classes["Car"].methods) == 1
        m = mod.classes["Car"].methods[0]
        assert m.selector == "milage"
        assert m.body_ast is not None

    def test_category_impl_creates_class(self):
        ast = _make_ast({
            "kind": "ObjCCategoryImplDecl",
            "interface": {"name": "Bike"},
            "inner": [
                _method("speed", ret="int"),
            ],
        })
        mod = collect(ast)
        assert "Bike" in mod.classes
        assert mod.classes["Bike"].methods[0].selector == "speed"


class TestCollectStaticVar:
    def test_static_var_collected(self):
        ast = _make_ast({
            "kind": "VarDecl",
            "name": "_sharedConfig",
            "storageClass": "static",
            "type": {"qualType": "AppConfig *"},
        })
        mod = collect(ast)
        assert len(mod.statics) == 1
        assert mod.statics[0].name == "_sharedConfig"
        assert mod.statics[0].oz_type.c_type == "struct AppConfig *"

    def test_non_static_var_skipped(self):
        ast = _make_ast({
            "kind": "VarDecl",
            "name": "globalVar",
            "type": {"qualType": "int"},
        })
        mod = collect(ast)
        assert len(mod.statics) == 0

    def test_extern_var_skipped(self):
        ast = _make_ast({
            "kind": "VarDecl",
            "name": "externalVar",
            "storageClass": "extern",
            "type": {"qualType": "int"},
        })
        mod = collect(ast)
        assert len(mod.statics) == 0

    def test_included_static_skipped(self):
        ast = _make_ast({
            "kind": "VarDecl",
            "name": "_headerStatic",
            "storageClass": "static",
            "type": {"qualType": "int"},
            "loc": {"includedFrom": {"file": "some_header.h"}},
        })
        mod = collect(ast)
        assert len(mod.statics) == 0

    def test_static_with_gnu_null_init(self):
        """OZ-004: static PXAppConfig *_shared = nil; with GNUNullExpr."""
        ast = _make_ast({
            "kind": "VarDecl",
            "name": "_shared",
            "storageClass": "static",
            "type": {"qualType": "PXAppConfig *"},
            "inner": [{"kind": "GNUNullExpr"}],
        })
        mod = collect(ast)
        assert len(mod.statics) == 1
        assert mod.statics[0].name == "_shared"
        assert mod.statics[0].init_value == "NULL"
        assert mod.statics[0].oz_type.c_type == "struct PXAppConfig *"

    def test_static_with_null_to_pointer_cast(self):
        """OZ-004: nil as ImplicitCastExpr with NullToPointer."""
        ast = _make_ast({
            "kind": "VarDecl",
            "name": "_shared",
            "storageClass": "static",
            "type": {"qualType": "PXAppConfig *"},
            "inner": [{
                "kind": "ImplicitCastExpr",
                "castKind": "NullToPointer",
                "inner": [{"kind": "IntegerLiteral", "value": "0"}],
            }],
        })
        mod = collect(ast)
        assert len(mod.statics) == 1
        assert mod.statics[0].init_value == "NULL"

    def test_static_with_cstyle_null_cast(self):
        """OZ-004: nil as CStyleCastExpr with NullToPointer."""
        ast = _make_ast({
            "kind": "VarDecl",
            "name": "_shared",
            "storageClass": "static",
            "type": {"qualType": "PXAppConfig *"},
            "inner": [{
                "kind": "CStyleCastExpr",
                "castKind": "NullToPointer",
                "inner": [{"kind": "IntegerLiteral", "value": "0"}],
            }],
        })
        mod = collect(ast)
        assert len(mod.statics) == 1
        assert mod.statics[0].init_value == "NULL"

    def test_static_objc_type_unsupported_init_warns(self):
        """OZ-006: diagnostic when ObjC-typed static has unsupported init."""
        ast = _make_ast({
            "kind": "VarDecl",
            "name": "_shared",
            "storageClass": "static",
            "type": {"qualType": "PXAppConfig *"},
            "inner": [{
                "kind": "CallExpr",
                "type": {"qualType": "PXAppConfig *"},
            }],
        })
        mod = collect(ast)
        assert len(mod.statics) == 0
        assert any("_shared" in d and "unsupported initializer" in d
                    for d in mod.diagnostics)


class TestCollectTypeDefs:
    def test_enum_from_oz_transpile(self):
        ast = _make_ast(
            {
                "kind": "EnumDecl",
                "name": "oz_number_tag",
                "loc": {
                    "file": "/path/oz_transpile/OZNumber.h",
                    "includedFrom": {"file": "/path/OZNumber.m"},
                },
                "inner": [
                    {"kind": "EnumConstantDecl", "name": "OZ_NUM_INT32"},
                    {"kind": "EnumConstantDecl", "name": "OZ_NUM_UINT32"},
                    {"kind": "EnumConstantDecl", "name": "OZ_NUM_FLOAT"},
                ],
            },
        )
        mod = collect(ast)
        assert "enum oz_number_tag" in mod.type_defs
        defn = mod.type_defs["enum oz_number_tag"]
        assert "OZ_NUM_INT32 = 0," in defn
        assert "OZ_NUM_UINT32," in defn
        assert "OZ_NUM_FLOAT," in defn

    def test_union_from_oz_transpile(self):
        ast = _make_ast(
            {
                "kind": "RecordDecl",
                "name": "oz_number_value",
                "tagUsed": "union",
                "completeDefinition": True,
                "loc": {
                    "includedFrom": {"file": "/path/oz_transpile/OZNumber.m"},
                },
                "inner": [
                    {"kind": "FieldDecl", "name": "i32",
                     "type": {"qualType": "int"}},
                    {"kind": "FieldDecl", "name": "f32",
                     "type": {"qualType": "float"}},
                ],
            },
        )
        mod = collect(ast)
        assert "union oz_number_value" in mod.type_defs
        defn = mod.type_defs["union oz_number_value"]
        assert "int i32;" in defn
        assert "float f32;" in defn

    def test_non_oz_transpile_enum_ignored(self):
        ast = _make_ast(
            {
                "kind": "EnumDecl",
                "name": "SomeEnum",
                "loc": {"file": "/usr/include/something.h"},
                "inner": [
                    {"kind": "EnumConstantDecl", "name": "VAL1"},
                ],
            },
        )
        mod = collect(ast)
        assert len(mod.type_defs) == 0

    def test_unnamed_enum_ignored(self):
        ast = _make_ast(
            {
                "kind": "EnumDecl",
                "name": "",
                "loc": {"file": "/path/oz_transpile/OZNumber.h"},
                "inner": [
                    {"kind": "EnumConstantDecl", "name": "VAL1"},
                ],
            },
        )
        mod = collect(ast)
        assert len(mod.type_defs) == 0


class TestCollectVerbatimLines:
    def test_k_thread_define_collected(self, tmp_path):
        src = tmp_path / "main.m"
        src.write_text(
            '#import "OZObject.h"\n'
            "@interface Foo: OZObject\n@end\n"
            "K_THREAD_DEFINE(my_thread, 1024, entry, NULL, NULL, NULL, 7, 0, 0);\n"
        )
        ast = _make_ast(
            _interface("Foo", "OZObject"),
            {"kind": "VarDecl", "name": "dummy",
             "loc": {"file": str(src)},
             "type": {"qualType": "int"}},
        )
        mod = collect(ast)
        assert len(mod.verbatim_lines) == 1
        assert "K_THREAD_DEFINE" in mod.verbatim_lines[0]
        assert "my_thread" in mod.verbatim_lines[0]

    def test_multiline_macro_collected(self, tmp_path):
        src = tmp_path / "main.m"
        src.write_text(
            "@interface Foo: OZObject\n@end\n"
            "K_THREAD_DEFINE(my_thread, 1024, entry,\n"
            "\t\tNULL, NULL, NULL, 7, 0, 0);\n"
        )
        ast = _make_ast(
            _interface("Foo", "OZObject"),
            {"kind": "VarDecl", "name": "dummy",
             "loc": {"file": str(src)},
             "type": {"qualType": "int"}},
        )
        mod = collect(ast)
        assert len(mod.verbatim_lines) == 1
        assert "K_THREAD_DEFINE" in mod.verbatim_lines[0]
        assert "NULL, NULL, NULL" in mod.verbatim_lines[0]

    def test_no_macros_no_verbatim(self, tmp_path):
        src = tmp_path / "main.m"
        src.write_text(
            '@import "OZObject.h"\n'
            "@interface Foo: OZObject\n@end\n"
            "int main(void) { return 0; }\n"
        )
        ast = _make_ast(
            _interface("Foo", "OZObject"),
            {"kind": "VarDecl", "name": "dummy",
             "loc": {"file": str(src)},
             "type": {"qualType": "int"}},
        )
        mod = collect(ast)
        assert len(mod.verbatim_lines) == 0

    def test_no_source_file_graceful(self):
        ast = _make_ast(_interface("Foo", "OZObject"))
        mod = collect(ast)
        assert len(mod.verbatim_lines) == 0


# -- Helpers for property AST nodes --

def _property_decl(name, qual_type, **attrs):
    node = {
        "kind": "ObjCPropertyDecl",
        "name": name,
        "type": {"qualType": qual_type},
    }
    node.update(attrs)
    return node


def _property_impl(name, ivar_name, qual_type):
    return {
        "kind": "ObjCPropertyImplDecl",
        "name": name,
        "implKind": "synthesize",
        "propertyDecl": {"kind": "ObjCPropertyDecl", "name": name},
        "ivarDecl": {
            "kind": "ObjCIvarDecl",
            "name": ivar_name,
            "type": {"qualType": qual_type},
        },
    }


class TestCollectProperty:
    def test_readonly_nonatomic(self):
        ast = _make_ast(
            _interface("Car", "OZObject", inner=[
                _property_decl("color", "struct color *",
                               readonly=True, nonatomic=True),
            ]),
        )
        mod = collect(ast)
        props = mod.classes["Car"].properties
        assert len(props) == 1
        assert props[0].name == "color"
        assert props[0].is_readonly is True
        assert props[0].is_nonatomic is True

    def test_strong_property(self):
        ast = _make_ast(
            _interface("Car", "OZObject", inner=[
                _property_decl("model", "OZString *",
                               readwrite=True, nonatomic=True, strong=True),
            ]),
        )
        mod = collect(ast)
        prop = mod.classes["Car"].properties[0]
        assert prop.ownership == "strong"
        assert prop.is_readonly is False

    def test_unsafe_unretained_property(self):
        ast = _make_ast(
            _interface("Car", "OZObject", inner=[
                _property_decl("delegate", "id",
                               assign=True, unsafe_unretained=True,
                               nonatomic=True),
            ]),
        )
        mod = collect(ast)
        prop = mod.classes["Car"].properties[0]
        assert prop.ownership == "unsafe_unretained"

    def test_assign_property(self):
        ast = _make_ast(
            _interface("Car", "OZObject", inner=[
                _property_decl("speed", "int",
                               assign=True, unsafe_unretained=True,
                               nonatomic=True),
            ]),
        )
        mod = collect(ast)
        prop = mod.classes["Car"].properties[0]
        assert prop.ownership == "unsafe_unretained"

    def test_atomic_is_default(self):
        ast = _make_ast(
            _interface("Car", "OZObject", inner=[
                _property_decl("model", "OZString *",
                               atomic=True, strong=True),
            ]),
        )
        mod = collect(ast)
        prop = mod.classes["Car"].properties[0]
        assert prop.is_nonatomic is False

    def test_custom_getter(self):
        ast = _make_ast(
            _interface("Car", "OZObject", inner=[
                _property_decl("enabled", "BOOL",
                               getter={"kind": "ObjCMethodDecl",
                                       "name": "isEnabled"},
                               assign=True, nonatomic=True),
            ]),
        )
        mod = collect(ast)
        prop = mod.classes["Car"].properties[0]
        assert prop.getter_sel == "isEnabled"

    def test_custom_setter(self):
        ast = _make_ast(
            _interface("Car", "OZObject", inner=[
                _property_decl("speed", "int",
                               setter={"kind": "ObjCMethodDecl",
                                       "name": "setCustomSpeed:"},
                               assign=True, nonatomic=True),
            ]),
        )
        mod = collect(ast)
        prop = mod.classes["Car"].properties[0]
        assert prop.setter_sel == "setCustomSpeed:"

    def test_property_impl_links_ivar(self):
        ast = _make_ast(
            _interface("Car", "OZObject", inner=[
                _property_decl("color", "struct color *",
                               readonly=True, nonatomic=True),
            ]),
            _impl("Car", "OZObject", methods=[
                _property_impl("color", "_color", "struct color *"),
            ]),
        )
        mod = collect(ast)
        prop = mod.classes["Car"].properties[0]
        assert prop.ivar_name == "_color"

    def test_property_impl_custom_ivar_name(self):
        ast = _make_ast(
            _interface("Car", "OZObject", inner=[
                _property_decl("ackCount", "int",
                               assign=True, nonatomic=True),
            ]),
            _impl("Car", "OZObject", methods=[
                _property_impl("ackCount", "_count", "int"),
            ]),
        )
        mod = collect(ast)
        prop = mod.classes["Car"].properties[0]
        assert prop.ivar_name == "_count"

    def test_weak_property_raises_error(self):
        ast = _make_ast(
            _interface("Car", "OZObject", inner=[
                _property_decl("delegate", "id",
                               weak=True, nonatomic=True),
            ]),
        )
        mod = collect(ast)
        assert len(mod.classes["Car"].properties) == 0
        assert any("weak" in e and "delegate" in e for e in mod.errors)
