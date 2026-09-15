# SPDX-License-Identifier: Apache-2.0
cmake_minimum_required(VERSION 3.20.0)
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})

project(oz_transpiler_zephyr_tests)

# PAL in Zephyr mode
target_compile_definitions(app PRIVATE OZ_PLATFORM_ZEPHYR)

# Include paths
target_include_directories(app PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/../../include    # platform/ headers
    ${CMAKE_CURRENT_SOURCE_DIR}/generated              # transpiled output
    ${CMAKE_CURRENT_SOURCE_DIR}/generated/Foundation   # Foundation classes
)

# Test sources
target_sources(app PRIVATE
    src/main.c
    src/test_lifecycle.c
    src/test_dispatch.c
    src/test_memory.c
    src/test_protocol.c
    src/test_boxed_expr.c
    src/test_census.c
    src/test_freed_slot.c
)

# The refcount instruments (#452), on for this suite as of #490.
#
# It has to be app-wide rather than scoped to `test_freed_slot.c`: the
# poison it measures is emitted into `_oz_free`, which lives in the
# generated sources below, so a per-file definition would compile the test
# against a `Node_oz_free` that stamps nothing.
#
# Turning them on also gives the traps their only on-target coverage --
# nothing else here defines the macro, so every line of #452's and #490's C
# was reaching a board for the first time through this. All existing cases
# pass with it on, verified on mps2/an385 and qemu_cortex_a53.
target_compile_definitions(app PRIVATE OZ_DEBUG_REFCOUNT)

# Transpiled class sources
file(GLOB GENERATED_SOURCES "generated/*.c" "generated/Foundation/*.c")
target_sources(app PRIVATE ${GENERATED_SOURCES})
