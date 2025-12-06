
env:
    export ZEPHYR_TOOLCHAIN_VARIANT=cross-compile
    export CROSS_COMPILE=/opt/homebrew/bin/arm-none-eabi-

rebuild: 
    west build -p -b qemu_cortex_m3

build: 
    west build -b qemu_cortex_m3

clean:
    rip build

run:
    west build -t run
