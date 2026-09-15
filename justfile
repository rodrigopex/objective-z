alias b := build
alias c := clean
alias r := run
alias f := flash
alias m := monitor
alias t := test

project_dir := "samples/hello_world"
board := "mps2/an385"
riscv_board := "qemu_riscv32"
smp_board := "qemu_cortex_a53/qemu_cortex_a53/smp"
hw_board := "nrf52833dk/nrf52833"   # real silicon; see test-hardware
flags := ""
tty := "/dev/tty.usbmodem0006850372581"

# Twister output root, one per checkout, and every twister recipe below derives
# its own directory from it rather than repeating a literal.
#
# It is keyed on the checkout because work happens in `.claude/worktrees/oz-NNN`
# and several of those can be under test at once. Every recipe used to hardcode
# `/tmp/twister-out`, so two sweeps shared one path -- and `-c` below is what
# makes that fatal rather than merely wasteful: the second sweep *deletes* the
# directory the first one is still writing into. What that looks like is 8 of 14
# configurations failing at configure time, in Zephyr's snippet handling, with
# a FileNotFoundError for a directory that existed when the run started and
# nothing at all pointing at the change under test (#315).
#
# Keyed on the directory rather than the branch so that switching branches
# mid-sweep does not move the path out from under it, and so a worktree keeps
# one directory instead of one per branch it has ever held.
#
# Override it per invocation when a run's output has to be kept aside:
# `just outdir=/tmp/twister-out-before test`.
outdir := "/tmp/twister-out-" + file_name(justfile_directory())

# Every `-O` suffix the twister recipes below hang off `outdir`, and the list
# `clean-twister` walks to remove this checkout's output by name.
#
# Enumerated rather than globbed, because `{{ outdir }}*` over-reaches as soon
# as one name is a prefix of another. No two *lane* names collide today (all 16
# checked), but the hazard is live in two ways: this very checkout is `oz-477b`,
# so an `oz-477` lane would sweep it, and `spinvalidate` is already a prefix of
# `spinvalidate-smp` right here in this list.
#
# A new twister recipe with a new suffix needs a word here, or its output is
# never reclaimed by name.
outdir_suffixes := "riscv smp hw zephyr bench spinvalidate spinvalidate-smp"

# Confirmation for the one recipe that reaches outside this checkout. A
# variable rather than just's `[confirm]`, because these recipes run in
# non-interactive sessions where a prompt with no tty is a hang, not a prompt.
yes := "0"

rebuild:
    west build -p -b {{ board }} {{ project_dir }} -- {{ flags }}

build:
    west build -b {{ board }} {{ project_dir }} -- {{ flags }}

flash:
    west flash

# `rm -rf`, not `rip`: `rip` moves the bytes to /tmp/graveyard-$USER, which
# frees no space at all and is the opposite of the point. That argument was
# already written twenty lines below for the twister output and simply never
# applied here, so `clean` reclaimed nothing for as long as it has existed
# (#521). `px-keyboard/justfile` makes the same choice for the same reason.
#
# A CMake build directory is regenerable by definition, so `rip`'s undo buys
# nothing that a rebuild does not.
#
# This checkout's Zephyr build directory; the undo is `just build`.
clean:
    rm -rf build

# The Rust build artifacts, and the largest thing this repo accumulates by a
# wide margin: 1.9 GB in the primary checkout, and 2-9 GB in each worktree
# under `.claude/worktrees`. Nothing reached it before (#521) -- there was no
# `cargo clean` anywhere in the justfiles, the docs or CI.
#
# Kept out of `clean` because a cold Rust rebuild costs minutes where a Zephyr
# build directory costs seconds. `clean-all` is the one-command form.
#
# Deliberately ungated, unlike `clean-twister`: this reaches only this
# checkout's target directory, and cargo serialises access to it through
# `target/.cargo-lock`, so a concurrent build in the same checkout contends
# rather than corrupts -- worst case a repeated build, not a damaged tree. A
# machine-wide `cargo`/`rustc` check would be worse than nothing, because the
# lock is per target directory and any of the other worktrees building cannot
# affect this one.
#
# The transpiler's Cargo target directory -- the largest regenerable object here.
clean-rust:
    cargo clean --manifest-path tools/oz2c/Cargo.toml

# Every sample's build directory. The configure-time AST dumps land inside
# them (`cmake/oz2c.cmake:286`), so removing the directory reclaims those with
# it rather than leaving anything to chase separately -- `pool_demo` alone is
# 74 MB, 56 MB of that AST JSON.
#
# Every sample's own build directory, its AST dumps and generated C included.
clean-samples:
    rm -rf samples/*/build

# This checkout's twister output, and only this checkout's -- roughly a
# gigabyte per directory.
#
# `outdir` is keyed on the checkout, so these names are computable and there is
# no reason to reach anything else. What used to be here was
# `rm -rf /tmp/twister-out*`, which destroyed every lane's output, so
# `docs/WORKING.md` had to carry "Never `just clean-twister`" -- a recipe whose
# documentation was an instruction not to run it. This is that instruction
# implemented instead, and `clean-twister-all` is the old behaviour under a
# name that says what it does (#521).
#
# Every behaviour change here is in the safe direction: someone who typed
# `clean-twister` meaning the global sweep now reclaims less, and someone who
# typed it not knowing no longer destroys fifteen other lanes' output.
#
# The suffixes come from `outdir_suffixes` and are enumerated, not globbed --
# see the reasoning there; `spinvalidate` being a prefix of `spinvalidate-smp`
# is enough on its own to rule a glob out.
#
# Honours an override, so `just outdir=/tmp/twister-out-before clean-twister`
# removes exactly the directory a run was set aside in.
#
# `rm -rf`, not `rip`, for the reason given on `clean`.
#
# This checkout's twister output only, by name; roughly 1 GB per directory.
clean-twister:
    #!/usr/bin/env bash
    set -euo pipefail
    for s in "" {{ outdir_suffixes }}; do
        d="{{ outdir }}${s:+-$s}"
        if [ -e "$d" ]; then
            echo "removing $d ($(du -sh "$d" | cut -f1))"
        fi
        # `.[0-9]*` catches any `.1`/`.2` rotation from a twister run made
        # without `-c` (#312). Unmatched, it reaches `rm` as a literal, which
        # `-f` makes a no-op. The `if` is load-bearing: `[ -e "$d" ] && echo`
        # as the loop's last command would exit non-zero under `set -e` on the
        # first directory that does not exist.
        rm -rf -- "$d" "$d".[0-9]*
    done

# Every checkout's twister output, including orphans -- a worktree that has
# been deleted leaves its sweep output behind with nothing left to name it, and
# this is the only thing that reaches those. That is why the broad form is kept
# rather than dropped.
#
# The glob is `twister-out*`, not `twister-out-*`, so it also reaches the
# single shared directory the recipes used before `outdir` existed, and any
# `.1`/`.2` rotation predating `-c` (#312) -- the same reason .gitignore spells
# it that way.
#
# Opt-in twice over: by name, and by `yes=1`. These directories belong to other
# sessions and one of them may be mid-sweep. It lists each match with its size
# and mtime first, because `docs/WORKING.md` says to read the matches rather
# than count them, and an mtime from a minute ago is a live sweep.
#
# Every checkout's twister output, orphans included; needs `yes=1`.
clean-twister-all:
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    dirs=(/tmp/twister-out*)
    if [ ${#dirs[@]} -eq 0 ]; then
        echo "no /tmp/twister-out* directories"
        exit 0
    fi
    for d in "${dirs[@]}"; do
        printf '%8s  %s  %s\n' \
            "$(du -sh "$d" | cut -f1)" "$(date -r "$d" '+%Y-%m-%d %H:%M')" "$d"
    done
    if [ "{{ yes }}" != "1" ]; then
        echo "Those belong to every checkout, not just this one." >&2
        echo "Read the mtimes above, then re-run with yes=1." >&2
        exit 1
    fi
    rm -rf -- "${dirs[@]}"

# Everything regenerable that this checkout owns: the Zephyr build directory,
# the Cargo target, every sample's build directory, and this checkout's own
# twister output. About 2.0 GB in a checkout that has built and swept.
#
# Reaches no other lane and nothing tracked. `tests/zephyr/generated/` is
# committed C gated by the `generated-freshness` job and lives under none of
# these paths; `deps/` is west-managed source and is not touched either.
#
# This is the one to run **before leaving a worktree**, which is where the disk
# actually goes: many lanes under `.claude/worktrees`, each able to grow its
# own multi-gigabyte Cargo target, and nothing ever re-enters a finished lane
# to clean it up (#521). `clean` stays cheap enough to type between builds.
#
# Prints `df` either side, because this justfile's standing advice about
# reclaiming space is to confirm with `df` rather than assume, and a recipe can
# do that itself instead of asking.
#
# Everything regenerable this checkout owns; run it before leaving a worktree.
clean-all:
    @df -h . | tail -1
    just clean
    just clean-rust
    just clean-samples
    just clean-twister
    @df -h . | tail -1

# What this workspace is holding, before deciding what to remove. Read-only --
# it deletes nothing.
#
# The last line is the recurrence driver and the reason this exists: each
# worktree can hold its own multi-gigabyte Cargo target, and a finished lane's
# is reclaimed by nobody. Free space here went 16 GB to 7.5 GB in an hour that
# way, and back only because each lane was asked by hand (#521).
#
# The `-` prefixes let a line fail silently: `du` on a path that does not exist
# is the normal case, not an error.
#
# What this checkout and its worktrees are holding on disk. Read-only.
disk-report:
    @df -h . | tail -1
    -@du -sh tools/oz2c/target build samples/*/build 2>/dev/null
    -@du -sh {{ outdir }}* 2>/dev/null
    -@du -sh .claude/worktrees/*/tools/oz2c/target 2>/dev/null

run:
    west build -t run

monitor:
    tio {{ tty }}

# The transpiler, built once, before anything that configures a sample.
#
# Every sample's configure step runs this same cargo build itself
# (oz2c.cmake), so a sweep that starts with a rebuild pending has 13 of
# them invoking cargo -- each with its own rustc fan-out -- within the same
# second. Cargo's own locks make those builds correct, not cheap, and the
# load spike is what makes a configure step fail to *start* oz2c at all
# (#308). Every twister recipe below depends on this, so the binary is warm
# and no configure step has work to do. It also enforces the standing
# measurement rule: a sweep that picks up a new binary halfway through
# reports a blend of two versions.
#
# Build the transpiler once, before anything configures a sample (#308).
#
# Every recipe that drives `oz2c` depends on this, including the host
# corpora -- which did not, and so only worked when something else had
# happened to build it first (#344). In a fresh checkout
# `just test-behavior` failed all 81 cases with "oz2c not built", and
# `just test-all` with it, since none of the four recipes it calls built
# the binary either. The twister recipes had always declared it; the host
# ones were the omission.
oz2c:
    cargo build --manifest-path tools/oz2c/Cargo.toml

# `-c` (--clobber-output) on every twister recipe below, and it is not
# cosmetic. Without it twister *renames* the previous output directory rather
# than replacing it -- `twister-out` becomes `twister-out.1`, then `.2`, and so
# on with no upper bound. Each run of this recipe writes about a gigabyte, so
# the rotations are pure accumulation: 34 of them had built up to 27 GB, on a
# volume that was at 97% capacity as a result. `-c` deletes the old directory
# instead, so each target keeps exactly one.
#
# The cost, stated because it is a real loss: a previous run's output is gone
# rather than kept as `.1`, so two runs can no longer be diffed against each
# other. Copy the directory aside first, or point the run elsewhere with
# `just outdir=... test`, when that is what you need.
#
# `-c` is also why `outdir` above has to be per checkout: deleting a directory
# a concurrent sweep is writing into is worse than rotating it away (#315).
#
# All samples on ARM (`mps2/an385`), built and run under twister.
test: oz2c
    west twister -T samples/ -p {{ board }} -c -O {{ outdir }}

# Same samples on RISC-V. gpio_demo is filtered out by its own sample.yaml:
# qemu_riscv32 has no led0/sw0 device-tree aliases, and hello_category's
# debug_lines scenario pins mps2/an385, so 13 configurations run here against
# the 15 on ARM.
#
# The same samples on RISC-V (`qemu_riscv32`); 13 configurations select.
test-riscv: oz2c
    west twister -T samples/ -p {{ riscv_board }} -c -O {{ outdir }}-riscv

# Two cores (CONFIG_SMP=y, CONFIG_MP_MAX_NUM_CPUS=2). Only the samples that
# pin no platform, plus arc_demo's own SMP scenarios -- see its sample.yaml for
# why the single-core expectations cannot be reused under real concurrency.
#
# The same samples on two cores, the only board with real lock contention.
test-smp: oz2c
    west twister -T samples/ -p {{ smp_board }} -c -O {{ outdir }}-smp

# Both architectures, so neither hides an architecture-specific regression.
#
# `test-smp` is deliberately **not** here, and #443 briefly put it here by
# mistake -- making this recipe byte-identical to `test-all-boards` below,
# which already ran all three. Two aggregates with one body is worse than
# either split, and the distinction this one draws is real: the two
# architectures are the fast sweep, and SMP is the slower third that
# `test-all-boards` adds. The gap #443 set out to close was never local
# anyway; it was that **CI** ran no SMP leg, which `smp-tests` in
# `.github/workflows/ci.yml` now does.
test-boards:
    just test
    just test-riscv

# `-Wall -Wextra` clean is not the same as valid C: a bare `;` at file scope
# lived in every generated program until #264 and passed that sweep, the corpus
# compile check and `-Werror` on three boards alike. Reports rather than gates,
# a few sites remaining with their reasons in the script; the host half of this
# claim *is* a gate, in corpus_parity.rs.
# ISO C constraint violations in generated C, on target with the ARM toolchain.
test-pedantic *args: oz2c
    python3 scripts/objz_pedantic_sweep.py --board {{ board }} {{args}}

# CONFIG_SPIN_VALIDATE had never been on, on any board, so every green
# `@synchronized` result came from a configuration where the checks are
# compiled out. It is not reachable by accident either: it sits inside
# `if ASSERT` in Zephyr's subsys/debug/Kconfig and no sample enables
# asserts, so the overlay turns both on.
#
# Both boards, because they populate `struct k_spinlock` differently:
# `thread_cpu` alone on single-core ARM, `locked` + `thread_cpu` with two
# cores, where the assertions face real contention.
#
# This is a gate: an __ASSERT failure is a runtime fatal error, so twister
# reports it as a failing configuration. Confirmed to be able to see one --
# disabling the oz_sync_owner check in emit.rs makes both legs report
# `ASSERTION FAIL [z_spin_lock_valid(l)]`, which is what a green run here
# is worth anything against.
#
# Zephyr's own spinlock assertions against generated C, on both boards (#278).
test-spin-validate: oz2c
    west twister -T samples/ -p {{ board }} -c -O {{ outdir }}-spinvalidate \
        -x=EXTRA_CONF_FILE={{ justfile_directory() }}/samples/overlay-spin-validate.conf
    west twister -T samples/ -p {{ smp_board }} -c -O {{ outdir }}-spinvalidate-smp \
        -x=EXTRA_CONF_FILE={{ justfile_directory() }}/samples/overlay-spin-validate.conf

# Real silicon: nRF52833DK over its on-board J-Link, flashed and run, with
# each sample's console output matched against its own `sample.yaml` -- the
# same oracle twister uses on QEMU, on hardware that has real flash timing,
# real interrupt latency and a real `k_mem_slab` in real RAM.
#
# Every board in `test-all-boards` is QEMU, and until this recipe existed
# docs/STATUS.md's "What is not verified" carried "no real board has been used"
# as its oldest item. Compiling for a board proves the input was understood;
# only running proves the output behaves, and QEMU running it proves neither
# of those about hardware.
#
# `hardware-map.yaml` carries the probe id, the jlink runner and the VCOM
# path, so the board needs no arguments -- but it does need to be plugged
# into the debug USB (next to the power switch) and switched on. Check with
# `nrfutil device list`, NOT `nrfjprog --ids`: that lists *remembered* probe
# ids, so it reports a board that is not there at all.
#
# Every sample but `smp_shared` selects here -- it contends two cores on one
# object and this part has one. `gpio_demo` runs its own hardware scenario,
# which asserts the button path QEMU cannot: mps2/an385 has no GPIO interrupt
# support, so the callback registration returns -ENOTSUP there.
#
# Every single-core sample flashed and run on a real nRF52833DK.
test-hardware: oz2c
    west twister -T samples/ -p {{ hw_board }} -c -O {{ outdir }}-hw \
        --device-testing --hardware-map hardware-map.yaml

# Every board, including SMP. The only recipe that exercises two cores.
test-all-boards:
    just test
    just test-riscv
    just test-smp

# Its own output directory, not `test`'s -- the `-zephyr` suffix on `outdir`.
# Both once wrote to the same path, which was survivable while twister rotated
# -- the loser's output became `.1` -- and is not once `-c` deletes instead.
# Running this would then silently discard the sample results, and the two
# suites test different things (15 samples vs the ztest cases over committed C),
# so neither is a stand-in for the other.
#
# The ztest cases over committed C, not the samples -- see tests/zephyr/.
test-zephyr:
    west twister -T tests/zephyr/ -p {{ if os() == "linux" { "native_sim" } else { board } }} -c -O {{ outdir }}-zephyr

bench:
    west build -p -b {{ board }} benchmarks/objc && west flash

bench-cpp:
    west build -p -b {{ board }} benchmarks/cpp && west flash

bench-mem-c:
    west build -p -b {{ board }} benchmarks/memory/c && west flash

bench-mem-cpp:
    west build -p -b {{ board }} benchmarks/memory/cpp && west flash

bench-mem-objc:
    west build -p -b {{ board }} benchmarks/memory/objc && west flash

# Its own output directory, for the reason on `test-zephyr`.
#
# Every benchmark under twister, on hardware.
test-bench: oz2c
    west twister -T benchmarks/ --device-testing --hardware-map hardware-map.yaml -c -O {{ outdir }}-bench

bench-mem:
    just bench-mem-c
    just bench-mem-cpp
    just bench-mem-objc

bench-footprint board="nrf52833dk/nrf52833":
    bash benchmarks/footprint.sh {{ board }}

bench-all:
    just board=nrf52833dk/nrf52833 bench
    just board=nrf52833dk/nrf52833 bench-cpp
    just board=nrf52833dk/nrf52833 bench-mem
    just board=nrf52833dk/nrf52833 bench-footprint

ast-dump file *includes:
    clang -Xclang -ast-dump=json -fsyntax-only {{includes}} {{file}} 2>/dev/null

# The 81-case behavior corpus through oz2c, the default backend. This
# harness carries the compiler/-O matrix, the sanitizers, leak detection and
# gcov, so it is where those reach the *generated* C -- `cargo test`'s
# corpus_parity only transpiles and compiles each case, never runs it.
#
# The 81-case behavior corpus through oz2c (gcc/clang, -O0/-O2, ASan, LSan).
test-behavior *args: oz2c
    python3 -m pytest tests/behavior/ -v {{args}}

# 40 tests adapted from LLVM, GNUstep, Apple, ObjFW and mulle-objc.
test-adapted *args: oz2c
    python3 -m pytest tests/adapted/ -v {{args}}


test-pal: oz2c
    python3 -m pytest tests/pal/ -v

# Both corpora under leak detection -- the gate `ci.yml`'s `leak-check` runs,
# reachable by name at last (#455).
#
# **Linux only, and it says so rather than skipping.** `-fsanitize=leak` does
# not exist on arm64 macOS, and neither does ASan's `detect_leaks=1`: the
# first fails to compile, the second aborts with "detect_leaks is not
# supported on this platform". A recipe that quietly passed on the machine
# that cannot run it would be the exact defect `ci.yml:473` names -- a check
# that holds nowhere -- so this one fails loudly and names CI as the place the
# gate lives.
#
# Takes the same `*args` as the corpora, so one case can be reproduced with
# `just test-leaks -k retain_release_balance`.
#
# Both corpora under leak detection (Linux only; the gate lives in CI).
test-leaks *args: oz2c
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "$(uname -s)" != "Linux" ]; then
        echo "just test-leaks: leak detection needs Linux." >&2
        echo "  -fsanitize=leak does not exist on arm64 macOS, and ASan's" >&2
        echo "  detect_leaks=1 aborts with 'not supported on this platform'." >&2
        echo "  The gate runs in CI as the 'leak-check' job; nothing local" >&2
        echo "  can stand in for it. Run the corpora without it instead:" >&2
        echo "    just test-behavior && just test-adapted" >&2
        exit 1
    fi
    python3 -m pytest tests/behavior/ -v --check-leaks {{args}}
    python3 -m pytest tests/adapted/ -v --check-leaks {{args}}


# Everything that runs on the host: both corpora through oz2c, the PAL's own
# C tests, and the transpile-and-compile smoke test. `test-all-transpiler` is
# gone with the Python backend -- it existed to run that pipeline's own unit
# suite alongside the corpora, and there is no second implementation to have
# a suite of its own now.
#
# Everything host-side: both corpora, the PAL's own C tests, and smoke.
test-all:
    just test-behavior
    just test-adapted
    just test-pal
    just smoke

test-ci-local:
    just test-all
    just test-behavior --compiler=clang
    just test-behavior --opt=O2
    just test-behavior --sanitize=address,undefined
    # The two cells CI has and this mirror did not (#455). The first is
    # ASan and LSan in one process, which nothing ran anywhere: the
    # `sanitizers` job passes `--sanitize` with no `--check-leaks`, so its
    # ASan leg runs with `detect_leaks=0` explicitly.
    just test-behavior --sanitize=address,undefined --check-leaks
    just test-leaks

test-regression: oz2c
    python3 -m pytest tests/behavior/ -v -k regression

smoke: oz2c
    python3 tests/smoke/run.py
