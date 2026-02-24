/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Static pool definitions for the pool_demo sample.
 * Must be a .c file — OZ_DEFINE_POOL uses SYS_INIT which
 * generates C constructs incompatible with the ObjC parser.
 */
#include <objc/pool.h>

/*
 * Sensor instance size: isa (4) + _refcount (4) + _value (4) = 12 bytes.
 * Round to 16 for alignment. Pool holds 4 instances.
 */
OZ_DEFINE_POOL(Sensor, 16, 4, 4);
