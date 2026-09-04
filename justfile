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

rebuild:
    west build -p -b {{ board }} {{ project_dir }} -- {{ flags }}

build:
    west build -b {{ board }} {{ project_dir }} -- {{ flags }}

flash:
    west flash

clean:
    rip build

run:
    west build -t run

monitor:
    tio {{ tty }}

test:
    west twister -T samples/ -p {{ board }} -O /tmp/twister-out

# Same samples on RISC-V. gpio_demo is filtered out by its own sample.yaml:
# qemu_riscv32 has no led0/sw0 device-tree aliases, so 12 of 13 run here.
test-riscv:
    west twister -T samples/ -p {{ riscv_board }} -O /tmp/twister-out-riscv

# Two cores (CONFIG_SMP=y, CONFIG_MP_MAX_NUM_CPUS=2). Only the samples that
# pin no platform, plus arc_demo's own SMP scenarios -- see its sample.yaml for
# why the single-core expectations cannot be reused under real concurrency.
test-smp:
    west twister -T samples/ -p {{ smp_board }} -O /tmp/twister-out-smp

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
test-pedantic *args:
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
test-spin-validate:
    west twister -T samples/ -p {{ board }} -O /tmp/twister-out-spinvalidate \
        -x=EXTRA_CONF_FILE={{ justfile_directory() }}/samples/overlay-spin-validate.conf
    west twister -T samples/ -p {{ smp_board }} -O /tmp/twister-out-spinvalidate-smp \
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
test-hardware:
    west twister -T samples/ -p {{ hw_board }} -O /tmp/twister-out-hw \
        --device-testing --hardware-map hardware-map.yaml

# Every board, including SMP. The only recipe that exercises two cores.
test-all-boards:
    just test
    just test-riscv
    just test-smp

test-zephyr:
    west twister -T tests/zephyr/ -p {{ if os() == "linux" { "native_sim" } else { board } }} -O /tmp/twister-out

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

test-bench:
    west twister -T benchmarks/ --device-testing --hardware-map hardware-map.yaml -O /tmp/twister-out

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

# The 71-case behavior corpus through oz_static, the default backend. This
# harness carries the compiler/-O matrix, the sanitizers, leak detection and
# gcov, so it is where those reach the *generated* C -- `cargo test`'s
# corpus_parity only transpiles and compiles each case, never runs it.
#
# The 71-case behavior corpus through oz_static (gcc/clang, -O0/-O2, ASan, LSan).
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
