# SPDX-License-Identifier: Apache-2.0
#
# C3Compile.cmake — Compile C3 (.c3) files with c3c compile-only while
# linking with GCC. Pattern follows ZigCompile.cmake.

include_guard(GLOBAL)

# ─── Find C3 ────────────────────────────────────────────────────────
function(c3_find_compiler)
    find_program(C3_COMPILER c3c)

    if(NOT C3_COMPILER)
        message(FATAL_ERROR
            "C3 benchmark requires c3c but c3c not found.\n"
            "Install C3 or ensure c3c is in PATH.\n"
            "See https://c3-lang.org/getting-started/prebuilt-binaries/")
    endif()

    execute_process(
        COMMAND ${C3_COMPILER} --version
        OUTPUT_VARIABLE _ver OUTPUT_STRIP_TRAILING_WHITESPACE)
    # Extract just the version line
    string(REGEX MATCH "Version:[^\n]*" _ver_line "${_ver}")
    message(STATUS "C3: Using c3c ${_ver_line}: ${C3_COMPILER}")

    set(C3_COMPILER ${C3_COMPILER} CACHE INTERNAL "C3 compiler")
endfunction()

# ─── Map Zephyr CPU config to C3 target ──────────────────────────────
function(_c3_get_target result)
    if(CONFIG_RISCV)
        if(CONFIG_64BIT)
            set(_target "elf-riscv64")
        else()
            set(_target "elf-riscv32")
        endif()
    else()
        message(FATAL_ERROR
            "C3Compile: Unsupported CPU. C3 currently supports elf-riscv32/64 "
            "for freestanding targets. ARM Cortex-M is not yet supported.\n"
            "Add your CPU to _c3_get_target() in C3Compile.cmake when available.")
    endif()

    set(${result} ${_target} PARENT_SCOPE)
endfunction()

# ─── Map Zephyr CPU config to C3 --riscv-cpu value ───────────────────
function(_c3_get_riscv_cpu result)
    set(_cpu "rvi")

    if(CONFIG_RISCV_ISA_EXT_M AND CONFIG_RISCV_ISA_EXT_A AND CONFIG_RISCV_ISA_EXT_C)
        set(_cpu "rvimac")
    endif()

    set(${result} ${_cpu} PARENT_SCOPE)
endfunction()

# ─── Build C3 flags from zephyr_interface ─────────────────────────────
function(_c3_build_flags result_var)
    set(_flags "")

    _c3_get_target(_target)
    list(APPEND _flags --target ${_target})

    if(CONFIG_RISCV)
        _c3_get_riscv_cpu(_riscv_cpu)
        list(APPEND _flags --riscv-cpu=${_riscv_cpu})
        list(APPEND _flags --riscv-abi=int-only)
    endif()

    # Optimization: ReleaseSafe equivalent
    list(APPEND _flags -O1)

    # No main entry point (we provide our own via entry.c)
    list(APPEND _flags --no-entry)

    # Relocation model: no PIC for bare metal
    list(APPEND _flags --reloc=none)

    # No C3 stdlib or libc — we link against Zephyr
    list(APPEND _flags --use-stdlib=no --link-libc=no --emit-stdlib=no)

    set(${result_var} ${_flags} PARENT_SCOPE)
endfunction()

# ─── Compile C3 sources ─────────────────────────────────────────────
#
# c3_compile_sources(<target> <source1.c3> [source2.c3 ...])
#
# Compiles each .c3 with c3c compile-only and adds the .o to the target.
#
function(c3_compile_sources target)
    _c3_build_flags(_c3_flags)
    set(_all_objects "")

    foreach(_src ${ARGN})
        get_filename_component(_name ${_src} NAME_WE)
        get_filename_component(_abs  ${_src} ABSOLUTE)

        set(_obj_dir ${CMAKE_CURRENT_BINARY_DIR}/c3_obj)
        set(_obj ${_obj_dir}/${_name}.o)

        # c3c writes to obj/<target>/<module>.o under the working dir.
        # Extract module name from source to predict the output path.
        file(STRINGS ${_abs} _module_line REGEX "^module ")
        string(REGEX REPLACE "^module ([a-zA-Z0-9_]+).*" "\\1" _module "${_module_line}")

        _c3_get_target(_c3_target)
        set(_c3_out ${_obj_dir}/obj/${_c3_target}/${_module}.o)

        add_custom_command(
            OUTPUT  ${_obj}
            COMMAND ${CMAKE_COMMAND} -E make_directory ${_obj_dir}
            COMMAND ${C3_COMPILER} compile-only
                    ${_c3_flags}
                    --single-module=yes
                    ${_abs}
            COMMAND ${CMAKE_COMMAND} -E rename ${_c3_out} ${_obj}
            WORKING_DIRECTORY ${_obj_dir}
            DEPENDS ${_abs}
            COMMENT "C3: ${_name}.c3"
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
