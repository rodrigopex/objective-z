# SPDX-License-Identifier: Apache-2.0
#
# oz_static.cmake — Transpile Objective-C (.m) to plain C via `oz2c`.
#
# The entry point every consumer calls is `objz_transpile_sources()`, defined
# at the bottom of this file. It used to live in `oz_transpile.cmake` and
# dispatch on CONFIG_OBJZ_BACKEND between two backends; with the Python one
# retired there is nothing to dispatch on, so that file is gone and this one
# is included directly from the module's CMakeLists.txt. The public name is
# unchanged because 13 samples, px-app and any out-of-tree user call it.
#
# Supported: any number of entry `.m` files (all merged into one
# translation unit, with further `.m`s pulled in automatically via
# `#import` — see tools/oz_static/src/imports.rs), INCLUDE_DIRS,
# ROOT_CLASS as a cross-check, and POOL_SIZES.
#
# Objects come from a per-class PAL slab (`OZ_SLAB_DEFINE`), sized by
# counting allocation sites and overridable per class — see
# tools/oz_static/src/pools.rs. A size can also be stated in the source
# itself as `/* oz-pool: Class=N,... */`, the same directive the Python
# backend's own test harness reads; POOL_SIZES here wins for the classes
# it names.
#

include_guard(GLOBAL)

# objz_find_clang() and _objz_build_ast_flags(), for the AST dumps oz2c reads
# as an ownership oracle. `oz_transpile.cmake` used to include this before
# handing over; now that it is gone, this file needs it itself.
include(${ZEPHYR_OBJZ_MODULE_DIR}/cmake/ObjcClang.cmake)

# ─── Public API ───────────────────────────────────────────────────────
#
# objz_transpile_sources_static(<target> <src.m> [<src2.m> ...]
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

    if(NOT _sources)
        message(FATAL_ERROR
            "objz_transpile_sources_static: no .m source files given")
    endif()

    # ── Build oz2c (debug profile: configure-time compile speed matters
    #    here, not the transpiler's own runtime speed). Once per configure,
    #    which is not once per sweep -- see the lock below ──────────────
    set(_oz_static_dir ${_mod}/tools/oz_static)
    set(_oz2c ${_oz_static_dir}/target/debug/oz2c)
    # Globbed at configure time, which is enough for the case this exists
    # for: editing the transpiler and rebuilding a sample. Adding a *new*
    # source file needs a re-configure, same as any CMake glob.
    file(GLOB_RECURSE _oz2c_srcs ${_oz_static_dir}/src/*.rs)
    list(APPEND _oz2c_srcs ${_oz_static_dir}/Cargo.toml)
    # Zephyr's own toolchain cmake exports CC/CFLAGS (the ARM
    # cross-compiler) into ENV, which cc-rs (tree-sitter-objc's C parser
    # build script) would otherwise inherit -- unset them so cargo builds
    # oz2c, a host tool, with the host's own native compiler.
    # Configure-time output streams straight to the terminal, unlike
    # build-time output, which ninja buffers until its edge finishes. So
    # this half of the work is the easiest to make visible and was the
    # least visible: three `execute_process` calls that between them ran
    # cargo, wrote hundreds of megabytes of AST and transpiled the whole
    # program, announced by nothing (#299).
    # Serialize this across the parallel CMake processes a twister sweep
    # runs. Thirteen samples configure at once and every one of them reaches
    # this line, so thirteen cargos -- each with its own rustc fan-out --
    # start within the same second. Cargo's own locks make the concurrent
    # *builds* correct, not cheap: the ones that lose the race still fork,
    # block and exit, and the log says so ("Blocking waiting for file lock
    # on package cache", 66 times in one 13-sample sweep). That load spike
    # is what makes a later `execute_process` fail to *start* oz2c at all,
    # which CMake reports as an error string in RESULT_VARIABLE with both
    # output streams empty -- the silent configure failure of #308.
    #
    # The lock lives under `target/`, so it is gitignored and `cargo clean`
    # takes it with the build it guards. `GUARD PROCESS` also releases it if
    # this CMake process dies before the explicit release below.
    set(_oz2c_lock ${_oz_static_dir}/target/.oz2c-build.lock)
    file(MAKE_DIRECTORY ${_oz_static_dir}/target)
    file(LOCK ${_oz2c_lock} GUARD PROCESS TIMEOUT 900 RESULT_VARIABLE _lock_rc)
    if(NOT _lock_rc STREQUAL "")
        # Not fatal: a lock we could not take costs concurrency, and cargo's
        # own locking still makes the build correct. Say so and carry on
        # rather than failing a build for a missing optimisation.
        message(WARNING
            "objz_transpile_sources_static: could not lock ${_oz2c_lock} "
            "(${_lock_rc}); building oz2c unserialized")
    endif()
    message(STATUS "oz_static: building oz2c (cargo)")
    execute_process(
        COMMAND ${CMAKE_COMMAND} -E env --unset=CC --unset=CXX --unset=CFLAGS --unset=CXXFLAGS
                --unset=LDFLAGS --unset=AR --unset=RANLIB --unset=NM
                cargo build --manifest-path ${_oz_static_dir}/Cargo.toml
        RESULT_VARIABLE _cargo_rc
        OUTPUT_VARIABLE _cargo_out
        ERROR_VARIABLE _cargo_err
        ECHO_OUTPUT_VARIABLE
        ECHO_ERROR_VARIABLE
    )
    if(NOT _cargo_rc EQUAL 0)
        message(FATAL_ERROR
            "objz_transpile_sources_static: cargo build of oz2c failed\n"
            "  result: ${_cargo_rc}\n"
            "  stderr: ${_cargo_err}\n"
            "  stdout: ${_cargo_out}")
    endif()
    # Released as soon as the binary exists: what has to be serialized is
    # the build, not the rest of this function. The `FATAL_ERROR` above
    # needs no release of its own -- `GUARD PROCESS` drops the lock when
    # CMake exits.
    file(LOCK ${_oz2c_lock} RELEASE)

    # Every listed `.m` becomes one entry file, and every directory they
    # live in becomes an `--impl-dir` so `#import "X.h"` can find its
    # sibling `X.m` there (oz2c never searches a header's own directory
    # for that -- see imports.rs::find_sibling_impl).
    set(_src_abs_list "")
    set(_src_dirs "")
    foreach(_src ${_sources})
        get_filename_component(_one_abs ${_src} ABSOLUTE)
        get_filename_component(_one_dir ${_one_abs} DIRECTORY)
        list(APPEND _src_abs_list ${_one_abs})
        list(APPEND _src_dirs ${_one_dir})
    endforeach()
    list(REMOVE_DUPLICATES _src_dirs)

    set(_oz2c_flags -I ${_mod}/include/oz_sdk)
    # CONFIG_OBJZ_HEAP enables `+allocWithHeap:` and the heap-aware free
    # path. The generated code is additionally guarded by OZ_HEAP_SUPPORT,
    # which is what makes the PAL expose the heap functions it calls -- so
    # both have to be set together, exactly as the Python backend does it.
    if(CONFIG_OBJZ_HEAP)
        list(APPEND _oz2c_flags --heap-support)
    endif()
    # CONFIG_OBJZ_INTROSPECTION enables -isKindOfClass: and
    # -conformsToProtocol:, the two introspection selectors that generate a
    # table. Unlike --heap-support this needs nothing defined on the C side:
    # the tables and their helpers are emitted into the companion source
    # itself, and only for the constructs a call site actually used. Class
    # identity ([Foo class], [obj class], -isMemberOfClass:) is always
    # available and unaffected.
    if(CONFIG_OBJZ_INTROSPECTION)
        list(APPEND _oz2c_flags --introspection)
    endif()
    # CONFIG_OBJZ_REFLECTION enables @selector, SEL, -respondsToSelector:
    # and the -performSelector: family. Like --introspection this needs
    # nothing on the C side: the selector records, their wrappers and the
    # two helpers are emitted into the companion source, and only for the
    # selectors a @selector(...) actually named.
    if(CONFIG_OBJZ_REFLECTION)
        list(APPEND _oz2c_flags --reflection)
    endif()
    # CONFIG_OBJZ_DEBUG_LINES puts #line directives on the generated C, so
    # gdb, addr2line, a fatal-error backtrace and coverage all name the .m
    # the code was written in instead of oz_static_generated/<Class>.c
    # (#305). Like --introspection this needs nothing on the C side: the
    # directives are in the emitted text and change only what the compiler
    # writes into DWARF, so the program itself is byte-for-byte the same.
    # Default y in Kconfig, and this flag is what supplies it -- oz2c emits
    # none without it, which is what keeps a hand-run transpile (and the
    # committed tests/zephyr/generated C) free of one machine's absolute
    # paths.
    if(CONFIG_OBJZ_DEBUG_LINES)
        list(APPEND _oz2c_flags --line-directives)
    endif()
    # Ask oz2c for its per-phase table. Off by default because the default
    # output already names the one number that matters (the AST ingest);
    # turn it on with -DOBJZ_OZ2C_TIMINGS=ON when that number needs
    # breaking down. Follows OBJZ_ALLOW_PARTIAL_AST /
    # OBJZ_REQUIRE_TESTED_CLANG in being a plain cache variable.
    if(OBJZ_OZ2C_TIMINGS)
        list(APPEND _oz2c_flags --timings)
    endif()
    foreach(_dir ${_src_dirs})
        list(APPEND _oz2c_flags --impl-dir ${_dir})
    endforeach()
    foreach(_dir ${OZT_INCLUDE_DIRS})
        list(APPEND _oz2c_flags -I ${_dir})
    endforeach()
    # A root class is inferred, not configured (imports/collect find the
    # one class with no superclass), so pass ROOT_CLASS only when the
    # caller stated one -- oz2c then verifies it matches, turning a
    # mis-stated root into an error instead of a silently different
    # program. Costs an extra collect pass, hence only when asked.
    if(OZT_ROOT_CLASS)
        list(APPEND _oz2c_flags --root-class ${OZT_ROOT_CLASS})
    endif()
    # Same spelling as the Python backend's flag, so a sample states its
    # pool sizes once and either backend honours them.
    if(OZT_POOL_SIZES)
        list(APPEND _oz2c_flags --pool-sizes ${OZT_POOL_SIZES})
    endif()
    # The target's own include directories, which `include_directories(include)`
    # in a sample's CMakeLists sets. Collected once into `_target_inc_dirs`
    # because **two consumers need them and only one used to get them** (#274):
    # oz2c resolved a sample's own headers while the Clang AST dump below did
    # not, so a dump of `samples/zbus_service/src/main.m` died on
    # `fatal error: 'TemperatureService.h' file not found` -- and, being
    # truncated rather than absent, looked like a complete oracle.
    #
    # `cmake/oz_transpile.cmake` has always added them to its own AST flags
    # (its "Add target include dirs" block), so this was a port omission
    # rather than a question anyone decided. The outgoing backend being the
    # correct reference is unusual enough to be worth the note.
    set(_target_inc_dirs "")
    get_target_property(_target_incs ${target} INCLUDE_DIRECTORIES)
    if(_target_incs)
        foreach(_dir ${_target_incs})
            string(FIND "${_dir}" "$<" _is_genexpr)
            if(_is_genexpr EQUAL -1)
                list(APPEND _target_inc_dirs ${_dir})
            endif()
        endforeach()
    endif()
    foreach(_dir ${_target_inc_dirs})
        list(APPEND _oz2c_flags -I ${_dir})
    endforeach()

    set(_outdir ${CMAKE_CURRENT_BINARY_DIR}/oz_static_generated)
    set(_manifest ${_outdir}/oz_static_manifest.txt)
    file(MAKE_DIRECTORY ${_outdir}/Foundation)

    # ── Clang AST dumps ───────────────────────────────────────────────
    #
    # tree-sitter gives oz2c syntax but no resolved types, so it cannot tell
    # on its own whether an `id`-typed ivar is an object the class owns. That
    # answer decides whether ARC releases the ivar: releasing a non-object
    # corrupts memory, skipping a real one leaks it, so without a dump oz2c
    # stays conservative and skips every `id` ivar. This build path passed no
    # --ast at all, which meant the on-target build was the one place those
    # facts were missing.
    #
    # One dump per source, and `--ast` is repeatable because a single dump
    # is not enough: Clang preprocesses `#import`s, so a dump of `main.m`
    # carries every `@interface` it imports but only the `@implementation`s
    # written in that one file. The module's own `src/*.m` are dumped too --
    # oz2c splices them via `--impl-dir`, and their ivars are exactly the
    # ones the Foundation classes own. oz2c unions the facts
    # (`astinfo::AstFacts::merge`).
    objz_find_clang()
    _objz_build_ast_flags(_ast_flags)
    list(PREPEND _ast_flags -I${_mod}/include/oz_sdk)
    # -fobjc-arc, or the dump carries no ownership qualifiers at all and the
    # whole exercise is pointless.
    list(APPEND _ast_flags -fobjc-arc)
    # The target's own include dirs, so a sample's `#include "Foo.h"` resolves
    # here as it already did for oz2c above (#274). `oz_transpile.cmake` has
    # always done this.
    foreach(_dir ${_target_inc_dirs})
        list(APPEND _ast_flags -I${_dir})
    endforeach()

    # clangd wants the same flags, minus `-w`: showing the diagnostics the dump
    # deliberately silences is the entire job (#304). Split here rather than
    # deriving one list from the other, so neither can drift.
    set(_ide_flags ${_ast_flags})
    # `-Warc-performSelector-leaks` fires on every `-performSelector:` whose
    # selector is not a literal, whatever the selector returns -- on
    # px-keyboard's four gestures, all void, that is four false positives with
    # nothing to act on. It is editor-only and cannot fail a build: the AST
    # dump runs with `-w` below, and the real build compiles generated C with
    # gcc, which never sees a `-performSelector:`. The ObjC idioms for
    # silencing it locally presuppose a runtime this project does not have.
    #
    # Suppressing it is only defensible together with #322, which answers the
    # precise question -- is an owning return being discarded -- exactly,
    # where Clang can only guess from a hair trigger.
    list(APPEND _ide_flags -Wno-arc-performSelector-leaks)
    list(APPEND _ast_flags -w)  # AST dump is transpiler input; warnings are noise

    file(GLOB _sdk_impls ${_mod}/src/*.m)
    set(_ast_dir ${_outdir}/ast)
    file(MAKE_DIRECTORY ${_ast_dir})
    set(_ast_script "${_ast_dir}/oz_static_ast.sh")
    set(_ast_lines "#!/bin/sh\n")
    set(_ast_args "")
    set(_ast_outputs "")
    # How many dumps, so each `echo` below can say `k/N` -- the counter is
    # what turns a silent stretch into a visible rate. This phase writes
    # hundreds of megabytes and took 2.9s on px-keyboard while printing
    # nothing at all (#299).
    list(LENGTH _src_abs_list _n_entry)
    list(LENGTH _sdk_impls _n_sdk)
    math(EXPR _n_ast "${_n_entry} + ${_n_sdk}")
    set(_ast_k 0)
    foreach(_src ${_src_abs_list} ${_sdk_impls})
        get_filename_component(_name ${_src} NAME)
        string(MAKE_C_IDENTIFIER "${_name}" _safe)
        set(_ast "${_ast_dir}/${_safe}.ast.json")
        math(EXPR _ast_k "${_ast_k} + 1")

        # The same source, the same flags, as a compile_commands.json entry, so
        # clangd parses the Objective-C rather than guessing from the generated
        # C. This loop is the right place because it already visits exactly the
        # files that need one -- the target's sources and the module's `src/*.m`
        # -- with flags Clang is known to accept on them: it is about to run
        # this very command. The previous call site was in the Python backend's
        # `oz_transpile.cmake` and went with it (#304), which left every `.m`
        # file with no entry at all.
        _objz_collect_compile_db(${_src} "${_ast_dir}/${_safe}.ide.o" ${_ide_flags})
        string(JOIN " " _one ${OBJZ_CLANG_COMPILER} ${_ast_flags}
               -fsyntax-only -Xclang -ast-dump=json ${_src})
        # Keep each dump's diagnostics next to it, but only the ones that
        # mean the dump is *incomplete* -- a `fatal error`, which is where
        # Clang stops (#274). The previous form was
        # `> ${_ast} 2>/dev/null || true`, which threw away both the exit
        # code and the message; and since every line ended in `|| true` the
        # script's own status was always 0, making the
        # `if(NOT _ast_rc EQUAL 0)` check below it dead code.
        #
        # Exit status alone is the wrong signal, which cost a pass here to
        # discover. Clang exits non-zero for an ordinary error too and then
        # *carries on*, so an ordinary error must not fail the dump. That is
        # still the rule, but the examples this comment used to give are gone:
        # `__get_BASEPRI` and friends came from flags that named no
        # `--target`, fixed in #274, and `__oz_timer_setup` from a PAL arm
        # retired with OZTimer in #267. The last one standing was
        # `oz_assert`, undeclared because nothing imported
        # `oz_sdk/assert.h` -- fixed in #304, and the dumps of px-keyboard and
        # every Foundation impl are now error-free. None of those touched an
        # ivar's ownership qualifier or whether an `@implementation` was seen,
        # which is all the oracle reads. A `fatal error` is different in kind:
        # Clang stops, and every declaration after it is simply absent from a
        # file that still looks complete.
        # The `> ${_ast}` redirection is confined to the clang command, so
        # a bare `echo` reaches the script's own stdout and lands in the
        # build log alongside oz2c's own progress.
        string(APPEND _ast_lines
            "echo \"oz_static: clang ast ${_ast_k}/${_n_ast} ${_name}\"\n"
            "${_one} > ${_ast} 2> ${_ast}.err\n"
            "grep 'error:' ${_ast}.err | grep -qv -e 'disallowed with ARC' \\\n"
            "    -e 'ARC forbids explicit message send' || rm -f ${_ast}.err\n")
        list(APPEND _ast_args --ast ${_ast})

        # The same dump as its own build-time edge, so ninja can track what
        # it really depends on and rebuild only what changed.
        #
        # `-MD -MF` makes Clang record the header closure it actually read,
        # and `-MT ${_ast}` names the dump as the depfile's target so it
        # matches this command's OUTPUT (without it Clang writes
        # `<stem>.o:`, which ninja rejects). `DEPENDS` alone cannot express
        # this: the closure is only known after preprocessing, which is why
        # editing a spliced header regenerated nothing before (#299).
        #
        # One script per source rather than `sh -c`, because the flag list
        # runs to kilobytes and the `> ${_ast}` redirection needs a shell.
        set(_one_script "${_ast_dir}/${_safe}.sh")
        file(WRITE ${_one_script}
            "#!/bin/sh\n"
            "${_one} -MD -MF ${_ast}.d -MT ${_ast} > ${_ast} 2> ${_ast}.err\n"
            "grep 'error:' ${_ast}.err | grep -qv -e 'disallowed with ARC' \\\n"
            "    -e 'ARC forbids explicit message send' || rm -f ${_ast}.err\n")
        add_custom_command(
            OUTPUT  ${_ast}
            COMMAND sh ${_one_script}
            DEPENDS ${_src}
            DEPFILE ${_ast}.d
            COMMENT "oz_static: clang ast ${_ast_k}/${_n_ast} ${_name}"
        )
        list(APPEND _ast_outputs ${_ast})
    endforeach()
    file(WRITE ${_ast_script} "${_ast_lines}")

    # A dump Clang could not complete is fatal, because it is
    # indistinguishable from a complete one by inspection and it silently
    # weakens the only oracle for ivar ownership (gaps N and AA of the
    # retired PARITY.md; docs/STATUS.md says how to read it).
    #
    # **Any** error, not only a `fatal error`, as of #307 -- with two
    # named exceptions, and the exceptions are the whole design.
    #
    # The old rule made the dumps a place diagnostics went to be ignored.
    # #304's `oz_assert` was undeclared in every dump of every sample for
    # as long as `oz_sdk/assert.h` went unimported, and no build ever said
    # so; #307's `conformsToProtocol:` sat in px-keyboard's the same way.
    # Neither truncated anything, both were real defects in the headers
    # oz2c reads, and "ordinary errors are fine" is why nobody saw either.
    #
    # But the old rule was not merely lax. Clang parses this Objective-C
    # under `-fobjc-arc`, and oz_static deliberately supports constructs
    # whose Objective-C *spelling* ARC refuses:
    #
    #   * `ARC forbids explicit message send of 'dealloc'` -- oz_static
    #     synthesizes the dealloc chain, and `samples/pool_demo`,
    #     `transpiled_led` and `gpio_demo` all write the send out;
    #   * `cast of a block pointer to '...' is disallowed with ARC` --
    #     `samples/gpio_demo` hands a block to a Zephyr callback field.
    #
    # Those two are matched and let through. Everything else fails, which
    # keeps the two defects above from recurring: neither is an ARC
    # complaint, so neither is covered by the exception.
    #
    # Measured, and the measurement is why the list is exactly two long.
    # Across `hello_world`, `zbus_service` and px-keyboard the dumps
    # carried one error between them (#307's). Across all thirteen
    # samples they carried four kinds: those two, plus a
    # `K_THREAD_DEFINE` in `samples/arc_demo` that does not parse as
    # Objective-C at all -- fixed there by wrapping it in `OZM`, not
    # excused here, because `expected identifier` is too broad a pattern
    # to ever allow through.
    #
    # Checked by its own script rather than here, because **this script runs
    # at two different times and only one of them can succeed**:
    #
    #   - at configure time (`execute_process` below), whose sole purpose is
    #     to discover the output file list for the manifest. Zephyr's
    #     *generated* headers do not exist yet on a pristine build, so
    #     anything reaching `zephyr/kernel.h` dies on
    #     `fatal error: 'zephyr/syscall_list.h' file not found`. Expected,
    #     and harmless: a file name does not depend on an ARC fact, and the
    #     build-time run overwrites this output anyway.
    #   - at build time (the `add_custom_command` further down), which
    #     `add_dependencies(oz_static_transpile_gen zephyr_generated_headers)`
    #     orders after those headers exist. **This is the run whose dumps
    #     reach the shipped C**, so this is the run worth checking.
    #
    # #274's include-path bug affected both, which is why it mattered: the
    # build-time dump was truncated too, for every sample with its own
    # `include/`.
    #
    # Deliberately fatal rather than a warning: #269 found that the
    # compatibility warning meant to catch a silently-substituted clang had
    # printed on every CI run for the life of the workflow, unread in a
    # 1400-line log. A warning about a substituted oracle is not a check.
    set(_ast_check "${_ast_dir}/oz_static_ast_check.sh")
    if(OBJZ_ALLOW_PARTIAL_AST)
        file(WRITE ${_ast_check}
            "#!/bin/sh\n"
            "# OBJZ_ALLOW_PARTIAL_AST=ON: report, never fail. ARC then treats\n"
            "# these files' `id` ivars conservatively and skips them --\n"
            "# correct, but a leak.\n"
            "for f in ${_ast_dir}/*.ast.json.err; do\n"
            "  [ -e \"$f\" ] || exit 0\n"
            "  echo \"oz_static: WARNING: diagnostics in a Clang AST dump:\" >&2\n"
            "  cat \"$f\" >&2\n"
            "done\n"
            "exit 0\n")
    else()
        file(WRITE ${_ast_check}
            "#!/bin/sh\n"
            "found=0\n"
            "for f in ${_ast_dir}/*.ast.json.err; do\n"
            "  [ -e \"$f\" ] || break\n"
            "  found=1\n"
            "  if grep -q 'fatal error:' \"$f\"; then\n"
            "    echo \"oz_static: Clang hit a fatal error, so this AST dump stops\" >&2\n"
            "    echo \"where the error is and the ivar-ownership oracle is\" >&2\n"
            "    echo \"incomplete for that file (see docs/STATUS.md):\" >&2\n"
            "  else\n"
            "    echo \"oz_static: Clang reported an error while dumping this AST.\" >&2\n"
            "    echo \"It does not truncate the dump, but it means the Objective-C\" >&2\n"
            "    echo \"oz2c reads does not parse cleanly -- usually a declaration\" >&2\n"
            "    echo \"missing from an oz_sdk header (#304, #307):\" >&2\n"
            "  fi\n"
            "  cat \"$f\" >&2\n"
            "done\n"
            "[ \$found -eq 0 ] && exit 0\n"
            "echo \"oz_static: fix the diagnostics above, or configure with\" >&2\n"
            "echo \"-DOBJZ_ALLOW_PARTIAL_AST=ON to report them and carry on\" >&2\n"
            "echo \"(with conservative ARC, which leaks \\`id\\` ivars in any\" >&2\n"
            "echo \"file whose dump really was truncated).\" >&2\n"
            "exit 1\n")
    endif()

    # The dumps are no longer produced at configure time. They were, and
    # only so that the configure-time transpile below could be handed
    # `--ast` -- but that run exists to discover a *file list*, which no ARC
    # fact affects. So it produced ~742 MB of JSON on px-keyboard, parsed
    # all of it, and threw the result away, once per configure (#299).
    #
    # `--ast` for the build-time run is taken from `_ast_args`, accumulated
    # unconditionally in the foreach above. It used to be rebuilt here by
    # size-testing each dump *as it existed at configure time*, and that
    # list was then used verbatim in the build-time command. Any dump that
    # was 0 bytes at configure time -- Clang dies before
    # `HandleTranslationUnit` and writes nothing, verified -- was therefore
    # dropped from the build-time command line too, even though the
    # build-time dump would have been complete. That silently weakened the
    # ownership oracle with no diagnostic, and it cannot happen now: the
    # list is the full set by construction, and a dump that is missing or
    # truncated is `${_ast_check}`'s job, at build time, as a hard error.
    set(_oz2c_ast ${_ast_args})

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
    # Says *why* there are two full runs, because with progress output on
    # (#299) the duplicate is now visible and reads as a bug otherwise:
    # CMake has to know the generated file list before it can declare it as
    # `OUTPUT` below, and the only thing that knows the list is oz2c. The
    # run's generated C is thrown away and rewritten at build time.
    # `--manifest-only`: the file list, and nothing else. No `--ast`, no
    # generated files -- 0.11s where the full run it replaced took 11.05s
    # on px-keyboard and wrote output the build-time run immediately
    # overwrote.
    #
    # It runs the same code path as a real transpile with only the writes
    # skipped, so the list cannot drift from the real one. The
    # `compare_files` below turns "cannot" into "does not".
    message(STATUS
        "oz_static: reading the generated file list (--manifest-only)")
    # `ECHO_*_VARIABLE` keeps #299's streamed progress -- the output still
    # reaches the terminal as it did -- while also retaining it, so a
    # failure can say what oz2c said. Without the capture the only record
    # was "oz2c failed at configure time", which is what made #308 look
    # like a transpiler bug for four days: the process died under a
    # parallel sweep and CMake threw away both its message and its exit
    # status.
    execute_process(
        COMMAND ${_oz2c} ${_oz2c_flags} --manifest-only ${_src_abs_list} ${_outdir}
                --manifest ${_manifest}
        RESULT_VARIABLE _rc
        OUTPUT_VARIABLE _oz2c_out
        ERROR_VARIABLE _oz2c_err
        ECHO_OUTPUT_VARIABLE
        ECHO_ERROR_VARIABLE
    )
    if(NOT _rc EQUAL 0)
        message(FATAL_ERROR
            "objz_transpile_sources_static: oz2c failed at configure time\n"
            "  result: ${_rc}\n"
            "  stderr: ${_oz2c_err}\n"
            "  stdout: ${_oz2c_out}")
    endif()
    file(STRINGS ${_manifest} _gen_files)
    # Keep the predicted list so the build-time run can be checked against
    # it. If they ever diverge, CMake has declared OUTPUTs that nothing
    # writes (or writes files nothing compiles), and the failure would
    # otherwise surface as a confusing missing-symbol or stale-object error
    # far from here.
    configure_file(${_manifest} ${_manifest}.predicted COPYONLY)

    # ── Build-time: re-run when the source changes ─────────────────────
    add_custom_command(
        OUTPUT  ${_gen_files}
        COMMAND ${CMAKE_COMMAND} -E env --unset=CC --unset=CXX --unset=CFLAGS --unset=CXXFLAGS
                --unset=LDFLAGS --unset=AR --unset=RANLIB --unset=NM
                cargo build --manifest-path ${_oz_static_dir}/Cargo.toml
        # The dumps are their own edges now (above), so ninja produces
        # them -- in parallel, and only the ones whose source or headers
        # changed -- and this command consumes them. It no longer re-runs
        # all N unconditionally.
        #
        # Still ordered before oz2c: this is the run whose facts reach the
        # shipped C, and a truncated dump must stop the build here rather
        # than quietly weaken ARC (#274).
        COMMAND sh ${_ast_check}
        COMMAND ${_oz2c} ${_oz2c_flags} ${_oz2c_ast} ${_src_abs_list} ${_outdir}
                --manifest ${_manifest}
        # Named failure if the file list the configure-time `--manifest-only`
        # run predicted is not the one a full run actually produced.
        COMMAND ${CMAKE_COMMAND} -E compare_files ${_manifest}.predicted ${_manifest}
        # The transpiler's own sources, not just the .m inputs: without them
        # ninja considers the generated C up to date after oz2c itself
        # changes, so a rebuilt transpiler silently produces nothing new.
        # That cost real debugging time -- a fix would land, the sample would
        # be rebuilt, and the old generated C would still be compiled.
        #
        # `${_ast_outputs}` is what makes a header edit reach the generated
        # C: Clang's depfile regenerates the affected dump, and this edge
        # then re-runs because one of its inputs changed. `${_sdk_impls}`
        # is listed because the module's own `src/*.m` are spliced into the
        # translation unit and were missing from this list entirely.
        DEPENDS ${_src_abs_list} ${_oz2c_srcs} ${_ast_outputs} ${_sdk_impls}
        # The only line ninja prints *before* the edge runs, so it carries
        # the scale rather than just the verb.
        COMMENT "oz_static: ${_n_entry} source(s) -> C via oz2c (${_n_ast} Clang AST dumps)"
        # Ninja buffers a command's output and prints it when the edge
        # finishes. This edge takes ~15s on px-keyboard and is 85% of that
        # build's wall clock, so buffered progress would all arrive after
        # the wait it is meant to explain -- which is worse than no progress
        # at all. USES_TERMINAL puts the edge in ninja's `console` pool,
        # which streams unbuffered, and also surfaces cargo's own progress.
        #
        # The cost is that the console pool has depth 1, so this edge no
        # longer overlaps other console jobs. That is acceptable here
        # precisely because it is already the critical path: every generated
        # .c depends on it, so nothing else was overlapping it anyway.
        # Under `make` there are no pools and this is simply ignored;
        # Zephyr defaults to ninja.
        #
        # No argument: it is a valueless flag, and `USES_TERMINAL TRUE`
        # makes CMake attribute the stray `TRUE` to the preceding COMMENT
        # ("COMMENT requires exactly one argument", CMP0175).
        USES_TERMINAL
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
    # BEFORE, not appended: a generated header is the translation of the
    # source header it was made from and carries the same basename, so both
    # can be on the include path at once. A sample that does
    # `target_include_directories(app PRIVATE include)` gets its own
    # directory searched first, and `#include "Car.h"` from generated C then
    # found `samples/*/include/Car.h` -- the Objective-C original, whose
    # `#import <objc/objc.h>` the ARM compiler cannot resolve. The generated
    # translation has to win.
    target_include_directories(${target} BEFORE PRIVATE ${_outdir} ${_outdir}/Foundation)

    # PAL: select Zephyr backend and provide include path
    target_include_directories(${target} PRIVATE ${_mod}/include)
    # oz_sdk too, for the SDK headers oz2c resolves but deliberately does
    # not splice: a header that reaches no Objective-C is left as the
    # ordinary `#include` it was, so the path it names has to resolve at
    # compile time as well. `samples/zbus_service` has
    # `#include <Foundation/OZLog.h>`, which lives only here.
    target_include_directories(${target} PRIVATE ${_mod}/include/oz_sdk)
    target_compile_definitions(${target} PRIVATE OZ_PLATFORM_ZEPHYR)
    if(CONFIG_OBJZ_HEAP)
        target_compile_definitions(${target} PRIVATE OZ_HEAP_SUPPORT)
    endif()

    # Real oz_sdk headers keep ARC ownership qualifiers (__unsafe_unretained,
    # etc.) for the Python pipeline's Clang-AST analysis, which genuinely
    # needs them -- oz_static preserves ivar declarations verbatim from
    # source rather than re-synthesizing them (its "literate" design), so
    # those qualifiers reach the final GCC compile unchanged. They're a
    # a no-op for the generated C either way: oz_static lowers the
    # qualifiers off the ivar declarations it emits, and its own ARC works
    # off the Clang AST's ownership facts rather than off these spellings
    # surviving into C. The -D covers the positions the lowering does not
    # rewrite (a method parameter, a local), and is an empty define rather
    # than an edit to the shared headers or to oz_static's verbatim copy.
    target_compile_definitions(${target} PRIVATE __unsafe_unretained=)

    # Add OZLog support (pure C, matching the prototypes hardcoded into
    # oz_static's own generated companion header -- see companion.rs). It
    # reaches for "oz_dispatch.h" and "OZObject_ozh.h", the Python
    # pipeline's generated filenames; the two shim headers written into
    # ${_outdir}/Foundation above are what make the same file compile here,
    # and ${_outdir}/Foundation is on this target's include path.
    target_sources(${target} PRIVATE ${_mod}/src/OZLog.c)

    set_target_properties(${target} PROPERTIES LINKER_LANGUAGE C)
endfunction()


# ─── Public entry point ───────────────────────────────────────────────
#
# `objz_transpile_sources()` is what samples, px-app and out-of-tree users
# call. It was a dispatcher over CONFIG_OBJZ_BACKEND until the Python
# pipeline was retired; there is one backend now, so it forwards. Kept as a
# separate name rather than renaming the implementation so that every
# existing call site keeps working.
function(objz_transpile_sources target)
    objz_transpile_sources_static(${target} ${ARGN})
endfunction()
