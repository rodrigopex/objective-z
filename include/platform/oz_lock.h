/* OZSpinLock — RAII spinlock for @synchronized support */
#pragma once

#include "oz_platform.h"

struct OZObject;

struct OZSpinLock {
	struct OZObject base;
	oz_spinlock_t _lock;
	oz_spinlock_key_t _key;
	struct OZObject *_obj;
};
