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
	int _lock;
	int _key;
	id _obj;
}
- (instancetype)initWithObject:(id)obj;
- (void)dealloc;
@end
