# SPDX-License-Identifier: Apache-2.0
#
# oz_static.cmake — Transpile Objective-C (.m) to plain C via `oz2c`.
#
# The entry point every consumer calls is `objz_transpile_sources()`, defined
# at the bottom of this file. It used to live in `oz_transpile.cmake` and
# dispatch on CONFIG_OBJZ_BACKEND between two backends; with the Python one
# retired there is nothing to dispatch on, so that file is gone and this one
# is included directly from the module's CMakeLists.txt. The public name is
# unchanged because 13 samples, px-app and any out-of-tree user call it.
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

# objz_find_clang() and _objz_build_ast_flags(), for the AST dumps oz2c reads
# as an ownership oracle. `oz_transpile.cmake` used to include this before
# handing over; now that it is gone, this file needs it itself.
include(${ZEPHYR_OBJZ_MODULE_DIR}/cmake/ObjcClang.cmake)

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
    # Globbed at configure time, which is enough for the case this exists
    # for: editing the transpiler and rebuilding a sample. Adding a *new*
    # source file needs a re-configure, same as any CMake glob.
    file(GLOB_RECURSE _oz2c_srcs ${_oz_static_dir}/src/*.rs)
    list(APPEND _oz2c_srcs ${_oz_static_dir}/Cargo.toml)
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
    # CONFIG_OBJZ_INTROSPECTION enables -isKindOfClass: and
    # -conformsToProtocol:, the two introspection selectors that generate a
    # table. Unlike --heap-support this needs nothing defined on the C side:
    # the tables and their helpers are emitted into the companion source
    # itself, and only for the constructs a call site actually used. Class
    # identity ([Foo class], [obj class], -isMemberOfClass:) is always
    # available and unaffected.
    if(CONFIG_OBJZ_INTROSPECTION)
        list(APPEND _oz2c_flags --introspection)
    endif()
    # CONFIG_OBJZ_REFLECTION enables @selector, SEL, -respondsToSelector:
    # and the -performSelector: family. Like --introspection this needs
    # nothing on the C side: the selector records, their wrappers and the
    # two helpers are emitted into the companion source, and only for the
    # selectors a @selector(...) actually named.
    if(CONFIG_OBJZ_REFLECTION)
        list(APPEND _oz2c_flags --reflection)
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
    # The target's own include directories, which `include_directories(include)`
    # in a sample's CMakeLists sets. Collected once into `_target_inc_dirs`
    # because **two consumers need them and only one used to get them** (#274):
    # oz2c resolved a sample's own headers while the Clang AST dump below did
    # not, so a dump of `samples/zbus_service/src/main.m` died on
    # `fatal error: 'TemperatureService.h' file not found` -- and, being
    # truncated rather than absent, looked like a complete oracle.
    #
    # `cmake/oz_transpile.cmake` has always added them to its own AST flags
    # (its "Add target include dirs" block), so this was a port omission
    # rather than a question anyone decided. The outgoing backend being the
    # correct reference is unusual enough to be worth the note.
    set(_target_inc_dirs "")
    get_target_property(_target_incs ${target} INCLUDE_DIRECTORIES)
    if(_target_incs)
        foreach(_dir ${_target_incs})
            string(FIND "${_dir}" "$<" _is_genexpr)
            if(_is_genexpr EQUAL -1)
                list(APPEND _target_inc_dirs ${_dir})
            endif()
        endforeach()
    endif()
    foreach(_dir ${_target_inc_dirs})
        list(APPEND _oz2c_flags -I ${_dir})
    endforeach()

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
    # The target's own include dirs, so a sample's `#include "Foo.h"` resolves
    # here as it already did for oz2c above (#274). `oz_transpile.cmake` has
    # always done this.
    foreach(_dir ${_target_inc_dirs})
        list(APPEND _ast_flags -I${_dir})
    endforeach()

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
        # Keep each dump's diagnostics next to it, but only the ones that
        # mean the dump is *incomplete* -- a `fatal error`, which is where
        # Clang stops (#274). The previous form was
        # `> ${_ast} 2>/dev/null || true`, which threw away both the exit
        # code and the message; and since every line ended in `|| true` the
        # script's own status was always 0, making the
        # `if(NOT _ast_rc EQUAL 0)` check below it dead code.
        #
        # Exit status alone is the wrong signal, which cost a pass here to
        # discover. Clang exits non-zero for an ordinary error too and then
        # *carries on*, so these dumps have always been produced with a
        # handful of errors in them -- `__get_BASEPRI` and friends, because
        # these flags name no `--target` and CMSIS therefore selects its
        # A-profile header on an M-profile build, plus `__oz_timer_setup`
        # from the PAL arm that goes with it. None of that truncates
        # anything, and none of it touches an ivar's ownership qualifier or
        # whether an `@implementation` was seen, which is all the oracle
        # reads. A `fatal error` is different in kind: Clang stops, and
        # every declaration after it is simply absent from a file that still
        # looks complete.
        string(APPEND _ast_lines
            "${_one} > ${_ast} 2> ${_ast}.err\n"
            "grep -q 'fatal error:' ${_ast}.err || rm -f ${_ast}.err\n")
        list(APPEND _ast_args --ast ${_ast})
    endforeach()
    file(WRITE ${_ast_script} "${_ast_lines}")

    # A dump Clang could not complete is fatal, because it is
    # indistinguishable from a complete one by inspection and it silently
    # weakens the only oracle for ivar ownership (gaps N and AA of the
    # retired PARITY.md; docs/STATUS.md says how to read it).
    #
    # Checked by its own script rather than here, because **this script runs
    # at two different times and only one of them can succeed**:
    #
    #   - at configure time (`execute_process` below), whose sole purpose is
    #     to discover the output file list for the manifest. Zephyr's
    #     *generated* headers do not exist yet on a pristine build, so
    #     anything reaching `zephyr/kernel.h` dies on
    #     `fatal error: 'zephyr/syscall_list.h' file not found`. Expected,
    #     and harmless: a file name does not depend on an ARC fact, and the
    #     build-time run overwrites this output anyway.
    #   - at build time (the `add_custom_command` further down), which
    #     `add_dependencies(oz_static_transpile_gen zephyr_generated_headers)`
    #     orders after those headers exist. **This is the run whose dumps
    #     reach the shipped C**, so this is the run worth checking.
    #
    # #274's include-path bug affected both, which is why it mattered: the
    # build-time dump was truncated too, for every sample with its own
    # `include/`.
    #
    # Deliberately fatal rather than a warning: #269 found that the
    # compatibility warning meant to catch a silently-substituted clang had
    # printed on every CI run for the life of the workflow, unread in a
    # 1400-line log. A warning about a substituted oracle is not a check.
    set(_ast_check "${_ast_dir}/oz_static_ast_check.sh")
    if(OBJZ_ALLOW_PARTIAL_AST)
        file(WRITE ${_ast_check}
            "#!/bin/sh\n"
            "# OBJZ_ALLOW_PARTIAL_AST=ON: report, never fail. ARC then treats\n"
            "# these files' `id` ivars conservatively and skips them --\n"
            "# correct, but a leak.\n"
            "for f in ${_ast_dir}/*.ast.json.err; do\n"
            "  [ -e \"$f\" ] || exit 0\n"
            "  echo \"oz_static: WARNING: truncated Clang AST dump:\" >&2\n"
            "  cat \"$f\" >&2\n"
            "done\n"
            "exit 0\n")
    else()
        file(WRITE ${_ast_check}
            "#!/bin/sh\n"
            "found=0\n"
            "for f in ${_ast_dir}/*.ast.json.err; do\n"
            "  [ -e \"$f\" ] || break\n"
            "  found=1\n"
            "  echo \"oz_static: Clang hit a fatal error, so this AST dump stops\" >&2\n"
            "  echo \"where the error is and the ivar-ownership oracle is\" >&2\n"
            "  echo \"incomplete for that file (see docs/STATUS.md):\" >&2\n"
            "  cat \"$f\" >&2\n"
            "done\n"
            "[ \$found -eq 0 ] && exit 0\n"
            "echo \"oz_static: fix the diagnostics above, or configure with\" >&2\n"
            "echo \"-DOBJZ_ALLOW_PARTIAL_AST=ON to proceed with conservative\" >&2\n"
            "echo \"ARC (which leaks \\`id\\` ivars in those files).\" >&2\n"
            "exit 1\n")
    endif()

    execute_process(COMMAND sh ${_ast_script})

    # Only non-empty dumps are handed over: oz2c rejects one it cannot parse,
    # and an empty file is not a fact about anything. Note this test is *not*
    # what catches a truncated dump -- that is the `.err` check above, and
    # conflating the two is what let #274 stand.
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
        # Ordered between the dumps and oz2c deliberately: this is the run
        # whose facts reach the shipped C, and a truncated dump must stop the
        # build here rather than quietly weaken ARC (#274).
        COMMAND sh ${_ast_check}
        COMMAND ${_oz2c} ${_oz2c_flags} ${_oz2c_ast} ${_src_abs_list} ${_outdir}
                --manifest ${_manifest}
        # The transpiler's own sources, not just the .m inputs: without them
        # ninja considers the generated C up to date after oz2c itself
        # changes, so a rebuilt transpiler silently produces nothing new.
        # That cost real debugging time -- a fix would land, the sample would
        # be rebuilt, and the old generated C would still be compiled.
        DEPENDS ${_src_abs_list} ${_oz2c_srcs}
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


# ─── Public entry point ───────────────────────────────────────────────
#
# `objz_transpile_sources()` is what samples, px-app and out-of-tree users
# call. It was a dispatcher over CONFIG_OBJZ_BACKEND until the Python
# pipeline was retired; there is one backend now, so it forwards. Kept as a
# separate name rather than renaming the implementation so that every
# existing call site keeps working.
function(objz_transpile_sources target)
    objz_transpile_sources_static(${target} ${ARGN})
endfunction()
