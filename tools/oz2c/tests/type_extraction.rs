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

/// C's three tagged type kinds, each named by a method signature, all
/// carrying their keyword through (#595).
///
/// `extract_type_and_stars` had an arm for `enum_specifier` and one for
/// `struct_specifier` -- each prepending the keyword, because the grammar
/// makes the tag a `type_identifier` child and the keyword no node at all
/// -- and **none for `union_specifier`**. The generic fallback then used
/// the tag bare, so a method returning `union nr_u` emitted
///
///     nr_u Shapes_bareUnion(struct Shapes *self);
///
/// in all three places the prototype appears, and clang answered
/// `must use 'union' tag to refer to type 'nr_u'`. Three sites, one
/// missing arm, and oz2c exited 0.
///
/// The three are one arm now: they differ only in the keyword, and keeping
/// them apart is how `union` came to be the one left out. C has no fourth
/// tagged type, so the match is complete rather than merely longer.
///
/// A `typedef`'d union was never affected -- a plain name needs no tag --
/// which is part of why this went unnoticed.
#[test]
fn all_three_tagged_type_kinds_keep_their_keyword() {
    let src = format!(
        "{}
enum nr_e {{ NREOne = 1, NRETwo = 2 }};
struct nr_s {{ int a; }};
union nr_u {{ int i; unsigned char bytes[4]; }};
typedef union nr_u NRUnion;

@interface Shapes : OZObject
- (enum nr_e)bareEnum;
- (struct nr_s)bareStruct;
- (union nr_u)bareUnion;
- (NRUnion)tdUnion;
- (int)takesUnion:(union nr_u)u;
@end
@implementation Shapes
- (enum nr_e)bareEnum {{ return NRETwo; }}
- (struct nr_s)bareStruct {{ struct nr_s s = {{ 3 }}; return s; }}
- (union nr_u)bareUnion {{ union nr_u u; u.i = 4; return u; }}
- (NRUnion)tdUnion {{ union nr_u u; u.i = 5; return u; }}
- (int)takesUnion:(union nr_u)u {{ return u.i; }}
@end

#include <stdio.h>

int main(void) {{
	Shapes *s = [Shapes alloc];
	union nr_u arg;
	arg.i = 6;
	printf(\"%d %d %d %d %d\\n\",
	       (int)[s bareEnum], [s bareStruct].a, [s bareUnion].i,
	       [s tdUnion].i, [s takesUnion:arg]);
	return 0;
}}
",
        PREAMBLE()
    );

    /* The prototype, in the shared header where the bug showed. Asserted
     * as well as run, because the run alone would not say *which* of the
     * three sites had been wrong. */
    let out = oz2c::transpile(&src).expect("all three tagged kinds transpile");
    assert!(
        out.source_c.contains("union nr_u Shapes_bareUnion(struct Shapes *self)"),
        "the union keyword must survive into the prototype:\n{}",
        out.source_c
    );
    assert!(
        out.source_c.contains("enum nr_e Shapes_bareEnum(struct Shapes *self)"),
        "and the enum's, which already worked:\n{}",
        out.source_c
    );
    assert!(
        out.source_c.contains("struct nr_s Shapes_bareStruct(struct Shapes *self)"),
        "and the struct's:\n{}",
        out.source_c
    );

    /* Compiled and run: the defect was a C type error, so the emitted text
     * alone proves nothing -- `nr_u Shapes_bareUnion(...)` reads fine. */
    assert_eq!(compile_and_run(&src, "tagged_type_kinds").trim(), "2 3 4 5 6");
}

/// A `union` *definition*'s tag is skipped when class names are tagged,
/// the same way a `struct`'s already was (#595, and #367's shape).
///
/// The class-tagging walk returns early for a `struct_specifier` so that
/// only the body is descended into -- `struct box` must not become
/// `struct struct box`, while a class-typed field inside it does need
/// tagging. A union fell to the generic walk instead, where its tag is an
/// ordinary `type_identifier`, so a tag matching a class name was tagged
/// as one:
///
///     union Thing { int raw; };   ->   union struct Thing { ... }
///
/// The field half worked for unions all along, because the generic walk
/// reaches the body either way; only the tag needed skipping.
///
/// **No store into the fields here, deliberately.** Storing an
/// ARC-managed reference into a plain C aggregate is refused by a separate
/// rule, and `__unsafe_unretained` on the field did not lift that refusal
/// when tried -- possibly the substring-versus-node reading #488 records,
/// and not this fix's business either way. The property under test is the
/// *emitted spelling* of a declaration, which needs no store to observe:
/// the assertions read the text and the case compiles.
#[test]
fn a_union_definitions_tag_is_not_mistaken_for_a_class() {
    let src = format!(
        "{}
@interface Held : OZObject
- (int)tag;
@end
@implementation Held
- (int)tag {{ return 9; }}
@end

/* A class-typed field inside each: the field needs tagging, the tag does
 * not. */
union box {{
	Held *held;
	int raw;
}};

struct sbox {{
	Held *held;
	int raw;
}};

@interface Probe : OZObject
- (int)run;
@end
@implementation Probe
- (int)run {{ return 1; }}
@end
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("declaring the aggregates transpiles");
    /* Both aggregates are *hoisted*, so the companion header is where the
     * definitions land -- `source_c` keeps only the breadcrumb comment.
     * Asserting on the wrong file is how this test first "failed". */
    let h = &out.companion_h;
    /* The tag untouched... */
    assert!(h.contains("union box {"), "the union tag must not be tagged:\n{}", h);
    assert!(!h.contains("union struct"), "no doubled keyword:\n{}", h);
    /* ...and the class-typed field tagged in both, which is the half that
     * already worked for unions via the generic walk. */
    assert_eq!(
        h.matches("struct Held *held;").count(),
        2,
        "the class-typed field needs tagging in both aggregates:\n{}",
        h
    );

    /* The colliding tag, which is what actually exercised the defect: a
     * union whose tag matches a class name was given the class's `struct`
     * keyword.
     *
     * Asserted on the emitted spelling and **not compiled**, because the
     * program is invalid C either way and for an unrelated reason --
     * struct, union and enum tags share one namespace, so `union Thing`
     * cannot coexist with the `struct Thing` a class lowers to, and clang
     * says `use of 'Thing' with tag type that does not match previous
     * declaration` whatever oz2c emits. Verified against plain C, without
     * oz2c in the picture.
     *
     * That program deserves a located refusal and does not get one -- a
     * gap in the #317 family, where a name the generated C needs collides
     * with the author's. Out of scope here. What is in scope is that oz2c
     * must not invent `union struct Thing`, which is the shape the missing
     * arm produced and the only thing this can check without the refusal
     * existing. */
    let colliding = format!(
        "{}
@interface Thing : OZObject
@end
@implementation Thing
@end

union Thing {{
	int raw;
}};

@interface Probe : OZObject
- (int)run;
@end
@implementation Probe
- (int)run {{ return 1; }}
@end
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&colliding).expect("oz2c accepts it; the C compiler is the one that objects");
    assert!(
        !out.companion_h.contains("union struct"),
        "a union tag must never be given a struct keyword:\n{}",
        out.companion_h
    );
    assert!(
        out.companion_h.contains("union Thing {"),
        "the tag is left exactly as written:\n{}",
        out.companion_h
    );
}
