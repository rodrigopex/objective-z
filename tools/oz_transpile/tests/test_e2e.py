# SPDX-License-Identifier: Apache-2.0

import os
import subprocess
import tempfile

import pytest

from oz_transpile.__main__ import main

FIXTURE_DIR = os.path.join(os.path.dirname(__file__), "fixtures")


class TestCLI:
    def test_basic_pipeline(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            rc = main(["--input", ast_file, "--outdir", tmpdir, "--verbose"])
            assert rc == 0

            expected = {"oz_dispatch.h", "oz_dispatch.c",
                        "oz_mem_slabs.h", "oz_mem_slabs.c",
                        "OZObject.h", "OZObject.c",
                        "OZLed.h", "OZLed.c"}
            generated = set(os.listdir(tmpdir))
            assert expected.issubset(generated)

    def test_dispatch_header_content(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            content = open(os.path.join(tmpdir, "oz_dispatch.h")).read()
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
            led_h = open(os.path.join(tmpdir, "OZLed.h")).read()
            assert "struct OZObject base;" in led_h
            assert "int _pin;" in led_h
            assert "BOOL _state;" in led_h

    def test_super_call(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            led_c = open(os.path.join(tmpdir, "OZLed.c")).read()
            assert "OZObject_init((struct OZObject *)self)" in led_c

    def test_pool_sizes(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir,
                  "--pool-sizes", "OZLed=8"])
            slabs_h = open(os.path.join(tmpdir, "oz_mem_slabs.h")).read()
            slabs_c = open(os.path.join(tmpdir, "oz_mem_slabs.c")).read()
            assert "oz_slab_OZLed" in slabs_h
            assert "8" in slabs_c

    def test_retain_release_in_root(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            root_c = open(os.path.join(tmpdir, "OZObject.c")).read()
            assert "OZObject_retain" in root_c
            assert "OZObject_release" in root_c
            assert "atomic_dec" in root_c


def _create_zephyr_stubs(tmpdir: str) -> None:
    """Create stub headers for Zephyr types used by generated C."""
    zephyr_dir = os.path.join(tmpdir, "zephyr")
    sys_dir = os.path.join(zephyr_dir, "sys")
    os.makedirs(sys_dir, exist_ok=True)

    with open(os.path.join(zephyr_dir, "kernel.h"), "w") as f:
        f.write("#pragma once\n")
        f.write("#include <stdint.h>\n#include <string.h>\n")
        f.write("typedef int k_timeout_t;\n")
        f.write("#define K_NO_WAIT ((k_timeout_t)0)\n")
        f.write("struct k_mem_slab { int dummy; };\n")
        f.write("#define K_MEM_SLAB_DEFINE(name, bsz, cnt, align) "
                "struct k_mem_slab name\n")
        f.write("static inline int k_mem_slab_alloc("
                "struct k_mem_slab *s, void **p, k_timeout_t t) "
                "{ (void)s; (void)t; *p = (void*)0; return 0; }\n")
        f.write("static inline void k_mem_slab_free("
                "struct k_mem_slab *s, void *p) "
                "{ (void)s; (void)p; }\n")

    with open(os.path.join(sys_dir, "atomic.h"), "w") as f:
        f.write("#pragma once\n")
        f.write("#include <stdint.h>\n")
        f.write("typedef int32_t atomic_t;\n")
        f.write("typedef int32_t atomic_val_t;\n")
        f.write("static inline atomic_val_t atomic_inc(atomic_t *t) "
                "{ return (*t)++; }\n")
        f.write("static inline atomic_val_t atomic_dec(atomic_t *t) "
                "{ atomic_val_t old = *t; (*t)--; return old; }\n")
        f.write("static inline atomic_val_t atomic_get(atomic_t *t) "
                "{ return *t; }\n")

    with open(os.path.join(zephyr_dir, "sys", "printk.h"), "w") as f:
        f.write("#pragma once\n")
        f.write("static inline void printk(const char *fmt, ...) { (void)fmt; }\n")


def _gcc_syntax_check(tmpdir: str) -> None:
    """Run gcc -fsyntax-only on all .c files in tmpdir."""
    c_files = [f for f in os.listdir(tmpdir) if f.endswith(".c")]
    for c_file in c_files:
        result = subprocess.run(
            ["gcc", "-fsyntax-only", "-Wall", "-Werror",
             "-I", tmpdir, f"-I{tmpdir}",
             os.path.join(tmpdir, c_file)],
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
    """Verify generated C passes gcc -fsyntax-only (with stubs for Zephyr)."""

    def test_generated_c_compiles(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            _create_zephyr_stubs(tmpdir)
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
            _create_zephyr_stubs(tmpdir)
            _gcc_syntax_check(tmpdir)
