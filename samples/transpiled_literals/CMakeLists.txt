# SPDX-License-Identifier: Apache-2.0
#
# Transpiled literals demo — pure C, no ObjC runtime.

cmake_minimum_required(VERSION 3.20.0)

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(transpiled_literals)

include(${CMAKE_CURRENT_SOURCE_DIR}/../../objc/cmake/oz_transpile.cmake)

objz_transpile_sources(app src/main.m)
