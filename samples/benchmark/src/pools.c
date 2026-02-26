/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Static pool definition for PooledObj.
 * Must be .c — OZ_DEFINE_POOL uses SYS_INIT, incompatible with ObjC parser.
 */
#include <objc/pool.h>

/*
 * PooledObj instance size: isa (4) + _refcount (4) + _x (4) = 12 bytes.
 * Round to 16 for alignment. Pool holds 8 instances.
 */
OZ_DEFINE_POOL(PooledObj, 16, 8, 4);
