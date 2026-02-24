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

rebuild:
    west build -p -b {{ board }} {{ project_dir }} -- -DCONFIG_OBJZ_USE_CLANG=y {{ flags }}

build:
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
    west twister -T samples/ -p {{ board }} -O /tmp/twister-out -x=CONFIG_OBJZ_USE_CLANG=y
