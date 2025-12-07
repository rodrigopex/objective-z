# SPDX-License-Identifier: Apache-2.0
cmake_minimum_required(VERSION 3.20.0)

set(ZEPHYR_EXTRA_MODULES "${CMAKE_CURRENT_SOURCE_DIR}/../../objc/")

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})

project(zbus_objc_sample)

include_directories(include)

target_sources(app PRIVATE
    src/channels.c
    src/Producer.m
    src/consumer.c
    src/main.m
)
