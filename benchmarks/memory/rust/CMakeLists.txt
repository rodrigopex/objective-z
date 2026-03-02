# SPDX-License-Identifier: Apache-2.0

cmake_minimum_required(VERSION 3.20.0)

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(rust_mem_benchmark)

rust_cargo_application()

target_sources(app PRIVATE src/heap_shim.c)
