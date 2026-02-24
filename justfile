alias b := build
alias c := clean
alias r := run
alias f := flash
alias m := monitor
alias t := test

project_dir := "samples/hello_world"
board := "mps2/an385"
flags := ""
tty := "/dev/tty.usbmodem0006850372581"
gcc_path := "/opt/homebrew/"
sdk_path := "~/.local/zephyr-sdk-0.17.4"

env:
    export ZEPHYR_TOOLCHAIN_VARIANT=cross-compile
    export CROSS_COMPILE=/opt/homebrew/bin/arm-none-eabi-

rebuild:
    ZEPHYR_TOOLCHAIN_VARIANT=cross-compile CROSS_COMPILE={{ gcc_path }}bin/arm-none-eabi- west build -p -b {{ board }} {{ project_dir }} -- {{ flags }}

build:
    ZEPHYR_TOOLCHAIN_VARIANT=cross-compile CROSS_COMPILE={{ gcc_path }}bin/arm-none-eabi- west build -b {{ board }} {{ project_dir }} -- {{ flags }}

rebuild-clang:
    west build -p -b {{ board }} {{ project_dir }} -- -DCONFIG_OBJZ_USE_CLANG=y {{ flags }}

build-clang:
    west build -b {{ board }} {{ project_dir }} -- -DCONFIG_OBJZ_USE_CLANG=y {{ flags }}

flash:
    west flash

clean:
    rip build

run:
    west build -t run

monitor:
    tio {{ tty }}

test:
    west twister -T samples/ -p {{ board }} --force-toolchain -O /tmp/twister-out -x=ZEPHYR_TOOLCHAIN_VARIANT=cross-compile -x=CROSS_COMPILE={{ gcc_path }}bin/arm-none-eabi-

test-clang:
    west twister -T samples/ -p {{ board }} -O /tmp/twister-out -x=CONFIG_OBJZ_USE_CLANG=y
