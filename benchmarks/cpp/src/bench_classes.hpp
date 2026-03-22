/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * C++ Benchmark Class Hierarchy (OZ-070)
 *
 * Mirrors OZ BenchBase/BenchChild/BenchGrandChild for
 * apples-to-apples comparison of dispatch, lifecycle,
 * properties, synchronization, and reference counting.
 */
#ifndef BENCH_CLASSES_HPP
#define BENCH_CLASSES_HPP

#include <atomic>
#include <cstdint>
#include <zephyr/spinlock.h>

class BenchBase {
public:
	int x = 0;
	std::atomic<int> refcount{1};

	/* Property backing fields */
	int _value = 0;
	int _atomicValue = 0;
	struct k_spinlock _prop_lock;

	virtual ~BenchBase() = default;

	virtual void nop() { __asm__ volatile("" ::: "memory"); }

	virtual int getValue() { return x; }

	static void classNop() { __asm__ volatile("" ::: "memory"); }

	/* Property accessors (nonatomic — direct field access) */
	int value() const { return _value; }

	void setValue(int v) { _value = v; }

	/* Property accessors (atomic — k_spinlock guarded, same as OZ) */
	int atomicValue()
	{
		k_spinlock_key_t key = k_spin_lock(&_prop_lock);
		int val = _atomicValue;
		k_spin_unlock(&_prop_lock, key);
		return val;
	}

	void setAtomicValue(int v)
	{
		k_spinlock_key_t key = k_spin_lock(&_prop_lock);
		_atomicValue = v;
		k_spin_unlock(&_prop_lock, key);
	}

	/* @synchronized equivalent — k_spinlock (same primitive as OZ) */
	void syncNop()
	{
		k_spinlock_key_t key = k_spin_lock(&_prop_lock);
		__asm__ volatile("" ::: "memory");
		k_spin_unlock(&_prop_lock, key);
	}

	void retain() { refcount.fetch_add(1, std::memory_order_relaxed); }

	void release()
	{
		if (refcount.fetch_sub(1, std::memory_order_acq_rel) == 1) {
			delete this;
		}
	}
};

class BenchChild : public BenchBase {
};

class BenchGrandChild : public BenchChild {
};

/* For placement-new lifecycle test */
class PooledObj {
public:
	int x = 0;

	virtual ~PooledObj() = default;

	virtual void nop() { __asm__ volatile("" ::: "memory"); }
};

/* Boxed integer for fair collection comparison with OZNumber */
struct BoxedInt {
	int32_t val;
};

#endif /* BENCH_CLASSES_HPP */
