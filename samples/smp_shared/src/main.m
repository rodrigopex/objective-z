/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Two cores, one object.
 *
 * Every other sample either has no threads or gives each thread its own
 * objects, so `@synchronized` and the refcount atomics have never faced
 * actual contention -- only the code paths that implement them. On
 * mps2/an385 and qemu_riscv32 `struct k_spinlock` has no members at all in
 * the configuration those samples ship (CONFIG_SMP off, and
 * CONFIG_SPIN_VALIDATE off because it sits behind CONFIG_ASSERT), so even
 * the lock's own state was unexercised. `just test-spin-validate` turns the
 * validator on and populates that struct on both boards (#278); the default
 * builds are still the empty-struct shape.
 *
 * This sample runs `main` and one worker thread on separate cores, both
 * hammering the *same* Counter:
 *
 *   - `-bump` reads, adds, and writes `_count` as three separate steps
 *     inside `@synchronized(self)`. If the lock does not serialize, updates
 *     are lost and the total comes out below 2 * ITERATIONS. The exact
 *     total is therefore a real assertion and not a smoke test: it fails if
 *     `@synchronized` is emitted as a no-op, if it locks a per-thread
 *     rather than a per-object lock, or if the PAL's spinlock is broken on
 *     this target.
 *
 *   - both sides also retain and release the shared object in a loop, so
 *     `oz_atomic_inc` / `oz_atomic_dec_and_test` are driven concurrently
 *     from two cores. A lost increment here would drop the refcount to zero
 *     early and free an object both cores are still using; the final
 *     retainCount check catches the arithmetic, and the object still being
 *     usable afterwards catches the free.
 *
 * This sample found a real defect, which is the reason it exists rather than
 * being another green sweep. `@synchronized` used to create a spinlock on the
 * *caller's own stack*, fresh per block, so two threads locked two different
 * locks and nothing was excluded between cores. It looked correct everywhere
 * it had been tested, because on a single core `k_spin_lock` disables
 * interrupts and that alone serializes the section. The lock is now a field
 * of the object (`oz_sync_lock` in the root struct), so the same object means
 * the same lock.
 *
 * The deliberate spin between the read and the write is what makes the count
 * assertion mean anything, and it took two attempts. Written as three tight
 * statements at 20000 iterations, the *unlocked* build also passed with a
 * perfect 40000: QEMU's TCG emulates the two cores in coarse translation
 * blocks and never interleaved a critical section that short. The assertion
 * would have looked strong and proved nothing -- and would have hidden the
 * defect above. With a 200-iteration volatile spin holding the window open,
 * and 2000 iterations to keep the runtime sane, the numbers separate cleanly:
 *
 *   per-block lock (before the fix):  count=2015 expected=4000
 *   no lock at all:                   count=2023 expected=4000
 *   per-object lock (now):            count=4000 expected=4000
 *
 * The first two lines are the point: the old lock was indistinguishable from
 * no lock. Worth re-running (delete the `@synchronized` and watch it fail)
 * before trusting this file.
 *
 * What this sample does *not* establish: that the refcount atomics are
 * necessary. `rc_after=1` held even in the unlocked build, because
 * retain/release go through real atomics rather than a plain
 * read-modify-write, and there is no equally simple way to un-atomic them
 * for a negative control. So the refcount lines confirm the atomics behave
 * correctly under two-core load; they do not prove the atomicity is load-
 * bearing the way the count line proves the lock is.
 */

#import <Foundation/Foundation.h>

/* Declared here so the Clang AST dump resolves without Zephyr's headers.
 * The signature matches Zephyr's own (void, not int) -- declaring
 * `int printk(...)` is what broke samples/pool_demo and
 * samples/transpiled_led on the first ARM cross-build. */
void printk(const char *fmt, ...);

#define ITERATIONS 2000
#define RETAIN_ITERATIONS 20000

@interface Counter: OZObject {
	int _count;
}
- (void)bump;
- (void)bumpNested;
- (int)count;
@end

@implementation Counter

- (id)init
{
	self = [super init];
	if (self != nil) {
		_count = 0;
	}
	return self;
}

/* Three statements, not `_count++`: see the file comment. */
- (void)bump
{
	@synchronized(self) {
		int current = _count;
		/* Widen the read-modify-write window deliberately: see the file
		 * comment. Without this, QEMU never interleaves the two cores
		 * inside so short a critical section and the unlocked build
		 * passes too, which would make the assertion meaningless. */
		for (volatile int d = 0; d < 200; d++) {
		}
		_count = current + 1;
	}
}

/* Re-entrant on purpose. A spinlock cannot be re-locked, so this is the
 * shape that would hang if the owner check were wrong. On host the PAL's
 * oz_spin_lock is a no-op, so a deadlock is unobservable there, which is
 * exactly how the hazard stayed theoretical -- for a while this sample was
 * the only place it could be falsified at all.
 *
 * It is now checked rather than merely survived: under
 * `just test-spin-validate` a second acquire fails Zephyr's own
 * z_spin_lock_valid() with `Invalid spinlock`, so the guard is confirmed by
 * a control that fails without it rather than by the run completing (#278).
 * samples/pool_demo carries the same shape across a method boundary, which
 * is what gets it checked on a single core too. */
- (void)bumpNested
{
	@synchronized(self) {
		[self bump];
	}
}

- (int)count
{
	return _count;
}

- (void)dealloc
{
	printk("Counter dealloc\n");
}

@end

/* The object both cores share. File scope so the worker thread can reach
 * it; `static Counter *` rather than `static struct Counter *` because the
 * transpiler adds the tag itself. */
static Counter *shared_counter;

/* Set by the worker when it is done. Plain `volatile int` rather than an
 * atomic: it is written by exactly one core and only ever read for a
 * change, so a torn read is not possible for a single aligned int, and
 * using the PAL's atomics here would test the wait rather than the lock. */
static volatile int worker_done;

/* Set by main once shared_counter is allocated, so the worker cannot start
 * hammering a nil object. See the wait in the worker entry. */
static volatile int counter_ready;

static void hammer(Counter *c)
{
	for (int i = 0; i < ITERATIONS; i++) {
		/* Half through the plain path, half re-entrant, so both the
		 * acquire and the skip-the-acquire branch face contention. */
		if ((i % 2) == 0) {
			[c bump];
		} else {
			[c bumpNested];
		}
	}
	for (int i = 0; i < RETAIN_ITERATIONS; i++) {
		[c retain];
		[c release];
	}
}

void smp_shared_worker_entry(void *p1, void *p2, void *p3)
{
	(void)p1;
	(void)p2;
	(void)p3;
	printk("worker started\n");
	/* K_THREAD_DEFINE starts this at boot with no delay, so on two cores it
	 * really does reach here before main has allocated the Counter -- the
	 * console shows "worker started" ahead of "counter created". Without
	 * this wait the worker would send to nil and lock through a null
	 * pointer. Found by reading the interleaved output rather than by a
	 * crash, which is the kind of thing that works until it does not.
	 *
	 * The flag is `volatile int` rather than a test of `shared_counter`
	 * itself: an object pointer cannot be volatile-qualified through the
	 * transpiler, and a non-volatile read is free to be hoisted out of this
	 * loop. */
	while (counter_ready == 0) {
	}
	hammer(shared_counter);
	worker_done = 1;
	printk("worker done\n");
}

K_THREAD_DEFINE(smp_shared_worker, 2048, smp_shared_worker_entry,
		NULL, NULL, NULL, 5, 0, 0);

int main(void)
{
	printk("=== SMP Shared Object Demo ===\n");

	shared_counter = [[Counter alloc] init];
	counter_ready = 1;
	printk("counter created, rc=%d\n", [shared_counter retainCount]);

	/* main is the second contender, so both cores are busy on the same
	 * object rather than one core working while the other waits. */
	hammer(shared_counter);
	printk("main done\n");

	while (worker_done == 0) {
		/* Spin rather than sleep: on two cores the worker is making
		 * progress on the other one, and a k_msleep call here would
		 * need a hand-written declaration for the Clang AST dump --
		 * the trap that produced conflicting `printk` declarations
		 * on the first cross-build. */
	}

	/* The whole point. Exact, not approximate: every one of the 2 *
	 * ITERATIONS increments has to have survived. */
	printk("count=%d expected=%d\n", [shared_counter count], 2 * ITERATIONS);

	/* Back to +1 after equal numbers of retain and release from two
	 * cores, and still alive to answer. */
	printk("rc_after=%d\n", [shared_counter retainCount]);
	printk("still usable, count=%d\n", [shared_counter count]);

	printk("=== Demo complete ===\n");
	return 0;
}
