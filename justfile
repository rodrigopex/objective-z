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
export ZEPHYR_TOOLCHAIN_VARIANT := "cross-compile"
export CROSS_COMPILE := gcc_path + "bin/arm-none-eabi-"

env:
    export ZEPHYR_TOOLCHAIN_VARIANT=cross-compile
    export CROSS_COMPILE=/opt/homebrew/bin/arm-none-eabi-

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
    west twister -T samples/ -p {{ board }} --force-toolchain -x=ZEPHYR_TOOLCHAIN_VARIANT=cross-compile -x=CROSS_COMPILE={{ gcc_path }}bin/arm-none-eabi-
