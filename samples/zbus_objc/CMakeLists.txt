# SPDX-License-Identifier: Apache-2.0
cmake_minimum_required(VERSION 3.20.0)

set(ZEPHYR_EXTRA_MODULES "${CMAKE_CURRENT_SOURCE_DIR}/../../")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})

project(zbus_objc_sample)

target_include_directories(app PRIVATE include)

objz_transpile_sources(app
    src/Producer.m
    src/main.m
)

target_sources(app PRIVATE
    src/channels.c
    src/consumer.c
)
