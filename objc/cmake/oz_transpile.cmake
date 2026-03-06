# SPDX-License-Identifier: Apache-2.0
#
# oz_transpile.cmake — Transpile Objective-C (.m) to plain C via Clang AST.
#
# No ObjC runtime dependency — generates pure C compilable by GCC.
# Reuses objz_find_clang() and _objz_build_ast_flags() from ObjcClang.cmake.

include_guard(GLOBAL)

get_filename_component(_OZ_TRANSPILE_CMAKE_DIR "${CMAKE_CURRENT_LIST_FILE}" DIRECTORY)
include(${_OZ_TRANSPILE_CMAKE_DIR}/ObjcClang.cmake)

# ─── Public API ───────────────────────────────────────────────────────
#
# objz_transpile_sources(<target> <source1.m> [source2.m ...]
#   [ROOT_CLASS <name>]
#   [POOL_SIZES <Class1=N,Class2=M,...>]
#   [INCLUDE_DIRS <dir1> [dir2 ...]]
# )
#
# Transpiles .m sources to pure C at build time.  Generated files go to
# ${CMAKE_CURRENT_BINARY_DIR}/oz_generated.
#
function(objz_transpile_sources target)
    cmake_parse_arguments(OZT "" "ROOT_CLASS;POOL_SIZES" "INCLUDE_DIRS" ${ARGN})

    # Remaining unparsed args are source files
    set(_sources ${OZT_UNPARSED_ARGUMENTS})
    if(NOT _sources)
        message(FATAL_ERROR "objz_transpile_sources: at least one source file required")
    endif()
    if(NOT OZT_ROOT_CLASS)
        set(OZT_ROOT_CLASS "OZObject")
    endif()

    objz_find_clang()
    _objz_build_ast_flags(_ast_flags)

    # Add user-specified include dirs
    foreach(_dir ${OZT_INCLUDE_DIRS})
        list(APPEND _ast_flags -I${_dir})
    endforeach()

    # Add target include dirs
    get_target_property(_target_incs ${target} INCLUDE_DIRECTORIES)
    if(_target_incs)
        foreach(_dir ${_target_incs})
            string(FIND "${_dir}" "$<" _is_genexpr)
            if(_is_genexpr EQUAL -1)
                list(APPEND _ast_flags -I${_dir})
            endif()
        endforeach()
    endif()

    set(_outdir ${CMAKE_CURRENT_BINARY_DIR}/oz_generated)
    set(_ast_dir ${CMAKE_CURRENT_BINARY_DIR}/oz_ast)
    set(_manifest ${_outdir}/oz_manifest.txt)

    # Locate transpiler relative to this cmake file (objc/cmake/ -> ../../tools)
    get_filename_component(_transpile_dir
        "${_OZ_TRANSPILE_CMAKE_DIR}/../../tools" ABSOLUTE)

    # ── Configure-time: AST dump + transpile to discover output files ──
    set(_ast_files "")
    set(_abs_sources "")
    foreach(_src ${_sources})
        get_filename_component(_name ${_src} NAME)
        get_filename_component(_abs ${_src} ABSOLUTE)
        string(MAKE_C_IDENTIFIER "${_name}" _safe)
        set(_ast "${_ast_dir}/${_safe}.ast.json")

        file(MAKE_DIRECTORY ${_ast_dir})
        execute_process(
            COMMAND ${OBJZ_CLANG_COMPILER}
                    ${_ast_flags}
                    -fsyntax-only -Xclang -ast-dump=json
                    ${_abs}
            OUTPUT_FILE ${_ast}
            ERROR_QUIET
        )
        list(APPEND _ast_files ${_ast})
        list(APPEND _abs_sources ${_abs})
    endforeach()

    set(_pool_flag "")
    if(OZT_POOL_SIZES)
        set(_pool_flag "--pool-sizes=${OZT_POOL_SIZES}")
    endif()

    file(MAKE_DIRECTORY ${_outdir})
    execute_process(
        COMMAND ${CMAKE_COMMAND} -E env PYTHONPATH=${_transpile_dir}
                ${Python3_EXECUTABLE} -m oz_transpile
                --input ${_ast_files}
                --outdir ${_outdir}
                --root-class=${OZT_ROOT_CLASS}
                --manifest=${_manifest}
                ${_pool_flag}
        RESULT_VARIABLE _rc
    )
    if(NOT _rc EQUAL 0)
        message(FATAL_ERROR "objz_transpile_sources: transpiler failed at configure time")
    endif()

    # Read manifest to discover generated files
    file(STRINGS ${_manifest} _gen_files)

    # ── Build-time: re-run when .m sources change ─────────────────────
    set(_stamp ${_outdir}/.oz_transpile.stamp)

    # Build a shell script to AST-dump each source then run the transpiler
    set(_script "${_ast_dir}/oz_transpile_build.sh")
    set(_script_lines "#!/bin/sh\nset -e\n")
    foreach(_src ${_abs_sources})
        get_filename_component(_name ${_src} NAME)
        string(MAKE_C_IDENTIFIER "${_name}" _safe)
        set(_ast "${_ast_dir}/${_safe}.ast.json")
        string(JOIN " " _ast_cmd ${OBJZ_CLANG_COMPILER} ${_ast_flags}
               -fsyntax-only -Xclang -ast-dump=json ${_src})
        string(APPEND _script_lines "${_ast_cmd} > ${_ast} || true\n")
    endforeach()
    string(JOIN " " _transpile_cmd
           PYTHONPATH=${_transpile_dir}
           ${Python3_EXECUTABLE} -m oz_transpile
           --input ${_ast_files}
           --outdir ${_outdir}
           --root-class=${OZT_ROOT_CLASS}
           --manifest=${_manifest}
           ${_pool_flag})
    string(APPEND _script_lines "${_transpile_cmd}\n")
    file(WRITE ${_script} "${_script_lines}")

    add_custom_command(
        OUTPUT  ${_stamp} ${_gen_files}
        COMMAND sh ${_script}
        COMMAND ${CMAKE_COMMAND} -E touch ${_stamp}
        DEPENDS ${_abs_sources}
        COMMENT "oz_transpile: generating C from ObjC"
    )

    add_custom_target(oz_transpile_gen DEPENDS ${_stamp})
    add_dependencies(${target} oz_transpile_gen)

    # Add generated .c files and include dir
    foreach(_f ${_gen_files})
        get_filename_component(_ext ${_f} EXT)
        if("${_ext}" STREQUAL ".c")
            target_sources(${target} PRIVATE ${_f})
        endif()
    endforeach()
    target_include_directories(${target} PRIVATE ${_outdir})

    set_target_properties(${target} PROPERTIES LINKER_LANGUAGE C)
endfunction()
