# SPDX-License-Identifier: Apache-2.0
#
# ObjcClang.cmake — Clang toolchain support for Objective-Z transpiler.
#
# Provides Clang detection, target triple mapping, AST analysis flags,
# and compile_commands.json generation for clangd IDE support.

include_guard(GLOBAL)

# ─── Find Clang ──────────────────────────────────────────────────────
#
# Search order:
#   1. OBJZ_CLANG_PATH   (explicit user override)
#   2. Zephyr SDK LLVM   (from ZEPHYR_SDK_INSTALL_DIR — the tested default)
#   3. Homebrew LLVM      (macOS fallback for RISC-V / missing system clang)
#   4. System clang       (PATH — Apple Clang or distro LLVM)
#
# The Zephyr SDK LLVM Clang 19 is the tested reference toolchain.
# Other compilers work but emit a compatibility warning.
#
function(objz_find_clang)
    set(_oz_tested_clang_ver "19")

    # 1. Explicit user override
    if(DEFINED OBJZ_CLANG_PATH)
        find_program(OBJZ_CLANG_COMPILER clang
            PATHS ${OBJZ_CLANG_PATH} NO_DEFAULT_PATH)
    endif()

    # 2. Zephyr SDK LLVM (default)
    if(NOT OBJZ_CLANG_COMPILER AND DEFINED ZEPHYR_SDK_INSTALL_DIR)
        find_program(OBJZ_CLANG_COMPILER clang
            PATHS ${ZEPHYR_SDK_INSTALL_DIR}/llvm/bin
            NO_DEFAULT_PATH)
    endif()

    # 3. Homebrew LLVM (macOS — needed for RISC-V when Apple Clang lacks backend)
    if(NOT OBJZ_CLANG_COMPILER)
        find_program(OBJZ_CLANG_COMPILER clang
            PATHS /opt/homebrew/opt/llvm/bin
                  /usr/local/opt/llvm/bin
            NO_DEFAULT_PATH)
    endif()

    # 4. System clang in PATH
    if(NOT OBJZ_CLANG_COMPILER)
        find_program(OBJZ_CLANG_COMPILER clang)
    endif()

    if(NOT OBJZ_CLANG_COMPILER)
        message(FATAL_ERROR
            "CONFIG_OBJZ requires clang but clang not found.\n"
            "Install the Zephyr SDK (includes LLVM Clang), or set\n"
            "-DOBJZ_CLANG_PATH=/path/to/clang/bin")
    endif()

    # RISC-V target verification — Apple Clang lacks RISC-V backend
    if(CONFIG_RISCV)
        _objz_get_clang_target_triple(_check_triple)
        execute_process(
            COMMAND ${OBJZ_CLANG_COMPILER} --target=${_check_triple} -x c -c /dev/null
                    -o /dev/null
            RESULT_VARIABLE _target_result
            ERROR_QUIET OUTPUT_QUIET)
        if(NOT _target_result EQUAL 0)
            message(FATAL_ERROR
                "Objective-Z: ${OBJZ_CLANG_COMPILER} lacks RISC-V backend.\n"
                "Set -DOBJZ_CLANG_PATH to a Clang with RISC-V support\n"
                "(Zephyr SDK LLVM or brew install llvm).")
        endif()
    endif()

    # Version / compatibility check
    execute_process(
        COMMAND ${OBJZ_CLANG_COMPILER} --version
        OUTPUT_VARIABLE _ver_full OUTPUT_STRIP_TRAILING_WHITESPACE)
    string(REGEX MATCH "[0-9]+\\.[0-9]+" _ver "${_ver_full}")
    string(REGEX MATCH "^[0-9]+" _ver_major "${_ver}")

    # Detect whether this is the Zephyr SDK LLVM
    string(FIND "${OBJZ_CLANG_COMPILER}" "zephyr-sdk" _is_zsdk)
    if(_is_zsdk GREATER -1)
        message(STATUS "Objective-Z: Using Zephyr SDK Clang ${_ver}: ${OBJZ_CLANG_COMPILER}")
    else()
        message(WARNING
            "Objective-Z: Using non-Zephyr-SDK Clang ${_ver}: ${OBJZ_CLANG_COMPILER}\n"
            "The tested environment is Zephyr SDK LLVM Clang ${_oz_tested_clang_ver}. "
            "Other versions may produce different AST output. "
            "Set ZEPHYR_SDK_INSTALL_DIR or -DOBJZ_CLANG_PATH to use the Zephyr SDK LLVM.")
    endif()

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
    elseif(CONFIG_RISCV)
        if(CONFIG_64BIT)
            set(_triple "riscv64-unknown-elf")
        else()
            set(_triple "riscv32-unknown-elf")
        endif()
    else()
        message(FATAL_ERROR
            "Objective-Z: Unsupported CPU for Clang compilation. "
            "Add your CPU to _objz_get_clang_target_triple() in ObjcClang.cmake.")
    endif()

    set(${result} ${_triple} PARENT_SCOPE)
endfunction()

# ─── Append arch built-in defines for host-target compile_commands ────
#
# When compile_commands.json uses the host target instead of the
# cross-compilation target, compiler built-in arch defines (__ARM_*,
# __riscv*, __thumb*, etc.) are absent.  CMSIS and Zephyr headers need
# them, so we query Clang for the embedded target's predefined macros
# and inject them as -D flags.
#
function(_objz_append_arch_defines result_var)
    set(_flags ${${result_var}})

    # Reconstruct target + arch flags for the Clang predefines query
    _objz_get_clang_target_triple(_triple)
    set(_query_flags --target=${_triple})
    if(DEFINED GCC_M_CPU)
        list(APPEND _query_flags -mcpu=${GCC_M_CPU})
    endif()
    if(CONFIG_COMPILER_ISA_THUMB2)
        list(APPEND _query_flags -mthumb)
    endif()
    if(CONFIG_FPU AND DEFINED GCC_M_FPU)
        list(APPEND _query_flags -mfpu=${GCC_M_FPU})
        if(CONFIG_FP_HARDABI)
            list(APPEND _query_flags -mfloat-abi=hard)
        elseif(CONFIG_FP_SOFTABI)
            list(APPEND _query_flags -mfloat-abi=softfp)
        endif()
    elseif(NOT CONFIG_FPU AND "${ARCH}" STREQUAL "arm")
        list(APPEND _query_flags -mfpu=none -mfloat-abi=soft)
    endif()
    if(CONFIG_RISCV)
        set(_rv_march "rv32i")
        set(_rv_mabi "ilp32")
        if(CONFIG_64BIT)
            set(_rv_march "rv64i")
            set(_rv_mabi "lp64")
        endif()
        if(CONFIG_RISCV_ISA_EXT_M)
            string(APPEND _rv_march "m")
        endif()
        if(CONFIG_RISCV_ISA_EXT_A)
            string(APPEND _rv_march "a")
        endif()
        if(CONFIG_RISCV_ISA_EXT_C)
            string(APPEND _rv_march "c")
        endif()
        list(APPEND _query_flags -march=${_rv_march} -mabi=${_rv_mabi})
    endif()

    # Query Clang for all predefined macros of the embedded target
    execute_process(
        COMMAND ${OBJZ_CLANG_COMPILER} ${_query_flags} -dM -E -x c /dev/null
        OUTPUT_VARIABLE _predefs
        OUTPUT_STRIP_TRAILING_WHITESPACE
        ERROR_QUIET)

    # Extract arch-specific defines and inject as -D flags
    string(REPLACE "\n" ";" _lines "${_predefs}")
    foreach(_line IN LISTS _lines)
        if(_line MATCHES "^#define (__ARM_[A-Za-z0-9_]+) (.+)$")
            list(APPEND _flags "-D${CMAKE_MATCH_1}=${CMAKE_MATCH_2}")
        elseif(_line MATCHES "^#define (__riscv[A-Za-z0-9_]*) (.+)$")
            list(APPEND _flags "-D${CMAKE_MATCH_1}=${CMAKE_MATCH_2}")
        elseif(_line MATCHES "^#define (__thumb[A-Za-z0-9_]*) (.+)$")
            list(APPEND _flags "-D${CMAKE_MATCH_1}=${CMAKE_MATCH_2}")
        endif()
    endforeach()

    set(${result_var} ${_flags} PARENT_SCOPE)
endfunction()

# ─── Build Clang flags from zephyr_interface ─────────────────────────
#
# Used by oz_transpile.cmake for clangd compile_commands.json generation.
#
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

    # FPU (ARM)
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

    # RISC-V ISA and ABI
    if(CONFIG_RISCV)
        set(_rv_march "rv32i")
        set(_rv_mabi "ilp32")
        if(CONFIG_64BIT)
            set(_rv_march "rv64i")
            set(_rv_mabi "lp64")
        endif()
        if(CONFIG_RISCV_ISA_EXT_M)
            string(APPEND _rv_march "m")
        endif()
        if(CONFIG_RISCV_ISA_EXT_A)
            string(APPEND _rv_march "a")
        endif()
        if(CONFIG_RISCV_ISA_EXT_C)
            string(APPEND _rv_march "c")
        endif()
        list(APPEND _flags -march=${_rv_march} -mabi=${_rv_mabi} -mno-relax)
    endif()

    # ObjC runtime (for clangd IDE support)
    list(APPEND _flags -fobjc-runtime=gnustep-2.0)
    list(APPEND _flags -fconstant-string-class=OZString)
    list(APPEND _flags -fblocks)

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

# ─── Collect compile_commands.json entry for ObjC files ──────────────
#
# _objz_collect_compile_db(<source.m> <output.o> <flag1> [flag2 ...])
#
# Appends a JSON entry to the OBJZ_COMPILE_DB_JSON global property.
# Registers a deferred function to write compile_commands_objc.json
# and create a merge target (once).
#
function(_objz_collect_compile_db source object)
    set(_args ${OBJZ_CLANG_COMPILER})
    list(APPEND _args ${ARGN})
    list(APPEND _args -c ${source} -o ${object})

    set(_json_args "")
    foreach(_arg IN LISTS _args)
        string(REPLACE "\\" "\\\\" _arg "${_arg}")
        string(REPLACE "\"" "\\\"" _arg "${_arg}")
        if(_json_args)
            string(APPEND _json_args ", ")
        endif()
        string(APPEND _json_args "\"${_arg}\"")
    endforeach()

    set_property(GLOBAL APPEND_STRING PROPERTY OBJZ_COMPILE_DB_JSON
        "{\"directory\": \"${CMAKE_BINARY_DIR}\", \"file\": \"${source}\", \"arguments\": [${_json_args}]},\n")
    set_property(GLOBAL APPEND PROPERTY _OBJZ_COLLECTED_M_FILES "${source}")

    get_property(_deferred GLOBAL PROPERTY _OBJZ_COMPILE_DB_DEFERRED)
    if(NOT _deferred)
        set_property(GLOBAL PROPERTY _OBJZ_COMPILE_DB_DEFERRED TRUE)
        set_property(GLOBAL PROPERTY _OBJZ_MODULE_DIR ${ZEPHYR_OBJZ_MODULE_DIR})
        cmake_language(DEFER DIRECTORY ${CMAKE_SOURCE_DIR}
            CALL _objz_write_compile_db)
    endif()
endfunction()

function(_objz_write_compile_db)
    get_property(_json GLOBAL PROPERTY OBJZ_COMPILE_DB_JSON)
    if(NOT _json)
        return()
    endif()

    get_property(_mod GLOBAL PROPERTY _OBJZ_MODULE_DIR)
    get_filename_component(_mod "${_mod}" REALPATH)

    # Scan all .m files in the project and add compile_commands entries
    # for any not already collected.  clangd strips -f flags during
    # interpolation, so every .m file needs an explicit entry.
    get_property(_collected_files GLOBAL PROPERTY _OBJZ_COLLECTED_M_FILES)
    file(GLOB_RECURSE _all_m_files
        "${_mod}/samples/*.m"
        "${_mod}/tests/*.m"
        "${_mod}/benchmarks/*.m"
        "${_mod}/src/*.m")
    # Extract the template arguments from the first collected entry
    # (everything between "arguments": [...] in the JSON).
    string(REGEX MATCH "\"arguments\": \\[([^]]+)\\]" _ "${_json}")
    set(_template_args "${CMAKE_MATCH_1}")
    if(_template_args AND _all_m_files)
        foreach(_m_file ${_all_m_files})
            get_filename_component(_m_file "${_m_file}" REALPATH)
            list(FIND _collected_files "${_m_file}" _idx)
            if(_idx EQUAL -1)
                # Build a synthetic entry reusing the template args but
                # replacing the source and object paths.
                string(MAKE_C_IDENTIFIER "${_m_file}" _safe)
                set(_obj "${CMAKE_BINARY_DIR}/clang_objc_arc/${_safe}.o")
                # Escape for JSON
                string(REPLACE "\\" "\\\\" _m_esc "${_m_file}")
                string(REPLACE "\"" "\\\"" _m_esc "${_m_esc}")
                string(REPLACE "\\" "\\\\" _obj_esc "${_obj}")
                string(REPLACE "\"" "\\\"" _obj_esc "${_obj_esc}")
                string(APPEND _json
                    "{\"directory\": \"${CMAKE_BINARY_DIR}\", \"file\": \"${_m_esc}\", \"arguments\": [${_template_args}]},\n")
            endif()
        endforeach()
    endif()

    string(REGEX REPLACE ",\n$" "\n" _json "${_json}")
    file(WRITE "${CMAKE_BINARY_DIR}/compile_commands_objc.json" "[\n${_json}]\n")

    add_custom_target(objz_compile_db ALL
        COMMAND ${Python3_EXECUTABLE}
                ${_mod}/scripts/objz_merge_compile_db.py
                ${CMAKE_BINARY_DIR}/compile_commands.json
                ${CMAKE_BINARY_DIR}/compile_commands_objc.json
        COMMENT "ObjZ: merging ObjC entries into compile_commands.json"
        VERBATIM
    )

    # Generate minimal .clangd at the app project root.  compile_commands
    # entries use the host target (macOS) for ObjC blocks/ARC support, so
    # ELF section attributes from Zephyr macros trigger a Mach-O error
    # that can only be suppressed via clangd's Diagnostics Suppress (it's
    # a hard error, not a -Wno-suppressible warning).
    set(_clangd_path "${CMAKE_SOURCE_DIR}/.clangd")
    file(WRITE "${_clangd_path}"
"# Auto-generated by Objective-Z module — do not edit manually.\n\
# Regenerated on every CMake configure (just build / just rebuild).\n\
\n\
CompileFlags:\n\
\tCompilationDatabase: build\n\
\n\
Diagnostics:\n\
\tSuppress:\n\
\t\t- attribute_section_invalid_for_target\n")
    message(STATUS "Objective-Z: generated ${_clangd_path}")

    message(STATUS "Objective-Z: wrote ${CMAKE_BINARY_DIR}/compile_commands_objc.json")
endfunction()

# ─── Build Clang flags for AST analysis (host-compatible) ───────────
#
# The AST dump only needs include paths, defines, and ObjC parsing.
# Uses -fobjc-runtime=macosx so both Apple Clang and LLVM Clang
# (Zephyr SDK) produce valid ObjC AST.  gnustep-2.0 is avoided
# because Apple Clang may crash with -ast-dump=json.
# -fblocks is required for LLVM Clang (Apple Clang enables it
# implicitly); without it, block syntax produces RecoveryExpr nodes.
#
function(_objz_build_ast_flags result_var)
    set(_flags "")

    list(APPEND _flags -fobjc-runtime=macosx)
    list(APPEND _flags -fconstant-string-class=OZString)
    list(APPEND _flags -fobjc-arc)
    list(APPEND _flags -fblocks)

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
