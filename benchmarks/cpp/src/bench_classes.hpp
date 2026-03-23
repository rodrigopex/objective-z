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

/*
 * SimpleString — mirrors OZString layout for fair collection comparison.
 * Stores const char * + length, with a virtual length() accessor.
 */
class SimpleString {
public:
	const char *_data;
	unsigned int _length;

	SimpleString(const char *s, unsigned int len) : _data(s), _length(len) {}

	virtual ~SimpleString() = default;

	virtual unsigned int length() { return _length; }
};

/*
 * Polymorphic container with standard C++ iterators.
 * Uses virtual begin()/end() + virtual operator++ and operator*
 * to match OZ's for-in overhead (vtable dispatch per step).
 *
 * OZ for-in: vtable dispatch on iter() + next() per element.
 * C++ range-for: vtable dispatch on operator++ and operator* per element.
 */
class IIterator {
public:
	virtual ~IIterator() = default;
	virtual IIterator &operator++() = 0;
	virtual SimpleString *operator*() const = 0;
	virtual bool operator!=(const IIterator &other) const = 0;
};

class StringArray {
public:
	class Iterator : public IIterator {
	public:
		SimpleString **ptr;

		explicit Iterator(SimpleString **p) : ptr(p) {}

		Iterator &operator++() override
		{
			++ptr;
			return *this;
		}

		SimpleString *operator*() const override { return *ptr; }

		bool operator!=(const IIterator &other) const override
		{
			return ptr != static_cast<const Iterator &>(other).ptr;
		}
	};

	SimpleString **items;
	int count;

	StringArray(SimpleString **items_, int count_)
		: items(items_), count(count_) {}

	Iterator begin() { return Iterator(items); }
	Iterator end() { return Iterator(items + count); }
};

#endif /* BENCH_CLASSES_HPP */
