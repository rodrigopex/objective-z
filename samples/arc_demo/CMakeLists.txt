# SPDX-License-Identifier: Apache-2.0

cmake_minimum_required(VERSION 3.20.0)

set(ZEPHYR_EXTRA_MODULES "${CMAKE_CURRENT_SOURCE_DIR}/../../")
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(arc_demo)

# Sensor gets one slot more than the counted allocation sites, so the
# reassignment loop in main() cannot race the Sensor that
# arc_demo_extra_thread_entry's Driver holds. The loop itself needs only one
# slot -- that is the property it demonstrates -- but its release-then-realloc
# briefly leaves the slot free, and pinning the pool to exactly 1 would make
# the sample depend on the two never interleaving.
objz_transpile_sources(app src/main.m POOL_SIZES Sensor=2)
