// SPDX-License-Identifier: Apache-2.0
//
// ivar_store_ordering.rs -- a strong ivar's previous value is released
// *before* the new one is allocated, where the store cannot read it.
//
// `render_strong_local_assign` already picks between two shapes through
// `classify_store`: a `+1` right-hand side that does not mention the
// variable releases first, and a plain identifier retains new / releases
// old / assigns so that `c = c` stays safe. Releasing first is what lets
// one slab slot serve a whole loop.
//
// `render_strong_ivar_assign` never consulted that classification. It
// hoisted the previous value into a temporary and emitted
// `(self->_x = value, [retain,] release(prev))` -- new value always
// evaluated first -- so a strong ivar needed two slots where a local
// needed one, for a store that provably cannot read the ivar.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// A class whose `-init` allocates into a strong ivar, initialised twice.
///
/// One slot is enough: the second `-init`'s store releases the previous
/// `Thing` *before* allocating the next, so the slot goes back to the slab
/// and the new allocation takes it again. Evaluating the new value first
/// needs two slots, and on a one-slot pool the second `-init` gets nil.
#[test]
fn reinitialised_ivar_needs_only_one_slab_slot() {
	let src = format!(
		"/* oz-pool: Thing=1, Holder=1 */\n{}{}",
		PREAMBLE(),
		"\
@interface Thing : OZObject {
	int _n;
}
@end
@implementation Thing
@end

@interface Holder : OZObject {
	Thing *_thing;
}
- (instancetype)init;
- (int)ok;
@end
@implementation Holder
- (instancetype)init {
	self = [super init];
	_thing = [Thing alloc];
	return self;
}
- (int)ok {
	return _thing != 0;
}
@end

#include <stdio.h>
int main(void) {
	Holder *h = [Holder alloc];
	h = [h init];
	printf(\"first=%d\\n\", [h ok]);
	h = [h init];
	printf(\"second=%d\\n\", [h ok]);
	return 0;
}
"
	);
	let stdout = compile_and_run(&src, "reinitialised_ivar_needs_only_one_slab_slot");
	assert_eq!(
		stdout, "first=1\nsecond=1\n",
		"the second -init must reuse the slot the first one's Thing gave back"
	);
}
