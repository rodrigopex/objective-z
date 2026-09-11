/**
 * @file OZIteratorProtocol.h
 * @brief Protocol for OZ collections a for-in loop can walk.
 *
 * A class conforming to OZIteratorProtocol answers -objectEnumerator with a cursor
 * positioned before its first element and -nextObject with each element in
 * turn, nil once exhausted. That pair is what `for (id x in c)` lowers
 * to, and oz2c keys the lowering on the two selectors rather than on
 * conformance -- so declaring them is what makes a class enumerable,
 * and adopting this is what says so out loud.
 */
#pragma once

#import "OZObject.h"

@protocol OZIteratorProtocol <OZObjectProtocol>

@required
/*
 * Atomicity is deliberately unconstrained: `readonly` and nothing more.
 * It used to require `nonatomic` while both implementors declared plain
 * `readonly`, so the requirement and every class satisfying it disagreed
 * (#413). Aligning the *protocol* down rather than the implementors up is
 * the right direction twice over -- a protocol has no business dictating
 * how a conformer synthesizes storage, and `OZArray`'s atomic getter is
 * the only thing in the tree exercising the `OZ_SPINLOCK` property path
 * (`tests/property_synthesize.rs` says so), which promoting it to
 * `nonatomic` would have quietly deleted.
 */
@property (readonly) uint16_t enumerationIndex;

- (instancetype)objectEnumerator;
- (id)nextObject;

@end
