// SPDX-License-Identifier: Apache-2.0
//
// type_extraction.rs - OZ-097: collect::extract_type_and_stars gaps that
// only surfaced once a real build (not just transpile() returning Ok) was
// tried against the real, unmodified Foundation headers:
//
// - A top-level `struct X;` forward-declaration (no body) never reached
//   anywhere a generated method prototype using it as a pointer type
//   actually needed it visible (the shared companion header) -- see
//   `OZArray.h`'s real `countByEnumeratingWithState:(struct
//   NSFastEnumerationState *)state`.
// - `extract_type_and_stars` had no case for `sized_type_specifier`
//   (`unsigned long`, `long long`, ...) at all, distinct from a
//   single-keyword `primitive_type` -- silently dropping the type
//   entirely (`count:(unsigned long)len` lost `unsigned long`, not just
//   the `struct` keyword).
//
// Both reproduce in the single-file emit() path too (this predates
// OZ-096's file-splitting) -- these tests use tests/common::compile_and_run
// directly, same as any other behavior test.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

#[test]
fn forward_declared_struct_param_compiles_and_runs() {
    let src = format!(
        "{}\n\
struct Opaque;

@interface Foo : OZObject
- (int)useOpaque:(struct Opaque *)p;
@end

@implementation Foo
- (int)useOpaque:(struct Opaque *)p {{
	return p == 0 ? 1 : 0;
}}
@end

#include <stdio.h>
int main(void) {{
	Foo *f = [Foo alloc];
	printf(\"result=%d\\n\", [f useOpaque:0]);
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "forward_declared_struct_param_compiles_and_runs");
    assert_eq!(stdout, "result=1\n");
}

#[test]
fn sized_type_specifier_param_compiles_and_runs() {
    let src = format!(
        "{}\n\
@interface Counter : OZObject
- (unsigned long)addToLength:(unsigned long)base extra:(unsigned long)extra;
@end

@implementation Counter
- (unsigned long)addToLength:(unsigned long)base extra:(unsigned long)extra {{
	return base + extra;
}}
@end

#include <stdio.h>
int main(void) {{
	Counter *c = [Counter alloc];
	printf(\"sum=%lu\\n\", [c addToLength:40 extra:2]);
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "sized_type_specifier_param_compiles_and_runs");
    assert_eq!(stdout, "sum=42\n");
}

/// Mirrors the exact real-world shape that motivated OZ-097:
/// `OZArray.h`'s `countByEnumeratingWithState:(struct
/// NSFastEnumerationState *)state ... count:(unsigned long)len` --
/// a forward-declared struct pointer *and* a sized-type-specifier
/// parameter on the same method.
#[test]
fn forward_declared_struct_and_sized_type_together() {
    let src = format!(
        "{}\n\
struct NSFastEnumerationState;

@interface Foo : OZObject
- (unsigned long)countByEnumeratingWithState:(struct NSFastEnumerationState *)state
				     objects:(void **)stackbuf
				       count:(unsigned long)len;
@end

@implementation Foo
- (unsigned long)countByEnumeratingWithState:(struct NSFastEnumerationState *)state
				     objects:(void **)stackbuf
				       count:(unsigned long)len {{
	return 0;
}}
@end

#include <stdio.h>
int main(void) {{
	Foo *f = [Foo alloc];
	printf(\"result=%lu\\n\", [f countByEnumeratingWithState:0 objects:0 count:5]);
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "forward_declared_struct_and_sized_type_together");
    assert_eq!(stdout, "result=0\n");
}

/// An initialiser's `*` tokens are not the declaration's pointer stars
/// (#491).
///
/// `extract_type_and_stars` walks the whole `declaration` subtree and
/// counts every `*` it meets, and the walk reached the `init_declarator`'s
/// *value* -- an expression, which is never part of the declared type. So
/// any `*` in an initialiser inflated the count:
///
///   - a cast, `Foo *v = (Foo *)[Foo make];` -> `("Foo", 2)`, which is how
///     a strong local lost release-on-overwrite (#491; the ARC half is
///     pinned in `arc_leak_regressions.rs`);
///   - a **multiply**, `__block int acc = 2 * 3;` -> `("int", 1)`, which
///     `emit::hoist_block_var` renders straight into the hoisted static.
///
/// The multiply is the one asserted here, because it is the spelling where
/// the overcount reaches emitted C as a wrong *type* rather than a missing
/// release -- `static int* acc;` returned from an `int`-typed method. A
/// dereference (`int n = *p;`) is the same shape.
///
/// `__block` is the position that makes it visible: `hoist_block_var` is
/// one of only two callers that *render* the count (the other is
/// `emit::file_scope_vars`), and it hoists a declaration the body would
/// otherwise emit in place, so the wrong type ends up at file scope where
/// the dropped initialiser is not there to hide it.
#[test]
fn a_multiply_in_an_initialiser_is_not_a_pointer_star() {
    let src = format!(
        "{}\n\
@interface Foo : OZObject
- (int)run;
@end

@implementation Foo
- (int)run {{
	__block int acc = 2 * 3;
	int scale = 2 * 3;

	acc = 6;
	return acc + scale;
}}
@end

#include <stdio.h>
int main(void) {{
	Foo *f = [Foo alloc];
	printf(\"r=%d\\n\", [f run]);
	return 0;
}}
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("should transpile");
    /* Both halves, because an absence alone would pass on output that
     * hoisted nothing at all. The needle is the whole hoisted
     * declaration: `acc` on its own also appears in the commented-out
     * source line `hoist_block_var` leaves behind, and `int*` on its own
     * appears in the prelude. */
    assert!(
        out.source_c.contains("static int acc;"),
        "a `__block int` must hoist as an int, not a pointer; \
         found no `static int acc;` in:\n{}",
        out.source_c
    );
    assert!(
        !out.source_c.contains("static int* acc;"),
        "the multiply's `*` was counted as a pointer star:\n{}",
        out.source_c
    );
    let stdout = compile_and_run(&src, "a_multiply_in_an_initialiser_is_not_a_pointer_star");
    assert_eq!(stdout, "r=12\n");
}
