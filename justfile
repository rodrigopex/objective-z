alias b := build
alias c := clean
alias r := run
alias f := flash
alias m := monitor
alias t := test

project_dir := "samples/hello_world"
board := "mps2/an385"
riscv_board := "qemu_riscv32"
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

test-all-transpiler:
    just test-transpiler
    just test-behavior
    just test-adapted
    just test-pal

smoke:
    python3 tests/smoke/run.py

update-golden:
    PYTHONPATH=tools python3 tools/oz_transpile/tests/golden/update.py
