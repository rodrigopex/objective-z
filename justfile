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

rebuild:
    west build -p -b {{ board }} {{ project_dir }} -- {{ flags }}

build:
    west build -b {{ board }} {{ project_dir }} -- {{ flags }}

flash:
    west flash

clean:
    rip build

# Every twister output directory for every checkout, at roughly a gigabyte
# each. `outdir` is keyed on the checkout, so a worktree that has been deleted
# leaves its sweep output behind with nothing left to name it -- this is the
# only thing that reaches those.
#
# `rm -rf`, not `rip`: `rip` moves the bytes to /tmp/graveyard-$USER, which
# frees no space at all and is the opposite of the point. Confirm with `df -h`
# rather than assuming.
#
# Deliberately not part of `clean`, which is scoped to the build directory:
# this destroys sweep output someone may still be reading.
#
# The glob is `twister-out*`, not `twister-out-*`, so it also reaches the
# single shared directory the recipes used before this scheme and any `.1`,
# `.2` rotation left over from before `-c` (#312) -- the same reason
# .gitignore spells it that way.
#
# Every checkout's twister output, ~1 GB each; not part of `clean` (#315).
clean-twister:
    rm -rf /tmp/twister-out*

run:
    west build -t run

monitor:
    tio {{ tty }}

# The transpiler, built once, before anything that configures a sample.
#
# Every sample's configure step runs this same cargo build itself
# (oz_static.cmake), so a sweep that starts with a rebuild pending has 13 of
# them invoking cargo -- each with its own rustc fan-out -- within the same
# second. Cargo's own locks make those builds correct, not cheap, and the
# load spike is what makes a configure step fail to *start* oz2c at all
# (#308). Every twister recipe below depends on this, so the binary is warm
# and no configure step has work to do. It also enforces the standing
# measurement rule: a sweep that picks up a new binary halfway through
# reports a blend of two versions.
#
# Build the transpiler once, before anything configures a sample (#308).
oz2c:
    cargo build --manifest-path tools/oz_static/Cargo.toml

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
# qemu_riscv32 has no led0/sw0 device-tree aliases, so 12 of 13 run here.
#
# The same samples on RISC-V (`qemu_riscv32`); 12 of 13 select.
test-riscv: oz2c
    west twister -T samples/ -p {{ riscv_board }} -c -O {{ outdir }}-riscv

# Two cores (CONFIG_SMP=y, CONFIG_MP_MAX_NUM_CPUS=2). Only the samples that
# pin no platform, plus arc_demo's own SMP scenarios -- see its sample.yaml for
# why the single-core expectations cannot be reused under real concurrency.
#
# The same samples on two cores, the only board with real lock contention.
test-smp: oz2c
    west twister -T samples/ -p {{ smp_board }} -c -O {{ outdir }}-smp

# Both supported boards, so an architecture-specific regression cannot hide.
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
# suites test different things (13 samples vs the ztest cases over committed C),
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

# The 76-case behavior corpus through oz_static, the default backend. This
# harness carries the compiler/-O matrix, the sanitizers, leak detection and
# gcov, so it is where those reach the *generated* C -- `cargo test`'s
# corpus_parity only transpiles and compiles each case, never runs it.
#
# The 76-case behavior corpus through oz_static (gcc/clang, -O0/-O2, ASan, LSan).
test-behavior *args:
    python3 -m pytest tests/behavior/ -v {{args}}

# 40 tests adapted from LLVM, GNUstep, Apple, ObjFW and mulle-objc.
test-adapted *args:
    python3 -m pytest tests/adapted/ -v {{args}}


test-pal:
    python3 -m pytest tests/pal/ -v


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
    just test-behavior -- --compiler=clang
    just test-behavior -- --opt=O2
    just test-behavior -- --sanitize=address,undefined

test-regression:
    python3 -m pytest tests/behavior/ -v -k regression

smoke:
    python3 tests/smoke/run.py
