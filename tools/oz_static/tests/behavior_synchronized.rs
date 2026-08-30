// SPDX-License-Identifier: Apache-2.0
//
// behavior_synchronized.rs - ports the oracle's @synchronized suite
// (tests/behavior/cases/synchronized/*.m and their _test.c drivers):
// basic_lock, counter, nested, with_locals, early_return.
//
// @synchronized lowers to a scoped critical section over a per-block
// lock -- see `emit::render_synchronized_statement` for why the lock is
// per block rather than per object, and how a `return` out of the body
// replays the pending unlock.
//
// On the host PAL `oz_spin_lock`/`oz_spin_unlock` are no-ops (single
// threaded), the same as they are for the oracle's own host-side behavior
// tests, so what these assert is that the lowering produces correct,
// compilable C with the body's effects intact and the lock/unlock
// balanced -- not mutual exclusion under real contention, which needs
// Zephyr.

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
	int _count;
}
- (void)increment;
- (int)count;
@end
@implementation SyncCounter
- (void)increment {
	@synchronized(self) {
		_count = _count + 1;
	}
}
- (int)count {
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
    // Ported from nested.m / nested_test.c. Note the two receivers may
    // alias (`[n runNested:n]` below): a per-object lock would
    // self-deadlock here on Zephyr, which is precisely why the lowering
    // uses a fresh lock per block -- see
    // `emit::render_synchronized_statement`.
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
