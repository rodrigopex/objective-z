# SPDX-License-Identifier: Apache-2.0

cmake_minimum_required(VERSION 3.20.0)

set(ZEPHYR_EXTRA_MODULES "${CMAKE_CURRENT_SOURCE_DIR}/../../")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(benchmark)

objz_transpile_sources(app src/main.m
    POOL_SIZES "BenchBase=8,BenchChild=4,BenchGrandChild=4,OZFixedPoint=16,OZArray=4,OZDictionary=4,OZString=16,OZSpinLock=4")
