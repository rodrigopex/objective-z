/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * C++ Benchmark Class Hierarchy
 *
 * Mirrors ObjC BenchBase/BenchChild/BenchGrandChild/PooledObj
 * for apples-to-apples comparison of virtual dispatch, lifecycle,
 * and reference counting.
 */
#ifndef BENCH_CLASSES_HPP
#define BENCH_CLASSES_HPP

#include <atomic>
#include <cstdint>

class BenchBase {
public:
        int x = 0;
        std::atomic<int> refcount{1};

        virtual ~BenchBase() = default;

        virtual void nop() { __asm__ volatile("" ::: "memory"); }

        virtual int getValue() { return x; }

        static void classNop() { __asm__ volatile("" ::: "memory"); }

        void retain() { refcount.fetch_add(1, std::memory_order_relaxed); }

        void release()
        {
                if (refcount.fetch_sub(1, std::memory_order_acq_rel) == 1) {
                        delete this;
                }
        }
};

class BenchChild : public BenchBase {};

class BenchGrandChild : public BenchChild {};

class PooledObj {
public:
        int x = 0;

        virtual ~PooledObj() = default;

        virtual void nop() { __asm__ volatile("" ::: "memory"); }
};

#endif /* BENCH_CLASSES_HPP */
