# SPDX-License-Identifier: Apache-2.0
#
# Transpiled LED demo — pure C, no ObjC runtime.

cmake_minimum_required(VERSION 3.20.0)

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(transpiled_led)

target_sources(app PRIVATE
    src/main.c
    src/generated/OZObject.c
    src/generated/OZLed.c
    src/generated/oz_dispatch.c
    src/generated/oz_mem_slabs.c
)

target_include_directories(app PRIVATE src)
