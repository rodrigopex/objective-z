// SPDX-License-Identifier: Apache-2.0
//
// behavior_synchronized.rs - ports the oracle's @synchronized suite
// (tests/behavior/cases/synchronized/*.m and their _test.c drivers):
// basic_lock, counter, nested, with_locals, early_return.
//
// @synchronized lowers to a scoped critical section over the *object's own*
// lock -- a field in the root struct -- so two threads synchronizing on the
// same object contend on the same lock. See
// `emit::render_synchronized_statement`, and how a `return` out of the body
// replays the pending unlock.
//
// It used to be a lock declared inside the block, on the caller's own stack,
// fresh per call. That excluded nothing between cores, and looked correct
// because on a single core `k_spin_lock` disables interrupts and that alone
// serializes the section. `samples/smp_shared` measures the difference on two
// cores: `count=2015 expected=4000` with the old per-block lock, 4000 with the
// per-object one.
//
// On the host PAL `oz_spin_lock`/`oz_spin_unlock` are no-ops (single
// threaded), the same as they are for the oracle's own host-side behavior
// tests, so what these assert is that the lowering produces correct,
// compilable C with the body's effects intact and the lock/unlock
// balanced -- not mutual exclusion under real contention, which needs
// Zephyr and two cores (`just test-smp`).

mod common;
use common::{compile_and_run, expect_reject, ozobject_src};

#[test]
fn basic_lock_runs_body() {
    // Ported from basic_lock.m / basic_lock_test.c.
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface LockTest : OZObject {
	int _flag;
}
- (void)run;
- (int)flag;
@end
@implementation LockTest
- (void)run {
	@synchronized(self) {
		_flag = 42;
	}
}
- (int)flag {
	return _flag;
}
@end

#include <stdio.h>
int main(void) {
	LockTest *t = [LockTest alloc];
	printf(\"flag_before=%d\\n\", [t flag]);
	[t run];
	printf(\"flag_after=%d\\n\", [t flag]);
	[t release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "sync_basic_lock_runs_body");
    assert_eq!(stdout, "flag_before=0\nflag_after=42\n");
}

#[test]
fn counter_increments_under_lock() {
    // Ported from counter.m / counter_test.c: repeated entry into the
    // same @synchronized block must accumulate, i.e. the lock is
    // acquired and released each time rather than left held.
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface SyncCounter : OZObject {
	size_t _count;
}
- (void)increment;
- (size_t)count;
@end
@implementation SyncCounter
- (void)increment {
	@synchronized(self) {
		_count = _count + 1;
	}
}
- (size_t)count {
	return _count;
}
@end

#include <stdio.h>
int main(void) {
	SyncCounter *c = [SyncCounter alloc];
	for (int i = 0; i < 5; i++) {
		[c increment];
	}
	printf(\"count=%d\\n\", [c count]);
	[c release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "sync_counter_increments_under_lock");
    assert_eq!(stdout, "count=5\n");
}

#[test]
fn nested_synchronized_blocks() {
    // Ported from nested.m / nested_test.c. The two receivers may alias
    // (`[n runNested:n]` below), which is the case that matters here: one
    // object locked twice by one thread. A per-object lock without owner
    // tracking deadlocks on it on Zephyr while passing here, because the host
    // PAL's `oz_spin_lock` is a no-op -- so for a long time this test passed
    // for a reason unrelated to what it was testing. `oz_sync_owner` makes the
    // second entry skip the acquire, so it now passes because the lowering is
    // right. See `emit::render_synchronized_statement`, and
    // `samples/smp_shared` for the same shape against a real spinlock.
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface NestLock : OZObject {
	int _outer;
	int _inner;
}
- (void)runNested:(NestLock *)other;
- (int)outer;
- (int)inner;
@end
@implementation NestLock
- (void)runNested:(NestLock *)other {
	@synchronized(self) {
		_outer = 1;
		@synchronized(other) {
			_inner = 2;
		}
	}
}
- (int)outer {
	return _outer;
}
- (int)inner {
	return _inner;
}
@end

#include <stdio.h>
int main(void) {
	NestLock *a = [NestLock alloc];
	NestLock *b = [NestLock alloc];
	[a runNested:b];
	printf(\"outer=%d\\n\", [a outer]);
	printf(\"inner=%d\\n\", [a inner]);

	/* Same object as both receivers -- must not deadlock. */
	NestLock *n = [NestLock alloc];
	[n runNested:n];
	printf(\"self_outer=%d\\n\", [n outer]);
	printf(\"self_inner=%d\\n\", [n inner]);

	[a release];
	[b release];
	[n release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "sync_nested_synchronized_blocks");
    assert_eq!(stdout, "outer=1\ninner=2\nself_outer=1\nself_inner=2\n");
}

#[test]
fn locals_declared_inside_synchronized() {
    // Ported from with_locals.m / with_locals_test.c: a local object
    // allocated inside the body. oz_static has no ARC (#189), so the
    // local is released explicitly here rather than at scope exit.
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface SyncLocal : OZObject {
	int _marker;
}
- (void)run;
- (int)marker;
@end
@implementation SyncLocal
- (void)run {
	@synchronized(self) {
		SyncLocal *tmp = [SyncLocal alloc];
		_marker = 1;
		[tmp release];
	}
}
- (int)marker {
	return _marker;
}
@end

#include <stdio.h>
int main(void) {
	SyncLocal *s = [SyncLocal alloc];
	[s run];
	printf(\"marker=%d\\n\", [s marker]);
	[s release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "sync_locals_declared_inside_synchronized");
    assert_eq!(stdout, "marker=1\n");
}

#[test]
fn early_return_from_synchronized() {
    // Ported from early_return.m / early_return_test.c: `return` inside
    // the body must still release the lock on the way out, and the
    // returned expression must be evaluated while still holding it.
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface EarlyRet : OZObject {
	int _value;
}
- (int)compute;
- (int)value;
@end
@implementation EarlyRet
- (int)compute {
	@synchronized(self) {
		_value = 77;
		return _value;
	}
	return -1;
}
- (int)value {
	return _value;
}
@end

#include <stdio.h>
int main(void) {
	EarlyRet *e = [EarlyRet alloc];
	printf(\"computed=%d\\n\", [e compute]);
	printf(\"value=%d\\n\", [e value]);
	/* Entering again must still work -- proof the lock was released. */
	printf(\"computed_again=%d\\n\", [e compute]);
	[e release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "sync_early_return_from_synchronized");
    assert_eq!(stdout, "computed=77\nvalue=77\ncomputed_again=77\n");
}

/// `break` out of a @synchronized body would skip the unlock, leaving the
/// lock held; unlike `return` there is no value to hand back and no
/// oracle case needs it, so it stays a hard error rather than a silent
/// deadlock. A `break` belonging to a loop *inside* the body is fine --
/// `counter_increments_under_lock` and this test's accepted half cover
/// that.
#[test]
fn break_escaping_synchronized_rejected() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface BreakSync : OZObject
- (void)run;
@end
@implementation BreakSync
- (void)run {
	while (1) {
		@synchronized(self) {
			break;
		}
	}
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'break' inside @synchronized"), "diagnostics: {}", diags);
}

#[test]
fn break_inside_loop_within_synchronized_accepted() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface LoopSync : OZObject {
	int _n;
}
- (void)run;
- (int)n;
@end
@implementation LoopSync
- (void)run {
	@synchronized(self) {
		for (int i = 0; i < 10; i++) {
			if (i == 3) {
				break;
			}
			_n = i;
		}
	}
}
- (int)n {
	return _n;
}
@end

#include <stdio.h>
int main(void) {
	LoopSync *l = [LoopSync alloc];
	[l run];
	printf(\"n=%d\\n\", [l n]);
	[l release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "sync_break_inside_loop_within_synchronized");
    assert_eq!(stdout, "n=2\n");
}

/// The lock is the object's own field, not a fresh one per block.
///
/// This is the assertion that separates the two designs. Both compile and
/// both pass every single-threaded test, so only the *shape* of the emitted
/// C distinguishes them here; `samples/smp_shared` is what distinguishes
/// them by behaviour, and it needs two cores.
#[test]
fn synchronized_locks_the_objects_own_field() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Box : OZObject { int _n; }
- (void)set:(int)v;
@end
@implementation Box
- (void)set:(int)v
{
	@synchronized(self) {
		_n = v;
	}
}
@end
int main(void) { return 0; }
"
    );
    let out = oz_static::transpile(&src).expect("should transpile").source_c;
    assert!(
        out.contains("->oz_sync_lock"),
        "@synchronized must lock the object's own field; got:\n{}",
        out
    );
    assert!(
        !out.contains("oz_spinlock_t _oz_sync_lock"),
        "a per-block lock on the caller's stack excludes nothing between cores:\n{}",
        out
    );
}

/// The field is only added when the program actually uses `@synchronized`,
/// the same way `oz_prop_lock` is only added for atomic properties. On a
/// single-core target `struct k_spinlock` has no members, so this costs
/// nothing there either way -- but an unused field in every object is still
/// a footprint claim this project should not make idly.
#[test]
fn no_sync_lock_field_without_synchronized() {
    let src = format!("{}{}", ozobject_src(), "int main(void) { return 0; }\n");
    let out = oz_static::transpile(&src).expect("should transpile");
    let all = format!("{}{}", out.source_c, out.companion_h);
    assert!(
        !all.contains("oz_sync_lock"),
        "no @synchronized in the program, so no lock field:\n{}",
        all
    );
}

/// Nesting on the *same* object works, which is what real Objective-C does.
///
/// A spinlock cannot be re-locked, so the second entry must not attempt the
/// acquire at all: generated code records the owning thread on the object and
/// the inner block sees itself as already holding it, skipping both the lock
/// and the unlock. Only the outermost block releases.
///
/// This is exercised on host because `oz_current_thread()` returns a stable
/// identity there too -- the branch is real even though the spinlock beneath
/// it is a no-op.
#[test]
fn nesting_on_the_same_object_does_not_deadlock() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#include <stdio.h>
@interface Box : OZObject { int _n; }
- (void)twice;
- (int)n;
@end
@implementation Box
- (void)twice
{
	@synchronized(self) {
		_n = _n + 1;
		@synchronized(self) {
			_n = _n + 10;
			@synchronized(self) {
				_n = _n + 100;
			}
		}
	}
}
- (int)n { return _n; }
@end
int main(void) {
	Box *b = [[Box alloc] init];
	[b twice];
	/* Entering again proves the outermost block really did release. */
	[b twice];
	printf(\"n=%d\\n\", [b n]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "nesting_on_the_same_object_does_not_deadlock");
    assert_eq!(stdout, "n=222\n");
}

/// Nesting on *different* objects stays supported -- it is what the oracle's
/// own `tests/behavior/cases/synchronized/nested.m` does, and rejecting it
/// would break a corpus case for no reason. Two distinct objects mean two
/// distinct locks.
#[test]
fn nesting_on_different_objects_is_accepted() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#include <stdio.h>
@interface Box : OZObject { int _outer; int _inner; }
- (void)runNested:(Box *)other;
- (int)outer;
- (int)inner;
@end
@implementation Box
- (void)runNested:(Box *)other
{
	@synchronized(self) {
		_outer = 1;
		@synchronized(other) {
			_inner = 2;
		}
	}
}
- (int)outer { return _outer; }
- (int)inner { return _inner; }
@end
int main(void) {
	Box *a = [[Box alloc] init];
	Box *b = [[Box alloc] init];
	[a runNested:b];
	printf(\"outer=%d inner=%d\\n\", [a outer], [a inner]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "nesting_on_different_objects_is_accepted");
    assert_eq!(stdout, "outer=1 inner=2\n");
}
