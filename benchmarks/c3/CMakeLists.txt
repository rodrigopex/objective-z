# SPDX-License-Identifier: Apache-2.0

cmake_minimum_required(VERSION 3.20.0)

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(c3_benchmark)

target_sources(app PRIVATE src/entry.c src/timing_shim.c)

include(${CMAKE_CURRENT_SOURCE_DIR}/../cmake/C3Compile.cmake)
c3_find_compiler()
c3_compile_sources(app src/main.c3)
