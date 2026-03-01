# SPDX-License-Identifier: Apache-2.0
#
# ObjcClang.cmake — Compile Objective-C (.m) files with Clang while linking
# with GCC.

include_guard(GLOBAL)

# ─── Find Clang ──────────────────────────────────────────────────────
function(objz_find_clang)
    if(DEFINED OBJZ_CLANG_PATH)
        find_program(OBJZ_CLANG_COMPILER clang
            PATHS ${OBJZ_CLANG_PATH} NO_DEFAULT_PATH)
    else()
        find_program(OBJZ_CLANG_COMPILER clang)
    endif()

    if(NOT OBJZ_CLANG_COMPILER)
        message(FATAL_ERROR
            "CONFIG_OBJZ requires clang but clang not found.\n"
            "Set -DOBJZ_CLANG_PATH=/path/to/clang/bin or ensure clang is in PATH.")
    endif()

    execute_process(
        COMMAND ${OBJZ_CLANG_COMPILER} --version
        OUTPUT_VARIABLE _ver OUTPUT_STRIP_TRAILING_WHITESPACE)
    string(REGEX MATCH "[0-9]+\\.[0-9]+" _ver "${_ver}")
    message(STATUS "Objective-Z: Using Clang ${_ver} for ObjC: ${OBJZ_CLANG_COMPILER}")

    set(OBJZ_CLANG_COMPILER ${OBJZ_CLANG_COMPILER} CACHE INTERNAL
        "Clang compiler for Objective-C")
endfunction()

# ─── Map Zephyr CPU config to LLVM target triple ────────────────────
function(_objz_get_clang_target_triple result)
    if(CONFIG_CPU_CORTEX_M0 OR CONFIG_CPU_CORTEX_M0PLUS OR CONFIG_CPU_CORTEX_M1)
        set(_triple "armv6m-none-eabi")
    elseif(CONFIG_CPU_CORTEX_M3)
        set(_triple "armv7m-none-eabi")
    elseif(CONFIG_CPU_CORTEX_M4 OR CONFIG_CPU_CORTEX_M7)
        if(CONFIG_FPU)
            set(_triple "armv7em-none-eabihf")
        else()
            set(_triple "armv7em-none-eabi")
        endif()
    elseif(CONFIG_CPU_CORTEX_M23)
        set(_triple "armv8m.base-none-eabi")
    elseif(CONFIG_CPU_CORTEX_M33 OR CONFIG_CPU_CORTEX_M55 OR CONFIG_CPU_CORTEX_M85)
        if(CONFIG_FPU)
            set(_triple "armv8m.main-none-eabihf")
        else()
            set(_triple "armv8m.main-none-eabi")
        endif()
    elseif(CONFIG_CPU_CORTEX_A53 OR CONFIG_CPU_CORTEX_A55
           OR CONFIG_CPU_CORTEX_A72 OR CONFIG_CPU_CORTEX_A76)
        set(_triple "aarch64-none-elf")
    else()
        message(FATAL_ERROR
            "Objective-Z: Unsupported CPU for Clang compilation. "
            "Add your CPU to _objz_get_clang_target_triple() in ObjcClang.cmake.")
    endif()

    set(${result} ${_triple} PARENT_SCOPE)
endfunction()

# ─── Build Clang flags from zephyr_interface ─────────────────────────
function(_objz_build_clang_flags result_var)
    set(_flags "")

    # Target triple
    _objz_get_clang_target_triple(_triple)
    list(APPEND _flags --target=${_triple})

    # CPU
    if(DEFINED GCC_M_CPU)
        list(APPEND _flags -mcpu=${GCC_M_CPU})
    endif()

    # Thumb mode
    if(CONFIG_COMPILER_ISA_THUMB2)
        list(APPEND _flags -mthumb)
    endif()

    # FPU
    if(CONFIG_FPU AND DEFINED GCC_M_FPU)
        list(APPEND _flags -mfpu=${GCC_M_FPU})
        if(CONFIG_FP_HARDABI)
            list(APPEND _flags -mfloat-abi=hard)
        elseif(CONFIG_FP_SOFTABI)
            list(APPEND _flags -mfloat-abi=softfp)
        endif()
    elseif(NOT CONFIG_FPU AND "${ARCH}" STREQUAL "arm")
        list(APPEND _flags -mfpu=none -mfloat-abi=soft)
    endif()

    # ObjC runtime: GNUstep 2.0 (section-based metadata, __objc_load entry point)
    list(APPEND _flags -fobjc-runtime=gnustep-2.0)
    list(APPEND _flags -fconstant-string-class=OZString)

    # Blocks (closures) support
    if(CONFIG_OBJZ_BLOCKS)
        list(APPEND _flags -fblocks)
    endif()

    # Include dirs from zephyr_interface (skip generator expressions)
    get_property(_inc_dirs TARGET zephyr_interface
        PROPERTY INTERFACE_INCLUDE_DIRECTORIES)
    foreach(_dir ${_inc_dirs})
        string(FIND "${_dir}" "$<" _is_genexpr)
        if(_is_genexpr EQUAL -1)
            list(APPEND _flags -I${_dir})
        endif()
    endforeach()

    # System include dirs (skip generator expressions and GCC built-in paths)
    get_property(_sys_inc_dirs TARGET zephyr_interface
        PROPERTY INTERFACE_SYSTEM_INCLUDE_DIRECTORIES)
    foreach(_dir ${_sys_inc_dirs})
        string(FIND "${_dir}" "$<" _is_genexpr)
        string(FIND "${_dir}" "lib/gcc/" _is_gcc)
        if(_is_genexpr EQUAL -1 AND _is_gcc EQUAL -1)
            list(APPEND _flags -isystem ${_dir})
        endif()
    endforeach()

    # Compile definitions from zephyr_interface
    get_property(_defs TARGET zephyr_interface
        PROPERTY INTERFACE_COMPILE_DEFINITIONS)
    foreach(_def ${_defs})
        string(FIND "${_def}" "$<" _is_genexpr)
        if(_is_genexpr EQUAL -1)
            list(APPEND _flags -D${_def})
        endif()
    endforeach()

    # autoconf.h (all CONFIG_* defines) — use AUTOCONF_H set by Zephyr
    if(DEFINED AUTOCONF_H)
        list(APPEND _flags -imacros ${AUTOCONF_H})
    endif()

    # Clang built-in headers (stddef.h, stdint.h, etc.) needed with -nostdinc
    execute_process(
        COMMAND ${OBJZ_CLANG_COMPILER} --target=${_triple} -print-resource-dir
        OUTPUT_VARIABLE _resource_dir OUTPUT_STRIP_TRAILING_WHITESPACE)
    list(APPEND _flags -isystem ${_resource_dir}/include)

    # SDK sysroot libc headers (string.h, stdlib.h, etc.)
    # GCC gets these via --sysroot and -specs=picolibc.specs; Clang needs them explicitly.
    if(SYSROOT_DIR)
        list(APPEND _flags
            -isystem ${SYSROOT_DIR}/include
            -isystem ${SYSROOT_DIR}/sys-include
        )
    elseif(CMAKE_SYSROOT)
        list(APPEND _flags
            -isystem ${CMAKE_SYSROOT}/include
            -isystem ${CMAKE_SYSROOT}/sys-include
        )
    endif()

    # Common flags (Clang-compatible subset of Zephyr's GCC flags)
    list(APPEND _flags
        -nostdinc
        -fshort-enums
        -fno-common
        -ffunction-sections
        -fdata-sections
        -fno-strict-aliasing
        -fno-pic
        -fno-pie
        -fno-asynchronous-unwind-tables
        -fno-exceptions
        -fno-unwind-tables
        -Os
        -g -gdwarf-4
        -Wall
        -Wno-objc-macro-redefinition
    )

    set(${result_var} ${_flags} PARENT_SCOPE)
endfunction()

# ─── Compile ObjC sources ────────────────────────────────────────────
#
# objz_compile_objc_sources(<target> <source1.m> [source2.m ...])
#
# Compiles each .m with Clang and adds the resulting .o to the target.
#
function(objz_compile_objc_sources target)
    _objz_build_clang_flags(_clang_flags)
    set(_all_objects "")

    foreach(_src ${ARGN})
        get_filename_component(_name ${_src} NAME)
        get_filename_component(_abs  ${_src} ABSOLUTE)

        # Unique output path
        string(MAKE_C_IDENTIFIER "${_name}" _safe_name)
        set(_obj ${CMAKE_CURRENT_BINARY_DIR}/clang_objc/${_safe_name}.o)

        # Gather target-specific include dirs
        get_target_property(_target_incs ${target} INCLUDE_DIRECTORIES)
        set(_extra_incs "")
        if(_target_incs)
            foreach(_dir ${_target_incs})
                string(FIND "${_dir}" "$<" _is_genexpr)
                if(_is_genexpr EQUAL -1)
                    list(APPEND _extra_incs -I${_dir})
                endif()
            endforeach()
        endif()

        add_custom_command(
            OUTPUT  ${_obj}
            COMMAND ${CMAKE_COMMAND} -E make_directory
                    ${CMAKE_CURRENT_BINARY_DIR}/clang_objc
            COMMAND ${OBJZ_CLANG_COMPILER}
                    ${_clang_flags}
                    ${_extra_incs}
                    -c ${_abs}
                    -o ${_obj}
            DEPENDS ${_abs}
            COMMENT "Clang ObjC: ${_name}"
            COMMAND_EXPAND_LISTS
            VERBATIM
        )

        list(APPEND _all_objects ${_obj})
    endforeach()

    target_sources(${target} PRIVATE ${_all_objects})
    set_source_files_properties(${_all_objects} PROPERTIES
        EXTERNAL_OBJECT TRUE
        GENERATED       TRUE
    )

    # CMake cannot determine linker language when a target only has external
    # objects (.o). Force it to C so GCC is used for linking.
    set_target_properties(${target} PROPERTIES LINKER_LANGUAGE C)
endfunction()

# ─── Compile ObjC ARC sources ─────────────────────────────────────────
#
# objz_compile_objc_arc_sources(<target> <source1.m> [source2.m ...])
#
# Same as objz_compile_objc_sources but adds -fobjc-arc.
#
function(objz_compile_objc_arc_sources target)
    _objz_build_clang_flags(_clang_flags)
    list(APPEND _clang_flags -fobjc-arc)
    set(_all_objects "")

    foreach(_src ${ARGN})
        get_filename_component(_name ${_src} NAME)
        get_filename_component(_abs  ${_src} ABSOLUTE)

        string(MAKE_C_IDENTIFIER "${_name}" _safe_name)
        set(_obj ${CMAKE_CURRENT_BINARY_DIR}/clang_objc_arc/${_safe_name}.o)

        get_target_property(_target_incs ${target} INCLUDE_DIRECTORIES)
        set(_extra_incs "")
        if(_target_incs)
            foreach(_dir ${_target_incs})
                string(FIND "${_dir}" "$<" _is_genexpr)
                if(_is_genexpr EQUAL -1)
                    list(APPEND _extra_incs -I${_dir})
                endif()
            endforeach()
        endif()

        add_custom_command(
            OUTPUT  ${_obj}
            COMMAND ${CMAKE_COMMAND} -E make_directory
                    ${CMAKE_CURRENT_BINARY_DIR}/clang_objc_arc
            COMMAND ${OBJZ_CLANG_COMPILER}
                    ${_clang_flags}
                    ${_extra_incs}
                    -c ${_abs}
                    -o ${_obj}
            DEPENDS ${_abs}
            COMMENT "Clang ObjC ARC: ${_name}"
            COMMAND_EXPAND_LISTS
            VERBATIM
        )

        list(APPEND _all_objects ${_obj})
    endforeach()

    target_sources(${target} PRIVATE ${_all_objects})
    set_source_files_properties(${_all_objects} PROPERTIES
        EXTERNAL_OBJECT TRUE
        GENERATED       TRUE
    )
    set_target_properties(${target} PROPERTIES LINKER_LANGUAGE C)
endfunction()

# ─── Public API for samples ──────────────────────────────────────────
#
# objz_target_sources(<target> <source1> [source2 ...])
#
# Routes .m -> objz_compile_objc_arc_sources (Clang + -fobjc-arc)
# Routes .c -> target_sources (always GCC)
#
function(objz_target_sources target)
    set(_m_sources "")
    set(_c_sources "")

    foreach(_src ${ARGN})
        get_filename_component(_ext ${_src} EXT)
        if("${_ext}" STREQUAL ".m")
            list(APPEND _m_sources ${_src})
        else()
            list(APPEND _c_sources ${_src})
        endif()
    endforeach()

    foreach(_src ${_c_sources})
        target_sources(${target} PRIVATE ${_src})
    endforeach()

    if(_m_sources)
        objz_compile_objc_arc_sources(${target} ${_m_sources})
        _objz_generate_table_sizes(${target} TRUE ${_m_sources})
        if(CONFIG_OBJZ_STATIC_POOLS)
            _objz_generate_pools_impl(${target} TRUE ${_m_sources})
        endif()
    endif()
endfunction()

# ─── Build Clang flags for AST analysis (host-compatible) ───────────
#
# The AST dump only needs include paths, defines, and ObjC parsing.
# Architecture/codegen flags and -fobjc-runtime=gnustep-2.0 are
# omitted because Apple Clang may crash with -ast-dump=json when
# cross-compiling with gnustep-2.0.
#
function(_objz_build_ast_flags result_var)
    set(_flags "")

    list(APPEND _flags -fconstant-string-class=OZString)

    # Include dirs from zephyr_interface (skip generator expressions)
    get_property(_inc_dirs TARGET zephyr_interface
        PROPERTY INTERFACE_INCLUDE_DIRECTORIES)
    foreach(_dir ${_inc_dirs})
        string(FIND "${_dir}" "$<" _is_genexpr)
        if(_is_genexpr EQUAL -1)
            list(APPEND _flags -I${_dir})
        endif()
    endforeach()

    # System include dirs (skip generator expressions and GCC built-in paths)
    get_property(_sys_inc_dirs TARGET zephyr_interface
        PROPERTY INTERFACE_SYSTEM_INCLUDE_DIRECTORIES)
    foreach(_dir ${_sys_inc_dirs})
        string(FIND "${_dir}" "$<" _is_genexpr)
        string(FIND "${_dir}" "lib/gcc/" _is_gcc)
        if(_is_genexpr EQUAL -1 AND _is_gcc EQUAL -1)
            list(APPEND _flags -isystem ${_dir})
        endif()
    endforeach()

    # Compile definitions from zephyr_interface
    get_property(_defs TARGET zephyr_interface
        PROPERTY INTERFACE_COMPILE_DEFINITIONS)
    foreach(_def ${_defs})
        string(FIND "${_def}" "$<" _is_genexpr)
        if(_is_genexpr EQUAL -1)
            list(APPEND _flags -D${_def})
        endif()
    endforeach()

    if(DEFINED AUTOCONF_H)
        list(APPEND _flags -imacros ${AUTOCONF_H})
    endif()

    # Clang built-in headers (stddef.h, stdint.h, etc.)
    execute_process(
        COMMAND ${OBJZ_CLANG_COMPILER} -print-resource-dir
        OUTPUT_VARIABLE _resource_dir OUTPUT_STRIP_TRAILING_WHITESPACE)
    list(APPEND _flags -isystem ${_resource_dir}/include)

    # SDK sysroot libc headers
    if(SYSROOT_DIR)
        list(APPEND _flags
            -isystem ${SYSROOT_DIR}/include
            -isystem ${SYSROOT_DIR}/sys-include
        )
    elseif(CMAKE_SYSROOT)
        list(APPEND _flags
            -isystem ${CMAKE_SYSROOT}/include
            -isystem ${CMAKE_SYSROOT}/sys-include
        )
    endif()

    list(APPEND _flags
        -nostdinc
        -fshort-enums
        -Wall
        -Wno-objc-macro-redefinition
    )

    set(${result_var} ${_flags} PARENT_SCOPE)
endfunction()

# ─── Internal: generate table sizes header via tree-sitter ───────────
#
# Parses all .m files (Foundation + user) directly with tree-sitter.
# No Clang AST dumps needed — fast source-level structural analysis.
#
# Accumulates user .m files across multiple objz_target_sources() calls
# and defers target creation until all sources are collected.
#
function(_objz_generate_table_sizes target use_arc)
    # Accumulate user .m files across multiple calls
    foreach(_src ${ARGN})
        get_filename_component(_abs ${_src} ABSOLUTE)
        set_property(GLOBAL APPEND PROPERTY OBJZ_USER_M_FILES ${_abs})
    endforeach()

    # Defer actual target creation until all sources are accumulated
    get_property(_deferred GLOBAL PROPERTY _OBJZ_TABLE_SIZES_DEFERRED)
    if(NOT _deferred)
        set_property(GLOBAL PROPERTY _OBJZ_TABLE_SIZES_DEFERRED TRUE)
        cmake_language(DEFER DIRECTORY ${CMAKE_SOURCE_DIR}
            CALL _objz_create_table_sizes_target)
    endif()
endfunction()

function(_objz_create_table_sizes_target)
    get_property(_foundation_files GLOBAL PROPERTY OBJZ_FOUNDATION_M_FILES)
    get_property(_user_files GLOBAL PROPERTY OBJZ_USER_M_FILES)
    set(_all_m_files ${_foundation_files} ${_user_files})

    _objz_get_clang_target_triple(_triple)
    if("${_triple}" MATCHES "aarch64")
        set(_ptr_size 8)
    else()
        set(_ptr_size 4)
    endif()

    get_property(_gen_dir GLOBAL PROPERTY OBJZ_GENERATED_DIR)
    set(_ts_header ${_gen_dir}/objc/table_sizes.h)
    set(_gen_script ${ZEPHYR_EXTRA_MODULES}/scripts/objz_gen_table_sizes.py)

    # Per-class dtable pool generation when dispatch cache is enabled
    set(_dtable_flag "")
    set(_dtable_outputs "")
    if(CONFIG_OBJZ_DISPATCH_CACHE)
        set(_dtable_c ${_gen_dir}/dtable_pool.c)
        set(_dtable_flag --dtable-output=${_dtable_c})
        set(_dtable_outputs ${_dtable_c})
    endif()

    add_custom_command(
        OUTPUT  ${_ts_header} ${_dtable_outputs}
        COMMAND ${CMAKE_COMMAND} -E make_directory ${_gen_dir}/objc
        COMMAND ${Python3_EXECUTABLE} ${_gen_script}
                --pointer-size=${_ptr_size}
                --output=${_ts_header}
                ${_dtable_flag}
                ${_all_m_files}
        DEPENDS ${_all_m_files} ${_gen_script}
        COMMENT "Generating table sizes (tree-sitter)"
        VERBATIM
    )

    add_custom_target(objz_table_sizes DEPENDS ${_ts_header} ${_dtable_outputs})
    get_property(_lib_target GLOBAL PROPERTY OBJZ_LIBRARY_TARGET)
    if(_lib_target)
        add_dependencies(${_lib_target} objz_table_sizes)
        if(CONFIG_OBJZ_DISPATCH_CACHE AND _dtable_c)
            target_sources(${_lib_target} PRIVATE ${_dtable_c})
        endif()
    endif()
endfunction()

# ─── Internal: generate pools from Clang AST ────────────────────────
#
# Each call produces a uniquely-named generated_pools_N.c so that
# multiple calls per target (e.g. MRR + ARC sources) do not conflict.
# Each generated file defines pools only for classes DEFINED in the
# analyzed sources (size info required).
#
function(_objz_generate_pools_impl target use_arc)
    _objz_build_ast_flags(_ast_flags)
    if(use_arc)
        list(APPEND _ast_flags -fobjc-arc)
    endif()
    set(_ast_files "")

    foreach(_src ${ARGN})
        get_filename_component(_name ${_src} NAME)
        get_filename_component(_abs  ${_src} ABSOLUTE)
        string(MAKE_C_IDENTIFIER "${_name}" _safe)
        set(_ast ${CMAKE_CURRENT_BINARY_DIR}/clang_ast/${_safe}.ast.json)

        get_target_property(_target_incs ${target} INCLUDE_DIRECTORIES)
        set(_extra "")
        if(_target_incs)
            foreach(_dir ${_target_incs})
                string(FIND "${_dir}" "$<" _g)
                if(_g EQUAL -1)
                    list(APPEND _extra -I${_dir})
                endif()
            endforeach()
        endif()

        # Clang may report errors for host-incompatible ELF section
        # attributes in Zephyr headers, but the JSON AST output is still
        # valid for ObjC analysis.  The '|| true' ignores those errors.
        add_custom_command(
            OUTPUT  ${_ast}
            COMMAND ${CMAKE_COMMAND} -E make_directory
                    ${CMAKE_CURRENT_BINARY_DIR}/clang_ast
            COMMAND ${OBJZ_CLANG_COMPILER}
                    ${_ast_flags} ${_extra}
                    -fsyntax-only -Xclang -ast-dump=json
                    ${_abs} > ${_ast} || true
            DEPENDS ${_abs}
            COMMENT "Clang AST dump: ${_name}"
        )
        list(APPEND _ast_files ${_ast})
    endforeach()

    _objz_get_clang_target_triple(_triple)
    if("${_triple}" MATCHES "aarch64")
        set(_ptr_size 8)
    else()
        set(_ptr_size 4)
    endif()

    # Unique output name per call to avoid duplicate custom command errors
    get_target_property(_pool_gen_count ${target} _OBJZ_POOL_GEN_COUNT)
    if(NOT _pool_gen_count)
        set(_pool_gen_count 0)
    endif()
    math(EXPR _pool_gen_count "${_pool_gen_count} + 1")
    set_target_properties(${target} PROPERTIES
        _OBJZ_POOL_GEN_COUNT ${_pool_gen_count})

    set(_pools_c ${CMAKE_CURRENT_BINARY_DIR}/generated_pools_${_pool_gen_count}.c)
    set(_gen_script ${ZEPHYR_EXTRA_MODULES}/scripts/objz_gen_pools.py)

    add_custom_command(
        OUTPUT  ${_pools_c}
        COMMAND ${Python3_EXECUTABLE} ${_gen_script}
                --pointer-size=${_ptr_size}
                --output=${_pools_c}
                ${_ast_files}
        DEPENDS ${_ast_files} ${_gen_script}
        COMMENT "Generating pools from AST analysis"
        VERBATIM
    )

    target_sources(${target} PRIVATE ${_pools_c})
endfunction()

# ─── Generate static pools from Clang AST analysis ──────────────────
#
# objz_generate_pools(<target> <source1.m> [source2.m ...])
#
# Explicit call for MRR (manual retain/release) sources.
#
function(objz_generate_pools target)
    _objz_generate_pools_impl(${target} FALSE ${ARGN})
endfunction()

# ─── Generate static pools from Clang AST (ARC sources) ─────────────
#
# objz_generate_arc_pools(<target> <source1.m> [source2.m ...])
#
# Same as objz_generate_pools but adds -fobjc-arc for the AST dump.
#
function(objz_generate_arc_pools target)
    _objz_generate_pools_impl(${target} TRUE ${ARGN})
endfunction()
