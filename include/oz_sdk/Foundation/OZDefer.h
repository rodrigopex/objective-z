/**
 * @file OZDefer.h
 * @brief Deferred cleanup object for OZ transpiler.
 *
 * OZDefer stores a non-capturing block and a non-retained owner reference.
 * When OZDefer is deallocated, it calls the block passing the owner pointer.
 * The owner is __unsafe_unretained to avoid retain cycles when OZDefer is
 * stored as an ivar of the owner.
 */
#pragma once
#import "OZObject.h"

@interface OZDefer : OZObject {
	__unsafe_unretained id _owner;
	void (^_block)(id);
}
- (instancetype)initWithOwner:(id)owner block:(void (^)(id))aBlock;
- (instancetype)initWithBlock:(void (^)(id))aBlock;
- (void)dealloc;
@end
