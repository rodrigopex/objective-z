/**
 * @file OZSpinLock.h
 * @brief RAII spinlock class for @synchronized support.
 *
 * Lightweight ObjC interface that Clang can parse for AST dump.
 * The transpiler emits a pure-C struct backed by platform spinlock primitives.
 */
#pragma once
#import "OZObject.h"

@interface OZSpinLock : OZObject
{
	/*
	 * Only the object. `_lock`/`_key` used to sit here too and were
	 * never read: `@synchronized` locks a *synthesized* `oz_sync_lock`
	 * field on the object being synchronized, not anything on this
	 * class (see `emit::render_synchronized`). They survived
	 * `no_dead_ivars` only because the dead `include/platform/oz_lock.h`
	 * mentioned them, and went with it in #417.
	 */
	id _obj;
}
- (instancetype)initWithObject:(id)obj;
- (void)dealloc;
@end
