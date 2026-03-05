# SPDX-License-Identifier: Apache-2.0
#
# ZigCompile.cmake — Compile Zig (.zig) files with zig build-obj while
# linking with GCC. Pattern follows ObjcClang.cmake.

include_guard(GLOBAL)

# ─── Find Zig ────────────────────────────────────────────────────────
function(zig_find_compiler)
    find_program(ZIG_COMPILER zig)

    if(NOT ZIG_COMPILER)
        message(FATAL_ERROR
            "Zig benchmark requires zig but zig not found.\n"
            "Install Zig or ensure zig is in PATH.")
    endif()

    execute_process(
        COMMAND ${ZIG_COMPILER} version
        OUTPUT_VARIABLE _ver OUTPUT_STRIP_TRAILING_WHITESPACE)
    message(STATUS "Zig: Using Zig ${_ver}: ${ZIG_COMPILER}")

    set(ZIG_COMPILER ${ZIG_COMPILER} CACHE INTERNAL "Zig compiler")
endfunction()

# ─── Map Zephyr CPU config to Zig target triple ──────────────────────
function(_zig_get_target_triple result)
    if(CONFIG_CPU_CORTEX_M0 OR CONFIG_CPU_CORTEX_M0PLUS OR CONFIG_CPU_CORTEX_M1)
        set(_triple "thumb-freestanding-eabi")
    elseif(CONFIG_CPU_CORTEX_M3)
        set(_triple "thumb-freestanding-eabi")
    elseif(CONFIG_CPU_CORTEX_M4 OR CONFIG_CPU_CORTEX_M7)
        set(_triple "thumb-freestanding-eabi")
    elseif(CONFIG_CPU_CORTEX_M23 OR CONFIG_CPU_CORTEX_M33
           OR CONFIG_CPU_CORTEX_M55 OR CONFIG_CPU_CORTEX_M85)
        set(_triple "thumb-freestanding-eabi")
    elseif(CONFIG_RISCV)
        if(CONFIG_64BIT)
            set(_triple "riscv64-freestanding-eabi")
        else()
            set(_triple "riscv32-freestanding-eabi")
        endif()
    else()
        message(FATAL_ERROR
            "ZigCompile: Unsupported CPU. "
            "Add your CPU to _zig_get_target_triple() in ZigCompile.cmake.")
    endif()

    set(${result} ${_triple} PARENT_SCOPE)
endfunction()

# ─── Map Zephyr CPU config to Zig -mcpu value ────────────────────────
function(_zig_get_mcpu result)
    if(CONFIG_CPU_CORTEX_M0)
        set(_cpu "cortex_m0")
    elseif(CONFIG_CPU_CORTEX_M0PLUS)
        set(_cpu "cortex_m0plus")
    elseif(CONFIG_CPU_CORTEX_M3)
        set(_cpu "cortex_m3")
    elseif(CONFIG_CPU_CORTEX_M4)
        set(_cpu "cortex_m4")
    elseif(CONFIG_CPU_CORTEX_M7)
        set(_cpu "cortex_m7")
    elseif(CONFIG_CPU_CORTEX_M33)
        set(_cpu "cortex_m33")
    elseif(CONFIG_RISCV)
        set(_cpu "generic_rv32")
        if(CONFIG_RISCV_ISA_EXT_M)
            string(APPEND _cpu "+m")
        endif()
        if(CONFIG_RISCV_ISA_EXT_A)
            string(APPEND _cpu "+a")
        endif()
        if(CONFIG_RISCV_ISA_EXT_C)
            string(APPEND _cpu "+c")
        endif()
    else()
        set(_cpu "baseline")
    endif()

    set(${result} ${_cpu} PARENT_SCOPE)
endfunction()

# ─── Build Zig flags from zephyr_interface ────────────────────────────
function(_zig_build_flags result_var)
    set(_flags "")

    _zig_get_target_triple(_triple)
    _zig_get_mcpu(_mcpu)

    list(APPEND _flags -target ${_triple})
    list(APPEND _flags -mcpu ${_mcpu})

    # Optimization
    list(APPEND _flags -O ReleaseSafe)

    # Include dirs from zephyr_interface (for @cImport)
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

    # autoconf.h: Zig doesn't support -include, so we @cInclude it in Zig source.
    # Ensure its directory is in the include path.
    if(DEFINED AUTOCONF_H)
        get_filename_component(_autoconf_dir ${AUTOCONF_H} DIRECTORY)
        list(APPEND _flags -I${_autoconf_dir})
    endif()

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

    set(${result_var} ${_flags} PARENT_SCOPE)
endfunction()

# ─── Compile Zig sources ─────────────────────────────────────────────
#
# zig_compile_sources(<target> <source1.zig> [source2.zig ...])
#
# Compiles each .zig with zig build-obj and adds the .o to the target.
#
function(zig_compile_sources target)
    _zig_build_flags(_zig_flags)
    set(_all_objects "")

    foreach(_src ${ARGN})
        get_filename_component(_name ${_src} NAME_WE)
        get_filename_component(_abs  ${_src} ABSOLUTE)

        set(_obj ${CMAKE_CURRENT_BINARY_DIR}/zig_obj/${_name}.o)

        add_custom_command(
            OUTPUT  ${_obj}
            COMMAND ${CMAKE_COMMAND} -E make_directory
                    ${CMAKE_CURRENT_BINARY_DIR}/zig_obj
            COMMAND ${ZIG_COMPILER} build-obj
                    ${_zig_flags}
                    -femit-bin=${_obj}
                    ${_abs}
            DEPENDS ${_abs}
            COMMENT "Zig: ${_name}.zig"
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
