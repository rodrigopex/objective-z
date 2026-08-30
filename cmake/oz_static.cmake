# SPDX-License-Identifier: Apache-2.0
#
# oz_static.cmake — Transpile Objective-C (.m) to plain C via the OZ-091
# Rust spike (`oz2c`) instead of the Python AST-based pipeline.
#
# Supported: any number of entry `.m` files (all merged into one
# translation unit, with further `.m`s pulled in automatically via
# `#import` — see tools/oz_static/src/imports.rs), INCLUDE_DIRS,
# ROOT_CLASS as a cross-check, and POOL_SIZES.
#
# Objects come from a per-class PAL slab (`OZ_SLAB_DEFINE`), sized by
# counting allocation sites and overridable per class — see
# tools/oz_static/src/pools.rs. A size can also be stated in the source
# itself as `/* oz-pool: Class=N,... */`, the same directive the Python
# backend's own test harness reads; POOL_SIZES here wins for the classes
# it names.
#

include_guard(GLOBAL)

# ─── Public API ───────────────────────────────────────────────────────
#
# objz_transpile_sources_static(<target> <src.m> [<src2.m> ...]
#   [ROOT_CLASS <name>]
#   [POOL_SIZES <Class1=N,Class2=M,...>]
#   [INCLUDE_DIRS <dir1> [dir2 ...]]
# )
#
# Same call shape as objz_transpile_sources() (oz_transpile.cmake) so a
# sample's CMakeLists.txt doesn't need to know which backend it's using.
#
function(objz_transpile_sources_static target)
    cmake_parse_arguments(OZT "" "ROOT_CLASS;POOL_SIZES" "INCLUDE_DIRS" ${ARGN})

    set(_mod ${ZEPHYR_OBJZ_MODULE_DIR})
    set(_sources ${OZT_UNPARSED_ARGUMENTS})

    if(NOT _sources)
        message(FATAL_ERROR
            "objz_transpile_sources_static: no .m source files given")
    endif()

    # ── Build oz2c once (debug profile: configure-time compile speed
    #    matters here, not the transpiler's own runtime speed) ────────
    set(_oz_static_dir ${_mod}/tools/oz_static)
    set(_oz2c ${_oz_static_dir}/target/debug/oz2c)
    # Zephyr's own toolchain cmake exports CC/CFLAGS (the ARM
    # cross-compiler) into ENV, which cc-rs (tree-sitter-objc's C parser
    # build script) would otherwise inherit -- unset them so cargo builds
    # oz2c, a host tool, with the host's own native compiler.
    execute_process(
        COMMAND ${CMAKE_COMMAND} -E env --unset=CC --unset=CXX --unset=CFLAGS --unset=CXXFLAGS
                --unset=LDFLAGS --unset=AR --unset=RANLIB --unset=NM
                cargo build --manifest-path ${_oz_static_dir}/Cargo.toml
        RESULT_VARIABLE _cargo_rc
    )
    if(NOT _cargo_rc EQUAL 0)
        message(FATAL_ERROR "objz_transpile_sources_static: cargo build of oz2c failed")
    endif()

    # Every listed `.m` becomes one entry file, and every directory they
    # live in becomes an `--impl-dir` so `#import "X.h"` can find its
    # sibling `X.m` there (oz2c never searches a header's own directory
    # for that -- see imports.rs::find_sibling_impl).
    set(_src_abs_list "")
    set(_src_dirs "")
    foreach(_src ${_sources})
        get_filename_component(_one_abs ${_src} ABSOLUTE)
        get_filename_component(_one_dir ${_one_abs} DIRECTORY)
        list(APPEND _src_abs_list ${_one_abs})
        list(APPEND _src_dirs ${_one_dir})
    endforeach()
    list(REMOVE_DUPLICATES _src_dirs)

    set(_oz2c_flags -I ${_mod}/include/oz_sdk)
    # CONFIG_OBJZ_HEAP enables `+allocWithHeap:` and the heap-aware free
    # path. The generated code is additionally guarded by OZ_HEAP_SUPPORT,
    # which is what makes the PAL expose the heap functions it calls -- so
    # both have to be set together, exactly as the Python backend does it.
    if(CONFIG_OBJZ_HEAP)
        list(APPEND _oz2c_flags --heap-support)
    endif()
    foreach(_dir ${_src_dirs})
        list(APPEND _oz2c_flags --impl-dir ${_dir})
    endforeach()
    foreach(_dir ${OZT_INCLUDE_DIRS})
        list(APPEND _oz2c_flags -I ${_dir})
    endforeach()
    # A root class is inferred, not configured (imports/collect find the
    # one class with no superclass), so pass ROOT_CLASS only when the
    # caller stated one -- oz2c then verifies it matches, turning a
    # mis-stated root into an error instead of a silently different
    # program. Costs an extra collect pass, hence only when asked.
    if(OZT_ROOT_CLASS)
        list(APPEND _oz2c_flags --root-class ${OZT_ROOT_CLASS})
    endif()
    # Same spelling as the Python backend's flag, so a sample states its
    # pool sizes once and either backend honours them.
    if(OZT_POOL_SIZES)
        list(APPEND _oz2c_flags --pool-sizes ${OZT_POOL_SIZES})
    endif()
    get_target_property(_target_incs ${target} INCLUDE_DIRECTORIES)
    if(_target_incs)
        foreach(_dir ${_target_incs})
            string(FIND "${_dir}" "$<" _is_genexpr)
            if(_is_genexpr EQUAL -1)
                list(APPEND _oz2c_flags -I ${_dir})
            endif()
        endforeach()
    endif()

    set(_outdir ${CMAKE_CURRENT_BINARY_DIR}/oz_static_generated)
    set(_manifest ${_outdir}/oz_static_manifest.txt)
    file(MAKE_DIRECTORY ${_outdir}/Foundation)

    # ── Clang AST dumps ───────────────────────────────────────────────
    #
    # tree-sitter gives oz2c syntax but no resolved types, so it cannot tell
    # on its own whether an `id`-typed ivar is an object the class owns. That
    # answer decides whether ARC releases the ivar: releasing a non-object
    # corrupts memory, skipping a real one leaks it, so without a dump oz2c
    # stays conservative and skips every `id` ivar. This build path passed no
    # --ast at all, which meant the on-target build was the one place those
    # facts were missing.
    #
    # One dump per source, and `--ast` is repeatable because a single dump
    # is not enough: Clang preprocesses `#import`s, so a dump of `main.m`
    # carries every `@interface` it imports but only the `@implementation`s
    # written in that one file. The module's own `src/*.m` are dumped too --
    # oz2c splices them via `--impl-dir`, and their ivars are exactly the
    # ones the Foundation classes own. oz2c unions the facts
    # (`astinfo::AstFacts::merge`).
    objz_find_clang()
    _objz_build_ast_flags(_ast_flags)
    list(APPEND _ast_flags -w)  # AST dump is transpiler input; warnings are noise
    list(PREPEND _ast_flags -I${_mod}/include/oz_sdk)
    # -fobjc-arc, or the dump carries no ownership qualifiers at all and the
    # whole exercise is pointless.
    list(APPEND _ast_flags -fobjc-arc)

    file(GLOB _sdk_impls ${_mod}/src/*.m)
    set(_ast_dir ${_outdir}/ast)
    file(MAKE_DIRECTORY ${_ast_dir})
    set(_ast_script "${_ast_dir}/oz_static_ast.sh")
    set(_ast_lines "#!/bin/sh\n")
    set(_ast_args "")
    foreach(_src ${_src_abs_list} ${_sdk_impls})
        get_filename_component(_name ${_src} NAME)
        string(MAKE_C_IDENTIFIER "${_name}" _safe)
        set(_ast "${_ast_dir}/${_safe}.ast.json")
        string(JOIN " " _one ${OBJZ_CLANG_COMPILER} ${_ast_flags}
               -fsyntax-only -Xclang -ast-dump=json ${_src})
        # `|| true`: a source Clang cannot fully parse still yields a usable
        # partial dump, and oz2c's own diagnostics are the better error
        # anyway. A dump that comes out empty is skipped below.
        string(APPEND _ast_lines "${_one} > ${_ast} 2>/dev/null || true\n")
        list(APPEND _ast_args --ast ${_ast})
    endforeach()
    file(WRITE ${_ast_script} "${_ast_lines}")
    execute_process(COMMAND sh ${_ast_script} RESULT_VARIABLE _ast_rc)
    if(NOT _ast_rc EQUAL 0)
        message(FATAL_ERROR
            "objz_transpile_sources_static: Clang AST dump failed -- see ${_ast_dir}")
    endif()
    # Only non-empty dumps are handed over: oz2c rejects one it cannot parse,
    # and an empty file is not a fact about anything.
    set(_oz2c_ast "")
    foreach(_src ${_src_abs_list} ${_sdk_impls})
        get_filename_component(_name ${_src} NAME)
        string(MAKE_C_IDENTIFIER "${_name}" _safe)
        set(_ast "${_ast_dir}/${_safe}.ast.json")
        if(EXISTS ${_ast})
            file(SIZE ${_ast} _ast_size)
            if(_ast_size GREATER 0)
                list(APPEND _oz2c_ast --ast ${_ast})
            endif()
        endif()
    endforeach()
    if(NOT _oz2c_ast)
        message(WARNING
            "objz_transpile_sources_static: no usable Clang AST dump -- ARC will "
            "skip every id-typed ivar rather than risk releasing a non-object")
    endif()

    # src/OZLog.c (linked in below, shared verbatim with the Python
    # backend) `#include`s "oz_dispatch.h" and "OZObject_ozh.h" -- the
    # Python pipeline's own generated filenames. Shim both to oz_static's
    # own names ("oz_static_dispatch.h", "OZObject.h") so the same file
    # compiles under either backend without editing it.
    file(WRITE ${_outdir}/Foundation/oz_dispatch.h
        "/* shim: src/OZLog.c expects this name under either backend */\n#include \"oz_static_dispatch.h\"\n")
    file(WRITE ${_outdir}/Foundation/OZObject_ozh.h
        "/* shim: src/OZLog.c expects this name under either backend */\n#include \"OZObject.h\"\n")

    # ── Configure-time: run once to discover output files ─────────────
    execute_process(
        COMMAND ${_oz2c} ${_oz2c_flags} ${_oz2c_ast} ${_src_abs_list} ${_outdir}
                --manifest ${_manifest}
        RESULT_VARIABLE _rc
    )
    if(NOT _rc EQUAL 0)
        message(FATAL_ERROR "objz_transpile_sources_static: oz2c failed at configure time")
    endif()
    file(STRINGS ${_manifest} _gen_files)

    # ── Build-time: re-run when the source changes ─────────────────────
    add_custom_command(
        OUTPUT  ${_gen_files}
        COMMAND ${CMAKE_COMMAND} -E env --unset=CC --unset=CXX --unset=CFLAGS --unset=CXXFLAGS
                --unset=LDFLAGS --unset=AR --unset=RANLIB --unset=NM
                cargo build --manifest-path ${_oz_static_dir}/Cargo.toml
        COMMAND sh ${_ast_script}
        COMMAND ${_oz2c} ${_oz2c_flags} ${_oz2c_ast} ${_src_abs_list} ${_outdir}
                --manifest ${_manifest}
        DEPENDS ${_src_abs_list}
        COMMENT "oz_static: generating C from ObjC (oz2c)"
    )

    add_custom_target(oz_static_transpile_gen DEPENDS ${_gen_files})
    add_dependencies(oz_static_transpile_gen zephyr_generated_headers)
    add_dependencies(${target} oz_static_transpile_gen)

    foreach(_f ${_gen_files})
        get_filename_component(_ext ${_f} EXT)
        if("${_ext}" STREQUAL ".c")
            target_sources(${target} PRIVATE ${_f})
        endif()
    endforeach()
    # BEFORE, not appended: a generated header is the translation of the
    # source header it was made from and carries the same basename, so both
    # can be on the include path at once. A sample that does
    # `target_include_directories(app PRIVATE include)` gets its own
    # directory searched first, and `#include "Car.h"` from generated C then
    # found `samples/*/include/Car.h` -- the Objective-C original, whose
    # `#import <objc/objc.h>` the ARM compiler cannot resolve. The generated
    # translation has to win.
    target_include_directories(${target} BEFORE PRIVATE ${_outdir} ${_outdir}/Foundation)

    # PAL: select Zephyr backend and provide include path
    target_include_directories(${target} PRIVATE ${_mod}/include)
    # oz_sdk too, for the SDK headers oz2c resolves but deliberately does
    # not splice: a header that reaches no Objective-C is left as the
    # ordinary `#include` it was, so the path it names has to resolve at
    # compile time as well. `samples/zbus_service` has
    # `#include <Foundation/OZLog.h>`, which lives only here.
    target_include_directories(${target} PRIVATE ${_mod}/include/oz_sdk)
    target_compile_definitions(${target} PRIVATE OZ_PLATFORM_ZEPHYR)
    if(CONFIG_OBJZ_HEAP)
        target_compile_definitions(${target} PRIVATE OZ_HEAP_SUPPORT)
    endif()

    # Real oz_sdk headers keep ARC ownership qualifiers (__unsafe_unretained,
    # etc.) for the Python pipeline's Clang-AST analysis, which genuinely
    # needs them -- oz_static preserves ivar declarations verbatim from
    # source rather than re-synthesizing them (its "literate" design), so
    # those qualifiers reach the final GCC compile unchanged. They're a
    # a no-op for the generated C either way: oz_static lowers the
    # qualifiers off the ivar declarations it emits, and its own ARC works
    # off the Clang AST's ownership facts rather than off these spellings
    # surviving into C. The -D covers the positions the lowering does not
    # rewrite (a method parameter, a local), and is an empty define rather
    # than an edit to the shared headers or to oz_static's verbatim copy.
    target_compile_definitions(${target} PRIVATE __unsafe_unretained=)

    # Add OZLog support (pure C, matching the prototypes hardcoded into
    # oz_static's own generated companion header -- see companion.rs). It
    # reaches for "oz_dispatch.h" and "OZObject_ozh.h", the Python
    # pipeline's generated filenames; the two shim headers written into
    # ${_outdir}/Foundation above are what make the same file compile here,
    # and ${_outdir}/Foundation is on this target's include path.
    target_sources(${target} PRIVATE ${_mod}/src/OZLog.c)

    set_target_properties(${target} PROPERTIES LINKER_LANGUAGE C)
endfunction()
