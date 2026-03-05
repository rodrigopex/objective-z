# SPDX-License-Identifier: Apache-2.0

cmake_minimum_required(VERSION 3.20.0)

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(zig_benchmark)

target_sources(app PRIVATE src/entry.c)

include(${CMAKE_CURRENT_SOURCE_DIR}/../cmake/ZigCompile.cmake)
zig_find_compiler()
zig_compile_sources(app src/main.zig)
