# Bucket B Audit — Adaptable Reference Test Patterns

## Summary

9 Bucket B suites audited. 28 extractable patterns identified.

## Audit Results

| Suite | Source | Pattern | Adaptable? | Effort | Adaptation |
|-------|--------|---------|------------|--------|------------|
| arc | src/main.c | Scope-exit releases local | Yes | Trivial | Remove refcount check, use slab reuse |
| arc | src/main.c | Property setter retains new, releases old | Yes | Trivial | Use dealloc tracking |
| arc | src/main.c | Ivar released on dealloc | Yes | Trivial | Slab reuse pattern |
| arc_intensive | src/main.c | Nested retain/release with ivars | Yes | Trivial | Strip stats, use dealloc flag |
| arc_intensive | src/main.c | Scope cleanup in reverse order | Yes | Trivial | Track dealloc sequence |
| arc_intensive | src/main.c | .cxx_destruct ivar cleanup | Yes | Trivial | Verify ivar freed |
| categories | src/main.c | Category method callable | Yes | Trivial | Direct call, check result |
| categories | src/main.c | Category overrides method | Yes | Trivial | Call, verify overridden value |
| categories | src/main.c | Base + category coexist | Yes | Trivial | Call both, verify |
| message_dispatch | src/main.c | Method dispatches correctly | Yes | Moderate | Replace msg_lookup with call |
| message_dispatch | src/main.c | Subclass override resolves | Yes | Moderate | Call on subclass, compare |
| message_dispatch | src/main.c | Super send finds parent | Yes | Moderate | Use [super method] pattern |
| message_dispatch | src/main.c | +initialize runs once | Yes | Moderate | Track call count |
| flat_dispatch | src/main.c | Hierarchy dispatch chain | Yes | Moderate | Call at each level |
| flat_dispatch | src/main.c | Category override in hierarchy | Yes | Moderate | Call, verify override |
| flat_dispatch | src/main.c | No cross-class contamination | Yes | Trivial | Verify separate results |
| static_pools | src/main.c | Pool alloc succeeds | Yes | Trivial | No introspection present |
| static_pools | src/main.c | Pool slot reuse after free | Yes | Trivial | Alloc/free/realloc |
| static_pools | src/main.c | Pool capacity enforced | Yes | Trivial | Exhaust, verify ENOMEM |
| memory | src/main.c | Object alloc/dealloc cycles | Yes | Trivial | Direct alloc/release |
| memory | src/main.c | Ivar zeroing on alloc | Yes | Trivial | Check ivars after alloc |
| memory | src/main.c | Object equality (isEqual:) | Yes | Trivial | Direct call |
| memory | src/main.c | String properties (cStr, length) | Yes | Trivial | Direct call |
| hash_table | src/main.c | All instance methods dispatch | Yes | Trivial | Call all 7, verify |
| hash_table | src/main.c | Class methods dispatch | Yes | Trivial | Call, verify |
| hash_table | src/main.c | Subclass inherits all methods | Yes | Trivial | Call inherited, verify |
| class_registry | src/main.c | Property struct copy | Yes | Complex | Direct struct operations |
| class_registry | src/main.c | isKindOfClass hierarchy | No | — | Requires runtime introspection |

## Selected for Adaptation (Phase 6)

- **ARC** (4 tests): scope cleanup, property retain, ivar dealloc, nested intensive
- **Dispatch** (3 tests): hierarchy, category merge, flat chain
- **Pool/Slab** (2 tests): slab reuse, exhaustion recovery
