# SPDX-License-Identifier: Apache-2.0

cmake_minimum_required(VERSION 3.20.0)

set(ZEPHYR_EXTRA_MODULES "${CMAKE_CURRENT_SOURCE_DIR}/../../objc/")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(hello_world)

include(${CMAKE_CURRENT_SOURCE_DIR}/../../objc/cmake/ObjcClang.cmake)
objz_target_sources(app src/main.m)
