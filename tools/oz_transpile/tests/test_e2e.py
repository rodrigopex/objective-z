# SPDX-License-Identifier: Apache-2.0

import os
import subprocess
import tempfile

import pytest

from oz_transpile.__main__ import main

FIXTURE_DIR = os.path.join(os.path.dirname(__file__), "fixtures")
PAL_INCLUDE_DIR = os.path.join(os.path.dirname(__file__), "..", "..", "..", "include")


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
                    "oz_mem_slabs.h", "oz_mem_slabs.c",
                    "OZObject_ozh.h", "OZObject_ozm.c"}.issubset(foundation_files)
            assert {"OZLed_ozh.h", "OZLed_ozm.c"}.issubset(user_files)

    def test_dispatch_header_content(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            content = open(os.path.join(tmpdir, "Foundation", "oz_dispatch.h")).read()
            # toggle is in protocol -> should have vtable
            assert "OZ_SEND_toggle" in content
            # init is overridden -> protocol dispatch
            assert "OZ_SEND_init" in content
            # turnOn is unique -> no vtable entry
            assert "OZ_SEND_turnOn" not in content

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
            slabs_h = open(os.path.join(tmpdir, "Foundation", "oz_mem_slabs.h")).read()
            slabs_c = open(os.path.join(tmpdir, "Foundation", "oz_mem_slabs.c")).read()
            assert "oz_slab_OZLed" in slabs_h
            assert "8" in slabs_c

    def test_retain_release_in_root(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            root_c = open(os.path.join(tmpdir, "Foundation", "OZObject_ozm.c")).read()
            assert "OZObject_retain" in root_c
            assert "OZObject_release" in root_c
            assert "oz_atomic_dec_and_test" in root_c


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

            slabs_h = open(os.path.join(tmpdir, "Foundation", "oz_mem_slabs.h")).read()
            assert "OZLock_initWithObject" in slabs_h
            assert "OZLock_dealloc" in slabs_h

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
