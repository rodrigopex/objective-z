# SPDX-License-Identifier: Apache-2.0
#
# oz_transpile.cmake — Transpile Objective-C (.m) to plain C via Clang AST.
#
# No ObjC runtime dependency — generates pure C compilable by GCC.
# Reuses objz_find_clang() and _objz_build_ast_flags() from ObjcClang.cmake.
#
# Paths use ZEPHYR_OBJZ_MODULE_DIR (set by Zephyr module system).

include_guard(GLOBAL)

include(${ZEPHYR_OBJZ_MODULE_DIR}/cmake/ObjcClang.cmake)

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

    set(_mod ${ZEPHYR_OBJZ_MODULE_DIR})

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
    list(APPEND _ast_flags -w)  # Suppress warnings — AST dump is transpiler input only

    # Add shared transpiler root class (OZObject.h + OZObject.m)
    set(_oz_inc_dir ${_mod}/include/oz_sdk)
    set(_oz_root_src ${_mod}/src/OZObject.m)
    set(_oz_string_src ${_mod}/src/OZString.m)
    set(_oz_array_src ${_mod}/src/OZArray.m)
    set(_oz_dict_src ${_mod}/src/OZDictionary.m)
    set(_oz_q31_src ${_mod}/src/OZQ31.m)
    # Prepend so transpiler stubs (assert.h, Foundation/, etc.) take priority
    list(PREPEND _ast_flags -I${_oz_inc_dir})
    list(PREPEND _sources ${_oz_q31_src} ${_oz_dict_src} ${_oz_array_src}
                          ${_oz_string_src} ${_oz_root_src})
    if(CONFIG_OBJZ_HEAP)
        set(_oz_heap_src ${_mod}/src/OZHeap.m)
        list(PREPEND _sources ${_oz_heap_src})
    endif()

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

    # ── Collect compile_commands entries for clangd ─────────────────────
    # Build Clang flags but swap gnustep-2.0 for macosx runtime so clangd
    # gets native blocks and ARC support (clangd is IDE-only, not for build).
    _objz_build_clang_flags(_clang_flags)
    list(REMOVE_ITEM _clang_flags -fobjc-runtime=gnustep-2.0)
    list(APPEND _clang_flags -fobjc-runtime=macosx -fobjc-arc -fblocks -I${_oz_inc_dir})
    foreach(_dir ${OZT_INCLUDE_DIRS})
        list(APPEND _clang_flags -I${_dir})
    endforeach()
    if(_target_incs)
        foreach(_dir ${_target_incs})
            string(FIND "${_dir}" "$<" _is_genexpr)
            if(_is_genexpr EQUAL -1)
                list(APPEND _clang_flags -I${_dir})
            endif()
        endforeach()
    endif()
    foreach(_src ${_sources})
        get_filename_component(_abs ${_src} ABSOLUTE)
        get_filename_component(_name ${_src} NAME)
        string(MAKE_C_IDENTIFIER "${_name}" _safe)
        set(_obj ${CMAKE_CURRENT_BINARY_DIR}/clang_objc_arc/${_safe}.o)
        _objz_collect_compile_db(${_abs} ${_obj} ${_clang_flags})
    endforeach()

    set(_outdir ${CMAKE_CURRENT_BINARY_DIR}/oz_generated)
    set(_ast_dir ${CMAKE_CURRENT_BINARY_DIR}/oz_ast)
    set(_manifest ${_outdir}/oz_manifest.txt)
    set(_transpile_dir ${_mod}/tools)

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

    set(_heap_flag "")
    if(CONFIG_OBJZ_HEAP)
        set(_heap_flag "--heap-support")
    endif()

    file(MAKE_DIRECTORY ${_outdir})
    execute_process(
        COMMAND ${CMAKE_COMMAND} -E env PYTHONPATH=${_transpile_dir}
                ${Python3_EXECUTABLE} -m oz_transpile
                --input ${_ast_files}
                --sources ${_abs_sources}
                --outdir ${_outdir}
                --root-class=${OZT_ROOT_CLASS}
                --manifest=${_manifest}
                --verbose
                ${_pool_flag}
                ${_heap_flag}
        RESULT_VARIABLE _rc
    )
    if(NOT _rc EQUAL 0)
        message(FATAL_ERROR "objz_transpile_sources: transpiler failed at configure time")
    endif()

    # Read manifest to discover generated files
    file(STRINGS ${_manifest} _gen_files)

    # ── Build-time: re-run when .m sources change ─────────────────────
    set(_stamp ${_outdir}/.oz_transpile.stamp)


    # Build a shell script to AST-dump each source then run the transpiler.
    # Clang stderr is captured per-source; shown only on transpiler failure.
    set(_script "${_ast_dir}/oz_transpile_build.sh")
    set(_script_lines "#!/bin/sh\nset -e\n")
    set(_err_logs "")
    foreach(_src ${_abs_sources})
        get_filename_component(_name ${_src} NAME)
        string(MAKE_C_IDENTIFIER "${_name}" _safe)
        set(_ast "${_ast_dir}/${_safe}.ast.json")
        set(_err "${_ast_dir}/${_safe}.err.log")
        string(JOIN " " _ast_cmd ${OBJZ_CLANG_COMPILER} ${_ast_flags}
               -fsyntax-only -Xclang -ast-dump=json ${_src})
        string(APPEND _script_lines "${_ast_cmd} > ${_ast} 2>${_err} || true\n")
        list(APPEND _err_logs ${_err})
    endforeach()
    string(JOIN " " _transpile_cmd
           PYTHONPATH=${_transpile_dir}
           ${Python3_EXECUTABLE} -m oz_transpile
           --input ${_ast_files}
           --sources ${_abs_sources}
           --outdir ${_outdir}
           --root-class=${OZT_ROOT_CLASS}
           --manifest=${_manifest}
           --verbose
           ${_pool_flag}
           ${_heap_flag})
    # Run transpiler; on failure dump Clang error logs for diagnosis
    string(JOIN " " _err_logs_str ${_err_logs})
    string(APPEND _script_lines
           "${_transpile_cmd} || { echo '--- Clang AST errors ---'; cat ${_err_logs_str}; exit 1; }\n")
    file(WRITE ${_script} "${_script_lines}")

    add_custom_command(
        OUTPUT  ${_stamp} ${_gen_files}
        COMMAND sh ${_script}
        COMMAND ${CMAKE_COMMAND} -E touch ${_stamp}
        DEPENDS ${_abs_sources}
        COMMENT "oz_transpile: generating C from ObjC"
    )

    add_custom_target(oz_transpile_gen DEPENDS ${_stamp})
    add_dependencies(oz_transpile_gen zephyr_generated_headers)
    add_dependencies(${target} oz_transpile_gen)

    # Add generated .c files and include dir
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
    if(CONFIG_OBJZ_HEAP)
        target_compile_definitions(${target} PRIVATE OZ_HEAP_SUPPORT)
    endif()

    # Add OZLog support (pure C, uses generated oz_dispatch.h for %@)
    target_sources(${target} PRIVATE ${_mod}/src/OZLog.c)

    set_target_properties(${target} PROPERTIES LINKER_LANGUAGE C)
endfunction()
