# SPDX-License-Identifier: Apache-2.0

cmake_minimum_required(VERSION 3.20.0)

set(ZEPHYR_EXTRA_MODULES "${CMAKE_CURRENT_SOURCE_DIR}/../../")
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(transpiled_led)

target_include_directories(app PRIVATE src)

objz_transpile_sources(app src/OZLed.m src/main.m)
