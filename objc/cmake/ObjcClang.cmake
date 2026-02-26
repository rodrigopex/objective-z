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

    # ObjC runtime: GNUstep 1.7 (ABI compatible with GCC v8 __objc_exec_class,
    # but uses objc_msgSend dispatch and supports ARC)
    list(APPEND _flags -fobjc-runtime=gnustep-1.7)
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
# Routes .m -> objz_compile_objc_sources (Clang or GCC)
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
        objz_compile_objc_sources(${target} ${_m_sources})
    endif()
endfunction()

# ─── Public API for ARC samples ──────────────────────────────────────
#
# objz_target_arc_sources(<target> <source1> [source2 ...])
#
# Routes .m -> objz_compile_objc_arc_sources (Clang + -fobjc-arc)
# Routes .c -> target_sources (always GCC)
#
function(objz_target_arc_sources target)
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
    endif()
endfunction()
