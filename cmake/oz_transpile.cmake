# SPDX-License-Identifier: Apache-2.0
#
# oz_transpile.cmake — Transpile Objective-C (.m) to plain C via Clang AST.
#
# No ObjC runtime dependency — generates pure C compilable by GCC.

include_guard(GLOBAL)

# ─── Find Clang for AST dump ─────────────────────────────────────────
function(_oz_transpile_find_clang)
    if(DEFINED OZ_TRANSPILE_CLANG)
        return()
    endif()

    find_program(OZ_TRANSPILE_CLANG clang)
    if(NOT OZ_TRANSPILE_CLANG)
        find_program(OZ_TRANSPILE_CLANG clang
            PATHS /opt/homebrew/opt/llvm/bin
                  /usr/local/opt/llvm/bin
            NO_DEFAULT_PATH)
    endif()

    if(NOT OZ_TRANSPILE_CLANG)
        message(FATAL_ERROR
            "oz_transpile requires clang for AST dump.\n"
            "Install clang or set -DOZ_TRANSPILE_CLANG=/path/to/clang")
    endif()

    set(OZ_TRANSPILE_CLANG ${OZ_TRANSPILE_CLANG} CACHE INTERNAL
        "Clang for oz_transpile AST dump")
    message(STATUS "oz_transpile: using Clang: ${OZ_TRANSPILE_CLANG}")
endfunction()

# ─── Public API ───────────────────────────────────────────────────────
#
# oz_transpile(
#   TARGET <target>
#   SOURCES <source1.m> [source2.m ...]
#   OUTPUT_DIR <dir>
#   [ROOT_CLASS <name>]
#   [POOL_SIZES <Class1=N,Class2=M,...>]
# )
#
function(oz_transpile)
    cmake_parse_arguments(OZT "" "TARGET;OUTPUT_DIR;ROOT_CLASS;POOL_SIZES" "SOURCES" ${ARGN})

    if(NOT OZT_TARGET)
        message(FATAL_ERROR "oz_transpile: TARGET is required")
    endif()
    if(NOT OZT_SOURCES)
        message(FATAL_ERROR "oz_transpile: SOURCES is required")
    endif()
    if(NOT OZT_OUTPUT_DIR)
        set(OZT_OUTPUT_DIR "${CMAKE_CURRENT_BINARY_DIR}/oz_generated")
    endif()
    if(NOT OZT_ROOT_CLASS)
        set(OZT_ROOT_CLASS "OZObject")
    endif()

    _oz_transpile_find_clang()

    # Locate the transpiler script
    get_filename_component(_cmake_dir "${CMAKE_CURRENT_FUNCTION_LIST_FILE}" DIRECTORY)
    get_filename_component(_project_root "${_cmake_dir}/.." ABSOLUTE)
    set(_transpile_dir "${_project_root}/tools")

    # Step 1: AST dump each .m file
    set(_ast_files "")
    foreach(_src ${OZT_SOURCES})
        get_filename_component(_name ${_src} NAME)
        get_filename_component(_abs ${_src} ABSOLUTE)
        string(MAKE_C_IDENTIFIER "${_name}" _safe)
        set(_ast "${CMAKE_CURRENT_BINARY_DIR}/oz_ast/${_safe}.ast.json")

        add_custom_command(
            OUTPUT  ${_ast}
            COMMAND ${CMAKE_COMMAND} -E make_directory
                    ${CMAKE_CURRENT_BINARY_DIR}/oz_ast
            COMMAND ${OZ_TRANSPILE_CLANG}
                    -fsyntax-only -Xclang -ast-dump=json
                    ${_abs} > ${_ast} || true
            DEPENDS ${_abs}
            COMMENT "oz_transpile: AST dump ${_name}"
        )
        list(APPEND _ast_files ${_ast})
    endforeach()

    # Step 2: Transpile (single invocation for all AST files)
    # For now, only single-source is supported; concatenate if needed
    set(_pool_flag "")
    if(OZT_POOL_SIZES)
        set(_pool_flag "--pool-sizes=${OZT_POOL_SIZES}")
    endif()

    # We need a stamp file since output file names depend on class names
    set(_stamp "${OZT_OUTPUT_DIR}/.oz_transpile.stamp")

    add_custom_command(
        OUTPUT  ${_stamp}
        COMMAND ${CMAKE_COMMAND} -E make_directory ${OZT_OUTPUT_DIR}
        COMMAND ${CMAKE_COMMAND} -E env PYTHONPATH=${_transpile_dir}
                ${Python3_EXECUTABLE} -m oz_transpile
                --input ${_ast_files}
                --outdir ${OZT_OUTPUT_DIR}
                --root-class=${OZT_ROOT_CLASS}
                ${_pool_flag}
        COMMAND ${CMAKE_COMMAND} -E touch ${_stamp}
        DEPENDS ${_ast_files}
        COMMENT "oz_transpile: generating C from ObjC"
        VERBATIM
    )

    add_custom_target(oz_transpile_gen DEPENDS ${_stamp})
    add_dependencies(${OZT_TARGET} oz_transpile_gen)

    # Add output dir to include path
    target_include_directories(${OZT_TARGET} PRIVATE ${OZT_OUTPUT_DIR})

    # Glob generated .c files after generation
    # Since we can't glob at configure time for files that don't exist yet,
    # we need to know the class names. Use a wrapper approach: add sources
    # in a deferred step after generation.
    # For simplicity, use file(GLOB) at configure time if the output dir exists,
    # otherwise add a post-build step.
    if(EXISTS "${OZT_OUTPUT_DIR}")
        file(GLOB _gen_c_files "${OZT_OUTPUT_DIR}/*.c")
        if(_gen_c_files)
            target_sources(${OZT_TARGET} PRIVATE ${_gen_c_files})
        endif()
    endif()
endfunction()
