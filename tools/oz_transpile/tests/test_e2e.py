# SPDX-License-Identifier: Apache-2.0

import os
import subprocess
import tempfile

import pytest

from oz_transpile.__main__ import main

FIXTURE_DIR = os.path.join(os.path.dirname(__file__), "fixtures")
PAL_INCLUDE_DIR = os.path.join(os.path.dirname(__file__), "..", "..", "..", "include")
OZ_SDK_DIR = os.path.join(PAL_INCLUDE_DIR, "oz_sdk")


class TestCLI:
    def test_basic_pipeline(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            rc = main(["--input", ast_file, "--outdir", tmpdir, "--verbose"])
            assert rc == 0

            foundation_dir = os.path.join(tmpdir, "Foundation")
            foundation_files = set(os.listdir(foundation_dir))
            user_files = set(os.listdir(tmpdir)) - {"Foundation"}

            assert {"oz_dispatch.h", "oz_dispatch.c",
                    "OZObject_ozh.h", "OZObject_ozm.c"}.issubset(foundation_files)
            assert {"OZLed_ozh.h", "OZLed_ozm.c"}.issubset(user_files)

    def test_dispatch_header_content(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            content = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            # toggle is in protocol -> should have const vtable
            assert "OZ_PROTOCOL_SEND_toggle" in content
            # init is overridden -> protocol dispatch
            assert "OZ_PROTOCOL_SEND_init" in content
            # compile-time dispatch macros
            assert "OZ_IMPL_" in content
            assert "OZ_SEND(cls, sel, self, ...)" in content
            # turnOn is unique -> no vtable entry
            assert "OZ_PROTOCOL_SEND_turnOn" not in content

    def test_class_struct_hierarchy(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            led_h = open(os.path.join(tmpdir, "OZLed_ozh.h")).read()
            assert "struct OZObject base;" in led_h
            assert "int _pin;" in led_h
            assert "BOOL _state;" in led_h

    def test_super_call(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            led_c = open(os.path.join(tmpdir, "OZLed_ozm.c")).read()
            assert "OZObject_init((struct OZObject *)self)" in led_c

    def test_pool_sizes(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir,
                  "--pool-sizes", "OZLed=8"])
            led_h = open(os.path.join(tmpdir, "OZLed_ozh.h")).read()
            led_c = open(os.path.join(tmpdir, "OZLed_ozm.c")).read()
            assert "oz_slab_OZLed" in led_h
            assert "OZ_SLAB_DEFINE(oz_slab_OZLed" in led_c
            assert "8" in led_c

    def test_retain_release_in_root(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            root_c = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZObject_retain" in root_c
            assert "OZObject_release" in root_c
            assert "oz_atomic_dec_and_test" in root_c

    def test_diagnostics_not_emitted_for_valid_input(self):
        """Valid input should produce no 'not found' diagnostics."""
        import io
        import contextlib
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            stderr_buf = io.StringIO()
            with contextlib.redirect_stderr(stderr_buf):
                rc = main(["--input", ast_file, "--outdir", tmpdir])
            assert rc == 0
            assert "not found" not in stderr_buf.getvalue()


class TestSynchronizedE2E:
    """End-to-end test: .m -> Clang AST JSON -> transpiler -> C."""

    def test_synchronized_pipeline(self):
        ast_file = os.path.join(FIXTURE_DIR, "synchronized_sample.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            rc = main(["--input", ast_file, "--outdir", tmpdir, "--verbose"])
            assert rc == 0

            counter_c = open(os.path.join(tmpdir, "Counter_ozm.c")).read()
            assert "OZLock_initWithObject(OZLock_alloc()" in counter_c
            assert "OZObject_release((struct OZObject *)_sync)" in counter_c

            dispatch_h = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            assert "OZ_CLASS_OZLock" in dispatch_h

    def test_synchronized_counter_resets_per_method(self):
        """Both increment and getCount should use _sync (not _sync2)."""
        ast_file = os.path.join(FIXTURE_DIR, "synchronized_sample.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            counter_c = open(os.path.join(tmpdir, "Counter_ozm.c")).read()
            assert counter_c.count("struct OZLock *_sync =") == 2
            assert "_sync2" not in counter_c


def _gcc_syntax_check(tmpdir: str) -> None:
    """Run gcc -fsyntax-only on all .c files in tmpdir (recursive)."""
    foundation_dir = os.path.join(tmpdir, "Foundation")
    c_files = []
    for dirpath, _, filenames in os.walk(tmpdir):
        for f in filenames:
            if f.endswith(".c"):
                c_files.append(os.path.join(dirpath, f))
    for c_file in c_files:
        result = subprocess.run(
            ["gcc", "-fsyntax-only", "-Wall", "-Werror",
             "-DOZ_PLATFORM_HOST",
             "-I", tmpdir,
             "-I", foundation_dir,
             "-I", PAL_INCLUDE_DIR,
             c_file],
            capture_output=True, text=True,
        )
        assert result.returncode == 0, (
            f"{c_file} failed:\n{result.stderr}"
        )


@pytest.mark.skipif(
    subprocess.run(["gcc", "--version"], capture_output=True).returncode != 0,
    reason="gcc not available",
)
class TestGCCSyntax:
    """Verify generated C passes gcc -fsyntax-only with PAL host headers."""

    def test_generated_c_compiles(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            _gcc_syntax_check(tmpdir)

    def test_arc_with_object_ivars_compiles(self):
        """Full pipeline with ARC: object ivars, dealloc chain, local releases."""
        from oz_transpile.emit import emit
        from oz_transpile.model import OZClass, OZIvar, OZMethod, OZModule, OZParam, OZType
        from oz_transpile.resolve import resolve

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
        m.classes["Helper"] = OZClass("Helper", superclass="OZObject",
            ivars=[OZIvar("_peer", OZType("OZObject *"))],
            methods=[
                OZMethod("init", OZType("instancetype"), body_ast={
                    "kind": "CompoundStmt",
                    "inner": [{"kind": "ReturnStmt", "inner": [
                        {"kind": "ObjCMessageExpr", "selector": "init",
                         "receiverKind": "super (instance)", "inner": [],
                         "type": {"qualType": "OZObject *"}},
                    ]}],
                }),
                OZMethod("setPeer:", OZType("void"),
                         params=[OZParam("peer", OZType("OZObject *"))],
                         body_ast={
                    "kind": "CompoundStmt",
                    "inner": [{
                        "kind": "BinaryOperator", "opcode": "=",
                        "inner": [
                            {"kind": "ObjCIvarRefExpr", "decl": {"name": "_peer"}},
                            {"kind": "DeclRefExpr",
                             "referencedDecl": {"name": "peer"},
                             "type": {"qualType": "OZObject *"}},
                        ],
                    }],
                }),
            ],
        )
        resolve(m)

        with tempfile.TemporaryDirectory() as tmpdir:
            emit(m, tmpdir)
            _gcc_syntax_check(tmpdir)


class TestMultiFileTranspilation:
    """E2E tests for multi-file transpilation with --input taking multiple ASTs."""

    def test_cross_file_class_reference(self):
        """Class in file A, subclass in file B — merged correctly."""
        base_ast = os.path.join(FIXTURE_DIR, "multi_base.ast.json")
        sub_ast = os.path.join(FIXTURE_DIR, "multi_sub.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            rc = main(["--input", base_ast, sub_ast, "--outdir", tmpdir])
            assert rc == 0

            # Vehicle defined in base, Car in sub — both should be emitted
            all_files = set()
            for dirpath, _, filenames in os.walk(tmpdir):
                all_files.update(filenames)

            assert "Vehicle_ozh.h" in all_files
            assert "Vehicle_ozm.c" in all_files
            assert "Car_ozh.h" in all_files
            assert "Car_ozm.c" in all_files

    def test_cross_file_inheritance_hierarchy(self):
        """Car (file B) extends Vehicle (file A) — struct nesting correct."""
        base_ast = os.path.join(FIXTURE_DIR, "multi_base.ast.json")
        sub_ast = os.path.join(FIXTURE_DIR, "multi_sub.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", base_ast, sub_ast, "--outdir", tmpdir])

            car_h = open(os.path.join(tmpdir, "Car_ozh.h")).read()
            assert "struct Vehicle base;" in car_h
            assert "int _doors;" in car_h

            vehicle_h = open(os.path.join(tmpdir, "Vehicle_ozh.h")).read()
            assert "struct OZObject base;" in vehicle_h
            assert "int _speed;" in vehicle_h

    def test_cross_file_super_call(self):
        """Car.init calls [super init] which resolves to Vehicle_init."""
        base_ast = os.path.join(FIXTURE_DIR, "multi_base.ast.json")
        sub_ast = os.path.join(FIXTURE_DIR, "multi_sub.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", base_ast, sub_ast, "--outdir", tmpdir])

            car_c = open(os.path.join(tmpdir, "Car_ozm.c")).read()
            assert "Vehicle_init" in car_c

    def test_category_in_separate_file(self):
        """Category in separate AST merges methods into base class."""
        base_ast = os.path.join(FIXTURE_DIR, "multi_base.ast.json")
        cat_ast = os.path.join(FIXTURE_DIR, "multi_category.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            rc = main(["--input", base_ast, cat_ast, "--outdir", tmpdir])
            assert rc == 0

            vehicle_c = open(os.path.join(tmpdir, "Vehicle_ozm.c")).read()
            assert "Vehicle_honk" in vehicle_c

    def test_dispatch_header_includes_all_classes(self):
        """Dispatch header should have class IDs for all merged classes."""
        base_ast = os.path.join(FIXTURE_DIR, "multi_base.ast.json")
        sub_ast = os.path.join(FIXTURE_DIR, "multi_sub.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", base_ast, sub_ast, "--outdir", tmpdir])

            dispatch_h = open(os.path.join(tmpdir, "Foundation",
                                           "oz_dispatch.h")).read()
            assert "OZ_CLASS_OZObject" in dispatch_h
            assert "OZ_CLASS_Vehicle" in dispatch_h
            assert "OZ_CLASS_Car" in dispatch_h

    @pytest.mark.skipif(
        subprocess.run(["gcc", "--version"], capture_output=True).returncode != 0,
        reason="gcc not available",
    )
    def test_multi_file_generated_c_compiles(self):
        """GCC syntax check on multi-file output."""
        base_ast = os.path.join(FIXTURE_DIR, "multi_base.ast.json")
        sub_ast = os.path.join(FIXTURE_DIR, "multi_sub.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", base_ast, sub_ast, "--outdir", tmpdir])
            _gcc_syntax_check(tmpdir)


def _compile_and_run(source, extra_flags=None):
    """Compile ObjC source via Clang and run through the transpiler CLI."""
    with tempfile.TemporaryDirectory() as tmpdir:
        src_path = os.path.join(tmpdir, "source.m")
        ast_path = os.path.join(tmpdir, "source.ast.json")
        with open(src_path, "w") as f:
            f.write(source)
        result = subprocess.run(
            ["clang", "-Xclang", "-ast-dump=json", "-fsyntax-only",
             "-fobjc-runtime=macosx", "-I", OZ_SDK_DIR, src_path],
            capture_output=True,
        )
        if result.returncode != 0:
            raise RuntimeError(f"clang failed:\n{result.stderr.decode()}")
        with open(ast_path, "wb") as f:
            f.write(result.stdout)
        argv = ["--input", ast_path, "--outdir", tmpdir] + (extra_flags or [])
        return main(argv)


class TestCLIErrors:
    def test_resolve_error_returns_exit_1(self):
        """Inheritance cycle — corner case: Clang rejects this."""
        import json
        cycle_ast = {
            "kind": "TranslationUnitDecl",
            "inner": [
                {"kind": "ObjCInterfaceDecl", "name": "A",
                 "super": {"name": "B"}, "inner": []},
                {"kind": "ObjCInterfaceDecl", "name": "B",
                 "super": {"name": "A"}, "inner": []},
                {"kind": "ObjCImplementationDecl", "name": "A",
                 "super": {"name": "B"}, "inner": []},
                {"kind": "ObjCImplementationDecl", "name": "B",
                 "super": {"name": "A"}, "inner": []},
            ],
        }
        with tempfile.TemporaryDirectory() as tmpdir:
            ast_file = os.path.join(tmpdir, "cycle.ast.json")
            with open(ast_file, "w") as f:
                json.dump(cycle_ast, f)
            rc = main(["--input", ast_file, "--outdir", tmpdir])
            assert rc == 1

    def test_strict_with_diagnostics_returns_exit_1(self):
        """--strict should fail when diagnostics are present.
        Corner case: unsupported static initializer pattern."""
        import json
        ast = {
            "kind": "TranslationUnitDecl",
            "inner": [
                {"kind": "ObjCInterfaceDecl", "name": "OZObject", "inner": []},
                {"kind": "ObjCImplementationDecl", "name": "OZObject",
                 "inner": []},
                {"kind": "VarDecl", "name": "_shared",
                 "storageClass": "static",
                 "type": {"qualType": "OZObject *"},
                 "inner": [{"kind": "CallExpr",
                            "type": {"qualType": "OZObject *"}}]},
            ],
        }
        with tempfile.TemporaryDirectory() as tmpdir:
            ast_file = os.path.join(tmpdir, "diag.ast.json")
            with open(ast_file, "w") as f:
                json.dump(ast, f)
            rc = main(["--input", ast_file, "--outdir", tmpdir, "--strict"])
            assert rc == 1

    def test_manifest_written(self):
        """--manifest should write generated file paths."""
        with tempfile.TemporaryDirectory() as tmpdir:
            src_path = os.path.join(tmpdir, "led.m")
            ast_path = os.path.join(tmpdir, "led.ast.json")
            with open(src_path, "w") as f:
                f.write("""\
#import <Foundation/OZObject.h>
@protocol OZToggleable
- (void)toggle;
@end
@interface OZLed : OZObject <OZToggleable> {
        int _pin;
        BOOL _state;
}
- (void)toggle;
@end
@implementation OZLed
- (void)toggle { _state = !_state; }
@end
""")
            result = subprocess.run(
                ["clang", "-Xclang", "-ast-dump=json", "-fsyntax-only",
                 "-I", OZ_SDK_DIR, src_path],
                capture_output=True,
            )
            assert result.returncode == 0
            with open(ast_path, "wb") as f:
                f.write(result.stdout)
            manifest = os.path.join(tmpdir, "manifest.txt")
            rc = main(["--input", ast_path, "--outdir", tmpdir,
                        "--manifest", manifest])
            assert rc == 0
            content = open(manifest).read()
            assert "OZLed_ozm.c" in content
            assert "oz_dispatch.h" in content

    def test_missing_protocol_method_error(self):
        """OZ-033: missing protocol method should cause exit code 1."""
        rc = _compile_and_run("""\
#import <Foundation/OZObject.h>
@protocol SensorProto
- (int)readValue;
@end
@interface Sensor : OZObject <SensorProto>
- (void)name;
@end
@implementation Sensor
- (void)name {}
@end
""")
        assert rc == 1

    def test_protocol_conformance_passes_when_complete(self):
        """OZ-033: class implementing all protocol methods should succeed."""
        rc = _compile_and_run("""\
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
        assert rc == 0

    def test_external_protected_ivar_access_error(self):
        """OZ-043: external access to protected ivar should fail.
        Corner case: Clang rejects protected ivar access at parse time."""
        import json
        ast = {
            "kind": "TranslationUnitDecl",
            "inner": [
                {"kind": "ObjCInterfaceDecl", "name": "OZObject", "inner": []},
                {"kind": "ObjCImplementationDecl", "name": "OZObject",
                 "inner": [
                     {"kind": "ObjCMethodDecl", "name": "init",
                      "returnType": {"qualType": "instancetype"},
                      "inner": [{"kind": "CompoundStmt", "inner": [
                          {"kind": "ReturnStmt", "inner": [
                              {"kind": "DeclRefExpr",
                               "referencedDecl": {"name": "self"},
                               "type": {"qualType": "OZObject *"}}]}]}]},
                     {"kind": "ObjCMethodDecl", "name": "dealloc",
                      "returnType": {"qualType": "void"},
                      "inner": [{"kind": "CompoundStmt", "inner": []}]},
                 ]},
                {"kind": "ObjCInterfaceDecl", "name": "Car",
                 "super": {"name": "OZObject"},
                 "inner": [
                     {"kind": "ObjCIvarDecl", "name": "_color",
                      "type": {"qualType": "int"},
                      "access": "protected"},
                 ]},
                {"kind": "ObjCImplementationDecl", "name": "Car",
                 "super": {"name": "OZObject"}, "inner": []},
                {"kind": "FunctionDecl", "name": "main",
                 "type": {"qualType": "int ()"},
                 "inner": [{"kind": "CompoundStmt", "inner": [{
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
                 }]}],
                },
            ],
        }
        with tempfile.TemporaryDirectory() as tmpdir:
            ast_file = os.path.join(tmpdir, "protected.ast.json")
            with open(ast_file, "w") as f:
                json.dump(ast, f)
            rc = main(["--input", ast_file, "--outdir", tmpdir])
            assert rc == 1

    def test_external_public_ivar_access_succeeds(self):
        """OZ-043: external access to public ivar should succeed."""
        rc = _compile_and_run("""\
#import <Foundation/OZObject.h>
@interface Car : OZObject {
@public
        int _color;
}
@end
@implementation Car
@end
int main(void) {
        Car *myCar = [[Car alloc] init];
        int c = myCar->_color;
        return c;
}
""")
        assert rc == 0

    def test_unsupported_method_selector_error(self):
        """Methods with unsupported selectors should produce errors."""
        rc = _compile_and_run("""\
#import <Foundation/OZObject.h>
@interface Proxy : OZObject
- (void)forwardInvocation:(id)inv;
@end
@implementation Proxy
- (void)forwardInvocation:(id)inv {}
@end
""")
        assert rc == 1


def _clang_available() -> bool:
    try:
        return subprocess.run(
            ["clang", "--version"], capture_output=True).returncode == 0
    except FileNotFoundError:
        return False


@pytest.mark.skipif(not _clang_available(), reason="clang not available")
class TestGenericValidationE2E:
    """OZ-057/OZ-058: end-to-end generic type validation.

    .m source -> Clang AST dump -> oz_transpile -> error.
    """

    _MISMATCH_SOURCE = """\
#import <Foundation/Foundation.h>

@protocol DataProto
- (int)processValue:(int)value;
@end

@protocol SensorProto
- (int)readValue;
@end

@interface Sensor : OZObject <SensorProto>
- (int)readValue;
@end

@implementation Sensor
- (int)readValue { return 42; }
@end

int main(void) {
    Sensor *s = [[Sensor alloc] init];
    OZArray<id<DataProto>> *arr = @[ s ];
    return 0;
}
"""

    _VALID_SOURCE = """\
#import <Foundation/Foundation.h>

@protocol DataProto
- (int)processValue:(int)value;
@end

@interface Filter : OZObject <DataProto>
- (int)processValue:(int)value;
@end

@implementation Filter
- (int)processValue:(int)value { return value; }
@end

int main(void) {
    Filter *f = [[Filter alloc] init];
    OZArray<id<DataProto>> *arr = @[ f ];
    return 0;
}
"""

    def _clang_ast_dump(self, src_file, ast_file):
        import json
        result = subprocess.run(
            ["clang", "-Xclang", "-ast-dump=json", "-fsyntax-only",
             "-I", OZ_SDK_DIR, src_file],
            capture_output=True,
        )
        # Verify the AST is valid JSON — some Clang versions produce
        # malformed output on certain platforms.
        try:
            json.loads(result.stdout)
        except (json.JSONDecodeError, ValueError):
            pytest.skip("Clang produced invalid JSON AST on this platform")
        with open(ast_file, "wb") as f:
            f.write(result.stdout)

    def test_generic_mismatch_rejected(self):
        """Sensor does not conform to DataProto — transpiler must error."""
        import io
        import contextlib
        with tempfile.TemporaryDirectory() as tmpdir:
            src = os.path.join(tmpdir, "mismatch.m")
            ast = os.path.join(tmpdir, "mismatch.ast.json")
            with open(src, "w") as f:
                f.write(self._MISMATCH_SOURCE)
            self._clang_ast_dump(src, ast)

            stderr_buf = io.StringIO()
            with contextlib.redirect_stderr(stderr_buf):
                rc = main(["--input", ast, "--outdir", tmpdir,
                           "--sources", src])
            assert rc == 1, (
                f"expected transpiler to reject generic mismatch, "
                f"stderr: {stderr_buf.getvalue()}")
            assert "generic type mismatch" in stderr_buf.getvalue()
            assert "Sensor" in stderr_buf.getvalue()
            assert "DataProto" in stderr_buf.getvalue()

    def test_generic_correct_accepted(self):
        """Filter conforms to DataProto — no generic mismatch error."""
        import io
        import contextlib
        with tempfile.TemporaryDirectory() as tmpdir:
            src = os.path.join(tmpdir, "valid.m")
            ast = os.path.join(tmpdir, "valid.ast.json")
            with open(src, "w") as f:
                f.write(self._VALID_SOURCE)
            self._clang_ast_dump(src, ast)

            stderr_buf = io.StringIO()
            with contextlib.redirect_stderr(stderr_buf):
                main(["--input", ast, "--outdir", tmpdir,
                      "--sources", src])
            assert "generic type mismatch" not in stderr_buf.getvalue(), (
                f"unexpected generic error: {stderr_buf.getvalue()}")
