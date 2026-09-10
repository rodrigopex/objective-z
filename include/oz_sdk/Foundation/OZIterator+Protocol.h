/**
 * @file OZIterator+Protocol.h
 * @brief Protocol for OZ collections a for-in loop can walk.
 *
 * A class conforming to OZIteratorProtocol answers -iter with a cursor
 * positioned before its first element and -next with each element in
 * turn, nil once exhausted. That pair is what `for (id x in c)` lowers
 * to, and oz2c keys the lowering on the two selectors rather than on
 * conformance -- so declaring them is what makes a class enumerable,
 * and adopting this is what says so out loud.
 */
#pragma once

#import "OZObject.h"

@protocol OZIteratorProtocol <OZObjectProtocol>

@required
@property (nonatomic, readonly) uint16_t iterIdx;

- (instancetype)iter;
- (id)next;

@end
