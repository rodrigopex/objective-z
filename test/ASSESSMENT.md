# Test Suite Assessment for OZ Transpiler

Classification of existing runtime/Foundation test suites for transpiler reuse.

## Bucket Definitions

- **A — Directly usable:** Exercises transpiler-supported features, assertions check observable behavior
- **B — Adaptable:** Supported features but uses runtime introspection APIs for verification
- **C — Not applicable:** Features the transpiler does not support

## Classification

| Suite | Bucket | Notes |
|-------|--------|-------|
| `runtime/refcount` | A | alloc/init/dealloc, retain/release, nil messaging, dealloc counter |
| `runtime/protocols` | A | Protocol conformance, vtable dispatch, inherited conformance |
| `runtime/literals` | A | @42, @[], @{}, subscript, isEqual:, hash |
| `runtime/message_dispatch` | B | Dispatch is supported; uses objc_msg_lookup, object_getClass |
| `runtime/class_registry` | B | Simple hierarchy; uses objc_lookupClass, class_getName, etc. |
| `runtime/categories` | B | Category merge (transpiler merges at collect); uses class_respondsToSelector |
| `runtime/arc` | B | Scope cleanup, property accessors; uses objc_retain/release directly |
| `runtime/arc_intensive` | B | Comprehensive ARC patterns; uses objc_stats, slab APIs |
| `runtime/static_pools` | B | Pool alloc/reuse; uses K_MEM_SLAB verification |
| `runtime/memory` | B | Object methods, OZString; uses objc_lookupClass, objc_stats |
| `runtime/hash_table` | B | Multi-method dispatch; uses class_respondsToSelector |
| `runtime/flat_dispatch` | B | Hierarchy + category; uses objc_msg_lookup directly |
| `runtime/blocks` | C | Block captures, _Block_copy — not supported by transpiler |
| `runtime/gpio` | C | Zephyr GPIO emulation, ISR blocks — hardware-specific |
| `oz/string` | A | OZString constants: cStr, length, isEqual:, immortal retain |
| `oz/number` | A | OZNumber factory methods, value accessors, isEqual:, hash |
| `oz/array` | A | OZArray @[...] literals, count, objectAtIndex:, description |
| `oz/dictionary` | A | OZDictionary @{...} literals, count, objectForKey: |
| `oz/mutable_string` | A | OZMutableString: append, realloc growth, edge cases |

## Summary

- **Bucket A: 8 suites** — ready for transpiler behavior tests
- **Bucket B: 9 suites** — valuable patterns, need verification API replacement
- **Bucket C: 2 suites** — out of scope (blocks, GPIO hardware)
