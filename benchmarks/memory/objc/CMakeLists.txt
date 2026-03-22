# SPDX-License-Identifier: Apache-2.0

cmake_minimum_required(VERSION 3.20.0)

set(ZEPHYR_EXTRA_MODULES "${CMAKE_CURRENT_SOURCE_DIR}/../../../")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(objc_mem_benchmark)

target_sources(app PRIVATE src/main.c)

objz_transpile_sources(app src/mem_helpers.m
    POOL_SIZES "MemBase=4,MemChild=24,MemGrandChild=24,OZSpinLock=2")
