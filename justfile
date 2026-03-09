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
    west twister -T samples/ -T tests/objc-reference/ -p {{ board }} -O /tmp/twister-out

test-riscv:
    west twister -T samples/ -T tests/objc-reference/ -p {{ riscv_board }} -O /tmp/twister-out

test-all:
    west twister -T samples/ -T tests/objc-reference/ -p {{ board }} -p {{ riscv_board }} -O /tmp/twister-out

bench:
    west build -p -b {{ board }} benchmarks/objc && west build -t run

bench-cpp:
    west build -p -b {{ board }} benchmarks/cpp && west build -t run

bench-rust:
    west build -p -b {{ board }} benchmarks/rust && west build -t run

bench-zig:
    west build -p -b {{ board }} benchmarks/zig && west build -t run

bench-c3:
    west build -p -b {{ board }} benchmarks/c3 && west build -t run

bench-mem-c:
    west build -p -b {{ board }} benchmarks/memory/c && west build -t run

bench-mem-cpp:
    west build -p -b {{ board }} benchmarks/memory/cpp && west build -t run

bench-mem-rust:
    west build -p -b {{ board }} benchmarks/memory/rust && west build -t run

bench-mem-objc:
    west build -p -b {{ board }} benchmarks/memory/objc && west build -t run

bench-mem-zig:
    west build -p -b {{ board }} benchmarks/memory/zig && west build -t run

bench-mem:
    just bench-mem-c
    just bench-mem-cpp
    just bench-mem-rust
    just bench-mem-zig
    just bench-mem-objc

ast-dump file *includes:
    clang -Xclang -ast-dump=json -fsyntax-only {{includes}} {{file}} 2>/dev/null

transpile *args:
    PYTHONPATH=tools python3 -m oz_transpile {{args}}

test-transpiler:
    python3 -m pytest tools/oz_transpile/tests/ -v

test-behavior:
    python3 -m pytest test/behavior/ -v

smoke:
    python3 test/smoke/run.py

update-golden:
    PYTHONPATH=tools python3 tools/oz_transpile/tests/golden/update.py
