// SPDX-License-Identifier: Apache-2.0
//
// property_synthesize.rs - OZ-095: `@property`/`@synthesize` support,
// ported 1:1 from the Python pipeline's `resolve.py::_synthesize_properties`
// / `emit.py::_emit_synthesized_accessor` (see collect::resolve_properties,
// emit::render_synthesized_accessor). Covers what the two real Foundation
// headers actually use (readonly scalar, atomic-by-default, explicit
// `@synthesize name = _ivar`) via the real fixtures in common::mod, plus
// the rest of full parity: nonatomic, strong-object retain/release,
// custom getter=/setter= names, implicit synthesis (no @synthesize at
// all), and bare `@synthesize name;` against an already-bare ivar.
// `weak` rejection lives in static_bar_rejects.rs, alongside this
// codebase's other hard-rejected constructs.

mod common;
use common::{compile_and_run, iterator_protocol_src, ozarray_src, ozobject_src as PREAMBLE, ozq31_src};

#[test]
fn real_ozarray_iter_idx_getter_reads_after_iteration() {
    // OZArray.h's own `@property (readonly) uint16_t iterIdx;` +
    // OZArray.m's `@synthesize iterIdx = _iterIdx;` -- explicit ivar,
    // readonly, no `nonatomic` (so atomic: exercises the OZ_SPINLOCK
    // path, a no-op `if` on host but still must compile and run).
    let src = format!(
        "{}{}{}{}\n\
#include <stdio.h>
int main(void) {{
	OZArray *arr = @[@(10), @(20), @(30)];
	[arr iter];
	[arr next];
	[arr next];
	printf(\"iterIdx=%u\\n\", [arr iterIdx]);
	return 0;
}}
",
        PREAMBLE(),
        iterator_protocol_src(),
        ozq31_src(),
        ozarray_src()
    );
    let stdout = compile_and_run(&src, "real_ozarray_iter_idx_getter_reads_after_iteration");
    assert_eq!(stdout, "iterIdx=2\n");
}

#[test]
fn nonatomic_property_get_and_set() {
    let src = format!(
        "{}\n\
@interface Counter : OZObject
@property (nonatomic) int count;
@end

@implementation Counter
@synthesize count = _count;
@end

#include <stdio.h>
int main(void) {{
	Counter *c = [Counter alloc];
	printf(\"before=%d\\n\", [c count]);
	[c setCount:42];
	printf(\"after=%d\\n\", [c count]);
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "nonatomic_property_get_and_set");
    assert_eq!(stdout, "before=0\nafter=42\n");
}

#[test]
fn strong_object_setter_retains_new_releases_old() {
    // Setter must retain the incoming value and release whatever the
    // ivar held before -- mirrors `emit.py::_emit_synthesized_accessor`'s
    // strong-object branch exactly (just via this codebase's own
    // `oz_static_retain`/`oz_static_release`, not Python's `{root}_retain`).
    let src = format!(
        "{}\n\
@interface Holder : OZObject
@property (nonatomic, strong) OZObject *thing;
@end

@implementation Holder
@synthesize thing = _thing;
@end

#include <stdio.h>
int main(void) {{
	OZObject *a = [OZObject alloc];
	OZObject *b = [OZObject alloc];
	Holder *h = [Holder alloc];
	[h setThing:a];
	printf(\"a=%d b=%d\\n\", [a retainCount], [b retainCount]);
	[h setThing:b];
	printf(\"a=%d b=%d\\n\", [a retainCount], [b retainCount]);
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "strong_object_setter_retains_new_releases_old");
    assert_eq!(stdout, "a=2 b=1\na=1 b=2\n");
}

#[test]
fn custom_getter_and_setter_names() {
    let src = format!(
        "{}\n\
@interface Flag : OZObject
@property (nonatomic, getter=isReady, setter=setReady:) int ready;
@end

@implementation Flag
@synthesize ready = _ready;
@end

#include <stdio.h>
int main(void) {{
	Flag *f = [Flag alloc];
	printf(\"before=%d\\n\", [f isReady]);
	[f setReady:1];
	printf(\"after=%d\\n\", [f isReady]);
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "custom_getter_and_setter_names");
    assert_eq!(stdout, "before=0\nafter=1\n");
}

#[test]
fn implicit_synthesis_defaults_ivar_to_underscore_name() {
    // No `@synthesize` anywhere -- the getter/setter must still be
    // synthesized, backed by an auto-created `_count` ivar (no `_count`
    // field exists anywhere in source).
    let src = format!(
        "{}\n\
@interface Counter : OZObject
@property (nonatomic) int count;
@end

@implementation Counter
@end

#include <stdio.h>
int main(void) {{
	Counter *c = [Counter alloc];
	[c setCount:7];
	printf(\"count=%d\\n\", [c count]);
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "implicit_synthesis_defaults_ivar_to_underscore_name");
    assert_eq!(stdout, "count=7\n");
}

#[test]
fn bare_synthesize_uses_existing_bare_ivar_name() {
    // `@synthesize count;` (no explicit `= ivar`) with an ivar already
    // declared under the bare name `count` (not `_count`) -- Python's
    // oracle accepts this by default, only warning under --strict; see
    // `collect::resolve_properties`'s comment for why oz_static (which
    // has no non-fatal diagnostic channel) matches that default
    // (non-strict) behavior instead of hard-rejecting it.
    let src = format!(
        "{}\n\
@interface Counter : OZObject {{
	int count;
}}
@property (nonatomic) int count;
@end

@implementation Counter
@synthesize count;
@end

#include <stdio.h>
int main(void) {{
	Counter *c = [Counter alloc];
	[c setCount:9];
	printf(\"count=%d\\n\", [c count]);
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "bare_synthesize_uses_existing_bare_ivar_name");
    assert_eq!(stdout, "count=9\n");
}

#[test]
fn unmatched_synthesize_is_rejected() {
    // `@synthesize` for a name with no matching `@property` -- almost
    // certainly a typo, and unlike Python (which silently no-ops), this
    // codebase's own stated design ("anything the static subset doesn't
    // accept is a named, located hard error", see `lib::transpile`)
    // makes it a real diagnostic instead.
    let src = format!(
        "{}\n\
@interface Foo : OZObject
@end

@implementation Foo
@synthesize bogus = _bogus;
@end
",
        PREAMBLE()
    );
    let diags = common::expect_reject(&src);
    assert!(diags.contains("'@synthesize bogus'"), "diagnostics: {}", diags);
    assert!(diags.contains("no '@property bogus'"), "diagnostics: {}", diags);
}
