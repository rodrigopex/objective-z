# Adapted Upstream Tests

Tests adapted from established ObjC test suites to validate the OZ transpiler
against battle-tested behavioral specifications.

## Methodology

Each adapted test:

1. Studies the *behavioral specification* from the upstream test
2. Rewrites as a `.m` + `_test.c` pair using OZTestBase and Unity assertions
3. Removes runtime introspection (`class_getName`, `objc_msgSend`, …)
4. Uses static dispatch via `OZ_SEND` / direct C calls

**Step 3 does not reach `oz_retain_count`, and reading it as though it did cost
real evidence.** Several headers in `bucket_b/` and `mulle_spec/` record
"removed `__objc_refcount_get` introspection, replaced with slab-reuse
verification". `oz_retain_count` is a plain C call and the one refcount entry
point the design rules sanction (`docs/ARC.md` s 1.2) — not introspection — and
slab reuse is a strictly weaker oracle: it cannot tell "the setter retained it"
from "something else still holds it". That substitution is why
`bucket_b/arc_property_retain` claimed to verify a retaining setter while
passing against a non-retaining one, until #596. Prefer the refcount read; if a
claim must rest on slab reuse, pair it with a control assertion proving the pool
size (#455).

## Directories

| Directory | Upstream | License | Cases |
|---|---|---|---|
| `llvm_rewriter/` | LLVM Clang Rewriter tests | Apache 2.0 + LLVM Exception | 10 |
| `bucket_b/` | `tests/objc-reference/` Bucket B patterns (see [BUCKET_B_AUDIT.md](BUCKET_B_AUDIT.md)) | Internal | 9 |
| `gnustep/` | GNUstep libobjc2 tests | MIT | 8 |
| `objfw_spec/` | ObjFW | LGPL-3.0 (spec only) | 5 |
| `mulle_spec/` | mulle-objc | BSD-3-Clause | 3 |
| `apple_spec/` | Apple objc4 behavioural specs (**no APSL code** — see its [README](apple_spec/README.md)) | APSL (spec only) | 2 |

37 cases. **The shape is flat** — `tests/adapted/<source>/<case>.m`, with no
`cases/` level, unlike `tests/behavior/cases/<category>/`. Keep it flat: the two
things that count this corpus disagree about nesting. `conftest.py`'s
`discover_adapted_tests` uses `rglob("*.m")` and takes the category from the
immediate parent, while `corpus_parity.rs`'s `cases_under` reads exactly two
levels. A nested subdirectory would be counted by one and silently skipped by
the other.

`corpus_parity.rs` asserts the count is 37 before compiling anything, because a
wrong glob here does not fail — it narrows, and every case in the smaller set
passes (#400). Update that number in the commit that changes the corpus.

## Running

```sh
python3 -m pytest tests/adapted/ -v
just test-adapted
```

The runner asserts only that the process exits zero, so Unity's status is the
whole signal: a case whose assertions never reach its named construct looks
identical to one that does. That is what #596 found in four of five
`apple_spec` cases. Write the claim into the assertions, and say in the header
what the case does *not* prove.
