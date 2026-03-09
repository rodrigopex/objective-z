# SPDX-License-Identifier: Apache-2.0
cmake_minimum_required(VERSION 3.20.0)
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})

project(oz_transpiler_zephyr_tests)

# PAL in Zephyr mode
target_compile_definitions(app PRIVATE OZ_PLATFORM_ZEPHYR)

# Include paths
target_include_directories(app PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/../../include    # oz/platform/ headers
    ${CMAKE_CURRENT_SOURCE_DIR}/generated        # transpiled output
)

# Test sources
target_sources(app PRIVATE
    src/main.c
    src/test_lifecycle.c
    src/test_dispatch.c
    src/test_memory.c
    src/test_protocol.c
)

# Transpiled class sources
file(GLOB GENERATED_SOURCES "generated/*.c")
target_sources(app PRIVATE ${GENERATED_SOURCES})
