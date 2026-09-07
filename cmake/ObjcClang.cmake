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
# Other compilers work but emit a compatibility warning — or a hard
# error, with -DOBJZ_REQUIRE_TESTED_CLANG=ON.
#
# Set that in CI. A warning here is indistinguishable from silence: for
# the whole life of this project's CI the SDK was installed without its
# LLVM component, so step 2 found nothing, step 4 picked Ubuntu's
# preinstalled `/usr/bin/clang`, and every run printed
#
#   Objective-Z: Using non-Zephyr-SDK Clang 18.1: /usr/bin/clang
#
# into a 1400-line log that nobody read. The AST facts oz_static relies
# on for ivar ownership and method definedness were being produced by
# clang 18.1 while the project was tested against 19 — and while CI
# separately installed clang 20 and never used it. Anything that decides
# whether generated code is correct should not degrade quietly; that is
# oz_static's own standing rule, and this is the build applying it to
# itself.
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

    # Two independent questions, and only the first used to be asked.
    #
    # Vendor: is this the SDK's LLVM? Version: is it the major version the
    # AST facts were validated against? The old check tested the path for
    # "zephyr-sdk" and stopped, computing `_ver_major` and never using it
    # — so an SDK carrying a different clang would have reported "Using
    # Zephyr SDK Clang 21" with no complaint at all, which is the one case
    # `_oz_tested_clang_ver` exists for.
    string(FIND "${OBJZ_CLANG_COMPILER}" "zephyr-sdk" _is_zsdk)
    if(_is_zsdk GREATER -1)
        set(_oz_clang_source "Zephyr SDK")
    else()
        set(_oz_clang_source "non-Zephyr-SDK")
    endif()

    if(_is_zsdk GREATER -1 AND _ver_major STREQUAL _oz_tested_clang_ver)
        message(STATUS
            "Objective-Z: Using Zephyr SDK Clang ${_ver}: ${OBJZ_CLANG_COMPILER}")
    else()
        set(_oz_clang_complaint
            "Objective-Z: Using ${_oz_clang_source} Clang ${_ver}: ${OBJZ_CLANG_COMPILER}\n"
            "The tested environment is Zephyr SDK LLVM Clang ${_oz_tested_clang_ver}. "
            "Other versions may produce different AST output, which decides ivar "
            "ownership and method definedness in the generated C. "
            "Install the SDK's LLVM component (west sdk install --llvm, or setup.sh -l) "
            "and set ZEPHYR_SDK_INSTALL_DIR, or point -DOBJZ_CLANG_PATH at a Clang "
            "${_oz_tested_clang_ver}.")
        if(OBJZ_REQUIRE_TESTED_CLANG)
            message(FATAL_ERROR ${_oz_clang_complaint}
                "\nThis is fatal because -DOBJZ_REQUIRE_TESTED_CLANG=ON. "
                "Unset it to downgrade this to a warning.")
        else()
            message(WARNING ${_oz_clang_complaint})
        endif()
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

# Two helpers used to sit here, and both went dead when the Python backend
# was retired: `_objz_append_arch_defines()` and `_objz_build_clang_flags()`,
# whose only call sites were in `cmake/oz_transpile.cmake` (#304). Deleted
# rather than kept, because their premise expired with them -- both existed to
# patch up a compile_commands.json built for the *host* target, which is why
# one of them shelled out to Clang for `__ARM_*` predefines and the other
# named `-fobjc-runtime=gnustep-2.0`. The entries now come from
# `_objz_build_ast_flags()`, which names the real target triple (#274), so
# there is nothing left to patch up. `git show fec8609^:cmake/oz_transpile.cmake`
# has the callers if the reasoning is ever needed again.
#
# ─── Collect compile_commands.json entry for ObjC files ──────────────
#
# _objz_collect_compile_db(<source.m> <output.o> <flag1> [flag2 ...])
#
# Appends a JSON entry to the OBJZ_COMPILE_DB_JSON global property.
# Registers a deferred function to write compile_commands_objc.json
# and create a merge target (once).
#
# Called from `oz_static.cmake`'s AST-dump loop, once per `.m` file it is
# about to hand to Clang, with the same flags. That is the only caller: the
# entry is only as good as the flags, and those are the flags Clang is known
# to accept on that exact file.
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

    # Only the `.m` files that were actually collected get an entry here.
    #
    # Headers are a separate matter and are synthesised, by
    # `scripts/objz_merge_compile_db.py` rather than in this loop -- see #320.
    # The reasoning below is about `.m` files and does not carry over to them:
    # for a `.h` the nearest command clangd can interpolate from is the
    # *generated* C twin, which is not an Objective-C command at all.
    #
    # There used to be a fallback here that globbed every `.m` in the module
    # and synthesised an entry for the ones this build did not compile, on the
    # theory that clangd needs one per file. Two problems, and it is the second
    # that retires the idea rather than repairing it (#304). It globbed only
    # the module's own `samples/`, `tests/`, `benchmarks/` and `src/`, so an
    # out-of-tree app was never covered by it in the first place; and each
    # synthetic entry reused the *first* collected entry's argument list
    # verbatim, `-c <source> -o <object>` included, so every one of them named
    # the wrong input.
    #
    # Nothing is lost by dropping it. For a `.m` with no entry of its own
    # clangd interpolates from the nearest one it has, and now that the nearest
    # one is a real Objective-C command -- `-fobjc-arc`, `-fblocks`,
    # `-fobjc-runtime=macosx`, the right `--target` -- interpolation lands
    # somewhere far better than a synthesised command with a wrong `-c` ever
    # did. Only the include paths differ, and only for a file outside this
    # build. (That last sentence is why headers needed the opposite answer:
    # for them it is not only the include paths that differ.)
    string(REGEX REPLACE ",\n$" "\n" _json "${_json}")
    file(WRITE "${CMAKE_BINARY_DIR}/compile_commands_objc.json" "[\n${_json}]\n")

    # Whether the build directory sits inside the source tree. Both editor
    # conveniences below are conditional on it, for the same reason: twister
    # builds every sample in a temporary directory, and neither a symlink into
    # a deleted build nor a `.clangd` naming one does a checked-out tree any
    # good (#304).
    file(RELATIVE_PATH _bin_rel "${CMAKE_SOURCE_DIR}" "${CMAKE_BINARY_DIR}")
    if(_bin_rel MATCHES "^\\.\\." OR IS_ABSOLUTE "${_bin_rel}")
        set(_objz_in_tree_build FALSE)
    else()
        set(_objz_in_tree_build TRUE)
    endif()

    # A database in the build directory is not enough on its own. Editors and
    # their clangd plugins pass `--compile-commands-dir <project root>`, which
    # *overrides* the `CompilationDatabase:` key written below -- so clangd
    # looks in the source root, finds nothing, and falls back to a bare
    # `clang -x objective-c file.m`: host triple, no include paths, every
    # `#import` unresolved. Linking the database where they look is what makes
    # an out-of-tree app work without per-editor configuration (#320).
    #
    # A symlink rather than a copy, so it cannot go quietly stale.
    set(_objz_link_db "")
    if(_objz_in_tree_build)
        set(_objz_link_db
            COMMAND ${CMAKE_COMMAND} -E create_symlink
                    "${_bin_rel}/compile_commands.json"
                    "${CMAKE_SOURCE_DIR}/compile_commands.json")
    endif()

    add_custom_target(objz_compile_db ALL
        COMMAND ${Python3_EXECUTABLE}
                ${_mod}/scripts/objz_merge_compile_db.py
                ${CMAKE_BINARY_DIR}/compile_commands.json
                ${CMAKE_BINARY_DIR}/compile_commands_objc.json
                --root ${CMAKE_SOURCE_DIR}
                --root ${_mod}
                --build-dir ${CMAKE_BINARY_DIR}
        ${_objz_link_db}
        COMMENT "ObjZ: merging ObjC entries into compile_commands.json"
        VERBATIM
    )

    # Generate a minimal .clangd at the app project root, naming the build
    # directory this configure actually used -- but only when that directory
    # lives inside the source tree.
    #
    # It used to be the bare literal `build`, correct only for the default
    # in-tree layout. Naming ${CMAKE_BINARY_DIR} instead fixes `west build -d`
    # and breaks something worse: twister builds every sample in a temporary
    # directory, so `just test` would leave each sample's checked-out `.clangd`
    # pointing at a path that no longer exists. Skipping the write is the
    # answer for that case -- an out-of-tree build has no business rewriting
    # the editor config of a tree it is only reading (#304).
    if(NOT _objz_in_tree_build)
        message(STATUS "Objective-Z: build dir is outside the source tree; "
                       "leaving ${CMAKE_SOURCE_DIR}/.clangd alone")
    else()
        set(_clangd_path "${CMAKE_SOURCE_DIR}/.clangd")
        file(WRITE "${_clangd_path}"
"# Auto-generated by Objective-Z module — do not edit manually.\n\
# Regenerated on every CMake configure (just build / just rebuild).\n\
\n\
CompileFlags:\n\
  CompilationDatabase: ${_bin_rel}\n")
        message(STATUS "Objective-Z: generated ${_clangd_path}")
    endif()

    message(STATUS "Objective-Z: wrote ${CMAKE_BINARY_DIR}/compile_commands_objc.json")
endfunction()

# ─── Build Clang flags for AST analysis, and for the IDE ────────────
#
# No longer "host-compatible", which this heading said for as long as the
# dump was parsed as the build machine: `--target=` has named the real
# embedded triple since #274. The same flags, minus `-w`, are what
# `oz_static.cmake` hands to `_objz_collect_compile_db()` (#304), so a change
# here moves both the transpiler's oracle and what clangd sees.
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

    # The dump has to be produced for the *target*, not the host (#274).
    # `_objz_get_clang_target_triple()` has existed all along and this
    # function never called it, so every dump was parsed as the build
    # machine: 64-bit pointers on an arm64 Mac, and — the part that broke
    # things — Zephyr's arch headers reaching for intrinsics the host has no
    # declaration of. Measured on `qemu_riscv32`, one `src/OZTimer.m` dump:
    # **20 errors without the triple, 1 with it.** Twenty matters because it
    # is Clang's default `-ferror-limit`, at which point it emits
    # `fatal error: too many errors emitted, stopping now` and stops — so
    # every RISC-V dump was silently truncated, and every sample's OZTimer
    # facts along with it. The one that remains is `__oz_timer_setup` being
    # undeclared, which is ordinary and truncates nothing (#267).
    _objz_get_clang_target_triple(_ast_triple)
    list(APPEND _flags --target=${_ast_triple})

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
