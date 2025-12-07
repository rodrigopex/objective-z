alias b := build
alias c := clean
alias r := run

project_dir := "samples/hello_world"

env:
    export ZEPHYR_TOOLCHAIN_VARIANT=cross-compile
    export CROSS_COMPILE=/opt/homebrew/bin/arm-none-eabi-

rebuild: 
    west build -p -b qemu_cortex_m3 {{project_dir}}

build: 
    west build -b qemu_cortex_m3 {{project_dir}}

clean:
    rip build

run:
    west build -t run
