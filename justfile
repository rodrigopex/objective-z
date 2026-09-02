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

transpile *args:
    PYTHONPATH=tools python3 -m oz_transpile {{args}}

test-transpiler:
    python3 -m pytest tools/oz_transpile/tests/ -v

test-behavior:
    python3 -m pytest tests/behavior/ -v

test-adapted:
    python3 -m pytest tests/adapted/ -v

test-pal:
    python3 -m pytest tests/pal/ -v

# Run the behavior corpus through BOTH backends and diff the Unity results.
# Proves the two agree on what the code *does*, not just that both compile.
# Set OZ_CLANG to the Zephyr SDK's clang (see CLAUDE.md) for the version the
# project is tested against; otherwise whatever clang is on PATH is used.
test-cross-backend *args:
    cargo build --manifest-path tools/oz_static/Cargo.toml
    python3 tests/tools/cross_backend.py --all {{args}}

test-all-transpiler:
    just test-transpiler
    just test-behavior
    just test-adapted
    just test-pal

test-all:
    just test-transpiler
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

update-golden:
    PYTHONPATH=tools python3 tools/oz_transpile/tests/golden/update.py
