# SPDX-License-Identifier: Apache-2.0

cmake_minimum_required(VERSION 3.20.0)

set(ZEPHYR_EXTRA_MODULES "${CMAKE_CURRENT_SOURCE_DIR}/../../")
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(transpiled_literals)

objz_transpile_sources(app src/main.m
    POOL_SIZES OZObject=1,OZQ31=10,OZString=10,OZArray=2,OZDictionary=2)
