# SPDX-License-Identifier: Apache-2.0
#
# oz_static.cmake — Transpile Objective-C (.m) to plain C via the OZ-091
# Rust spike (`ozcc`) instead of the Python AST-based pipeline.
#
# Pilot scope only: a single entry `.m` file (sibling `.m`s are pulled in
# automatically via `#import`, see tools/oz_static/src/imports.rs), no
# POOL_SIZES, no CONFIG_OBJZ_HEAP — oz_static's allocator is always plain
# malloc/free, no PAL slab/heap integration yet.

include_guard(GLOBAL)

# ─── Public API ───────────────────────────────────────────────────────
#
# objz_transpile_sources_static(<target> <src.m>
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

    if(OZT_POOL_SIZES)
        message(FATAL_ERROR
            "objz_transpile_sources_static: POOL_SIZES not supported yet (OZ-091 -- "
            "oz_static's allocator is always malloc/free, no PAL slab integration)")
    endif()
    if(CONFIG_OBJZ_HEAP)
        message(FATAL_ERROR
            "objz_transpile_sources_static: CONFIG_OBJZ_HEAP not supported yet (OZ-091)")
    endif()
    list(LENGTH _sources _n_sources)
    if(NOT _n_sources EQUAL 1)
        message(FATAL_ERROR
            "objz_transpile_sources_static: exactly one entry .m file is supported "
            "(sibling .m files are pulled in automatically via #import)")
    endif()

    # ── Build ozcc once (debug profile: configure-time compile speed
    #    matters here, not the transpiler's own runtime speed) ────────
    set(_oz_static_dir ${_mod}/tools/oz_static)
    set(_ozcc ${_oz_static_dir}/target/debug/ozcc)
    # Zephyr's own toolchain cmake exports CC/CFLAGS (the ARM
    # cross-compiler) into ENV, which cc-rs (tree-sitter-objc's C parser
    # build script) would otherwise inherit -- unset them so cargo builds
    # ozcc, a host tool, with the host's own native compiler.
    execute_process(
        COMMAND ${CMAKE_COMMAND} -E env --unset=CC --unset=CXX --unset=CFLAGS --unset=CXXFLAGS
                --unset=LDFLAGS --unset=AR --unset=RANLIB --unset=NM
                cargo build --manifest-path ${_oz_static_dir}/Cargo.toml
        RESULT_VARIABLE _cargo_rc
    )
    if(NOT _cargo_rc EQUAL 0)
        message(FATAL_ERROR "objz_transpile_sources_static: cargo build of ozcc failed")
    endif()

    get_filename_component(_src_abs ${_sources} ABSOLUTE)
    get_filename_component(_src_dir ${_src_abs} DIRECTORY)

    set(_ozcc_flags -I ${_mod}/include/oz_sdk --impl-dir ${_src_dir})
    foreach(_dir ${OZT_INCLUDE_DIRS})
        list(APPEND _ozcc_flags -I ${_dir})
    endforeach()
    get_target_property(_target_incs ${target} INCLUDE_DIRECTORIES)
    if(_target_incs)
        foreach(_dir ${_target_incs})
            string(FIND "${_dir}" "$<" _is_genexpr)
            if(_is_genexpr EQUAL -1)
                list(APPEND _ozcc_flags -I ${_dir})
            endif()
        endforeach()
    endif()

    set(_outdir ${CMAKE_CURRENT_BINARY_DIR}/oz_static_generated)
    set(_manifest ${_outdir}/oz_static_manifest.txt)
    file(MAKE_DIRECTORY ${_outdir}/Foundation)

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
        COMMAND ${_ozcc} ${_ozcc_flags} ${_src_abs} ${_outdir} --manifest ${_manifest}
        RESULT_VARIABLE _rc
    )
    if(NOT _rc EQUAL 0)
        message(FATAL_ERROR "objz_transpile_sources_static: ozcc failed at configure time")
    endif()
    file(STRINGS ${_manifest} _gen_files)

    # ── Build-time: re-run when the source changes ─────────────────────
    add_custom_command(
        OUTPUT  ${_gen_files}
        COMMAND ${CMAKE_COMMAND} -E env --unset=CC --unset=CXX --unset=CFLAGS --unset=CXXFLAGS
                --unset=LDFLAGS --unset=AR --unset=RANLIB --unset=NM
                cargo build --manifest-path ${_oz_static_dir}/Cargo.toml
        COMMAND ${_ozcc} ${_ozcc_flags} ${_src_abs} ${_outdir} --manifest ${_manifest}
        DEPENDS ${_src_abs}
        COMMENT "oz_static: generating C from ObjC (ozcc)"
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
    target_include_directories(${target} PRIVATE ${_outdir} ${_outdir}/Foundation)

    # PAL: select Zephyr backend and provide include path
    target_include_directories(${target} PRIVATE ${_mod}/include)
    target_compile_definitions(${target} PRIVATE OZ_PLATFORM_ZEPHYR)

    # Add OZLog support (pure C, matches the prototype hardcoded into
    # oz_static's own generated companion header -- see companion.rs)
    target_sources(${target} PRIVATE ${_mod}/src/OZLog.c)

    set_target_properties(${target} PROPERTIES LINKER_LANGUAGE C)
endfunction()
