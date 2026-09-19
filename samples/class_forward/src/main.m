/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * `@class` forward declarations, on target (#564).
 *
 * This sample exists because the gate that would catch a regression here
 * cannot see the code otherwise. `just test-pedantic` sweeps the C
 * generated from `samples/`, and before this sample **no sample used
 * `@class` at all** -- so the tag declarations #564 emits were invisible to
 * it, and the sweep was green over a construct it had never compiled. The
 * same is true of `just test-boards`.
 *
 * What it pins down:
 *
 *   - A `@class` line is *consumed* and replaced by `struct PXPeer;`. Left
 *     alone it was copied into the generated `.c` verbatim, where GCC
 *     answered `stray '@' in program` about a file the author never wrote.
 *   - An **ivar** typed by a forward-declared class gets the `struct` tag.
 *     That is a different lowering path from a local (`emit`'s bare-ivar
 *     edit, not `render_expr`'s `type_identifier` arm), and it is the one
 *     nothing else in the tree exercises.
 *   - A class that is forward-declared and **never defined** is legal, which
 *     is the whole point of the construct: C reaches an undefined type
 *     through a tag, and `struct PXPeer;` is valid with nothing ever
 *     defining `struct PXPeer`.
 *
 * `PXPeer` is deliberately never implemented. A pointer to it can be
 * declared, stored and compared against nil, and nothing here may send it a
 * message -- a send through a forward-declared-only name is refused, with
 * the forward declaration named as the cause (#557).
 */

#import <Foundation/OZObject.h>
#include <zephyr/kernel.h>

/* Never defined in this translation unit, on purpose. */
@class PXPeer;

@interface PXHolder : OZObject {
	/* An ivar whose type is only forward-declared. Before #564 this
	 * reached the generated struct as a bare `PXPeer *_peer;`, with no
	 * tag and no type of that name in scope. */
	PXPeer *_peer;
	int _slot;
}
- (instancetype)initWithSlot:(int)slot;
- (int)slot;
- (int)peerIsUnset;
@end

@implementation PXHolder

- (instancetype)initWithSlot:(int)slot
{
	self = [super init];
	if (self != nil) {
		_slot = slot;
		_peer = nil;
	}
	return self;
}

- (int)slot
{
	return _slot;
}

- (int)peerIsUnset
{
	/* A local of the same forward-declared type, which is the other
	 * lowering path. */
	PXPeer *probe = _peer;

	return probe == nil;
}

@end

int main(void)
{
	printk("=== Class Forward Demo ===\n");

	PXHolder *holder = [[PXHolder alloc] initWithSlot:564];

	if (holder == nil) {
		printk("holder alloc FAILED\n");
		return 0;
	}
	printk("slot=%d\n", [holder slot]);
	printk("peerUnset=%d\n", [holder peerIsUnset]);
	printk("=== Demo complete ===\n");
	return 0;
}
