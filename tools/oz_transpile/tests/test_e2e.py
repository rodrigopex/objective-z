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

            expected = {"oz_dispatch.h", "oz_mem_slabs.h",
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
            slabs = open(os.path.join(tmpdir, "oz_mem_slabs.h")).read()
            assert "oz_slab_OZLed" in slabs
            assert "8" in slabs

    def test_retain_release_in_root(self):
        ast_file = os.path.join(FIXTURE_DIR, "simple_led.ast.json")
        with tempfile.TemporaryDirectory() as tmpdir:
            main(["--input", ast_file, "--outdir", tmpdir])
            root_c = open(os.path.join(tmpdir, "OZObject.c")).read()
            assert "OZObject_retain" in root_c
            assert "OZObject_release" in root_c
            assert "atomic_dec" in root_c


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

            # Create stub headers for Zephyr types
            zephyr_dir = os.path.join(tmpdir, "zephyr")
            sys_dir = os.path.join(zephyr_dir, "sys")
            os.makedirs(sys_dir)

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

            # Compile each .c file
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
