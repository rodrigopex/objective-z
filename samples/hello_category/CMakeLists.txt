# SPDX-License-Identifier: Apache-2.0

cmake_minimum_required(VERSION 3.20.0)

set(ZEPHYR_EXTRA_MODULES "${CMAKE_CURRENT_SOURCE_DIR}/../../")
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(hello_category)

target_include_directories(app PRIVATE include)

objz_transpile_sources(app
    src/Car.m
    src/Car+Maintenance.m
    src/main.m
)
