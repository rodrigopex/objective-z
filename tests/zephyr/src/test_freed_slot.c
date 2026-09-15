/* SPDX-License-Identifier: Apache-2.0 */
/*
 * What survives a `k_mem_slab_free`, measured on target (#490).
 *
 * This is the half of the freed-slot instruments that no host harness can
 * establish. `refcount_traps.rs` proves the trap fires and where it sits in
 * `oz_release`, by staging the sentinel on a live object -- fully defined,
 * and identical on every platform. What it cannot prove is that the
 * sentinel is still there to be read after the allocator has taken the
 * block back, because that is a fact about the allocator. On arm64 macOS it
 * is not: a 12-byte block read back `05 00 00 00 00 00 00 00 05 00 00 00`,
 * malloc's bookkeeping having taken the body as well as the header. So the
 * survival claim has to be measured where the allocator is `k_mem_slab`.
 *
 * Neither file is sufficient alone. This one would pass if the trap read
 * the marker and did nothing with it; that one would pass on a target where
 * the marker never survives at all.
 *
 * **Why the marker is the refcount and not the class id.** `_meta` is the
 * root struct's first member, so `class_id` sits at offset 0 --
 * and `k_mem_slab_free` links a freed block into its free list by writing
 * the link *into the block*, at offset 0:
 *
 *     *(char **) mem = slab->free_list;      kernel/mem_slab.c
 *
 * So the class stamp is destroyed by the free it exists to outlive. Every
 * bit of `_meta` is: `heap_allocated` is bit 10 of the link,
 * `deallocating` bit 11, `immortal` bit 12. `oz_refcount` begins at exactly
 * `sizeof(char *)`, the first word the link cannot reach, and structurally
 * rather than by luck -- `oz_atomic_t` is Zephyr's `atomic_t`, a `long`,
 * and `sizeof(long) == sizeof(char *)` on both ILP32 and LP64, so its
 * alignment rounds the offset up to precisely `sizeof(char *)`.
 *
 * Verified on both widths while this was written: mps2/an385 (offset 4) and
 * qemu_cortex_a53 (offset 8), the stamped word intact on both, the class id
 * clobbered on both (588 and 376 -- the low bits of the two links).
 *
 * The reads below are deliberately of a slot the slab owns. That is defined
 * here in a way it is not on a host: slab storage is a static array that
 * outlives every allocation, so the bytes exist and are stable; only the
 * right to use them has gone.
 */
#include <zephyr/ztest.h>
#include <stddef.h>
#include "Node_ozh.h"
#include "OZObject_ozh.h"
#include "oz_dispatch.h"

ZTEST_SUITE(freed_slot, NULL, NULL, NULL, NULL, NULL);

/* The layout that makes the marker reachable after the free. Asserted
 * separately from the survival below, because a reorder of the root struct
 * would break the survival with no hint as to why. */
ZTEST(freed_slot, test_the_refcount_starts_where_the_free_list_link_ends)
{
	zassert_equal(0u, offsetof(struct OZObject, _meta),
		      "_meta must be the root struct's first member -- the dispatch "
		      "header reads class_id through `struct oz_metadata *` on that "
		      "basis");
	zassert_equal(4u, sizeof(struct oz_metadata),
		      "the metadata is one 32-bit word of bitfields; a wider one would "
		      "put a live field under the free-list link on a 32-bit target");
	zassert_equal(sizeof(char *), offsetof(struct OZObject, oz_refcount),
		      "oz_refcount must begin exactly where k_mem_slab_free's link "
		      "ends. Below that and the marker is clobbered; above it and "
		      "there is padding no one asked for");
}

/* The measurement: allocate, release to zero so `Node_oz_free` runs, then
 * read the slot back.
 *
 * Presence and absence on the same slot, in that order. The absence half --
 * that `class_id` did *not* survive -- is only a finding next to a marker
 * that did; on its own it would also be what a `_oz_free` that stamped
 * nothing at all looks like.
 */
ZTEST(freed_slot, test_the_sentinel_survives_the_free_and_the_class_id_does_not)
{
	struct Node *n = Node_alloc();
	struct OZObject *o;

	zassert_not_null(n, "the Node slab must have a slot free");
	o = (struct OZObject *)n;
	zassert_equal(OZ_CLASS_Node, o->_meta.class_id,
		      "the live object must know its class, or nothing below is about "
		      "a poisoned slot");

	/* Refcount is 1 from the alloc, so this one release deallocates and
	 * calls Node_oz_free, which poisons and returns the slot. */
	OZObject_release(o);

	zassert_equal(OZ_REFCOUNT_FREED, oz_atomic_get(&o->oz_refcount),
		      "the freed sentinel must outlive k_mem_slab_free -- it is the "
		      "only marker on this backend that does, and oz_retain/oz_release "
		      "read it to name a use-after-free");
	zassert_not_equal(OZ_CLASS_ID_FREED, o->_meta.class_id,
		      "the class stamp is expected to be GONE: offset 0 is where the "
		      "free-list link goes. If this ever passes, k_mem_slab changed "
		      "and the sentinel may no longer be needed");
}

/* Reallocating the slot clears the sentinel, so the trap cannot fire on a
 * legitimately reused block.
 *
 * `_oz_alloc` memsets the whole object and then writes refcount 1, so this
 * holds by construction -- which is exactly why it is worth a row: the
 * sentinel's safety depends on an initialisation in a different function,
 * and nothing else says so.
 */
ZTEST(freed_slot, test_reallocating_the_slot_clears_the_sentinel)
{
	struct Node *first = Node_alloc();
	struct Node *again;
	struct OZObject *o;

	zassert_not_null(first, "the Node slab must have a slot free");
	OZObject_release((struct OZObject *)first);

	again = Node_alloc();
	zassert_not_null(again, "the freed slot must be available again");
	o = (struct OZObject *)again;
	zassert_not_equal(OZ_REFCOUNT_FREED, oz_atomic_get(&o->oz_refcount),
			  "a reused slot must not still read as freed, or every object "
			  "after the first free traps");
	zassert_equal(1, oz_atomic_get(&o->oz_refcount),
		      "a fresh object starts at one reference");

	OZObject_release(o);
}
