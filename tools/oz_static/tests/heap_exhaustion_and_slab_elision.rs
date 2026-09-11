// SPDX-License-Identifier: Apache-2.0
//
// heap_exhaustion_and_slab_elision.rs - the two mechanical ways the heap
// path was second-class while `+dynamicAlloc` made it first-class in the
// API (#419).
//
//   1. **Heap exhaustion was silent.** The slab allocator has carried
//      `OZ_STATIC_TRAP_POOL_EXHAUSTION` since the pools work; the heap
//      allocator had a bare `return (struct {name} *)0;` and nothing else.
//      The comment on the slab trap says why that matters -- "that nil
//      then travels", surfacing as `EXC_BAD_ACCESS` inside a function with
//      nothing to do with the cause -- and a heap is far easier to exhaust
//      than a statically sized slab.
//
//      The trap is **opt-in, under the same macro**, not on by default.
//      `companion::render_heap_exhaustion_trap` carries the argument; the
//      short version is that nil-on-failure is one contract across both
//      allocators, a build that keeps it has to keep it on both, and
//      `[Cls dynamicAlloc]` behaving differently from `[Cls alloc]` on the
//      identical failure is the defect #418 was about.
//
//   2. **A heap-only class still reserved a slab.** `PoolSizes::for_class`
//      floored at one because a zero-block `K_MEM_SLAB_DEFINE` is not a
//      slab, so "how many slots" could not express "none". The presence
//      question is now asked separately (`ever_slab_allocated`), and a
//      class with no `[Class alloc]` site anywhere gets no slab at all.
//
// What is *not* tested here is a trap firing: an assert aborts, so there is
// no output to assert on. The text tests say the trap is emitted, the
// compile with the macro defined says the text is real C, and the
// compile-and-run without it says the nil contract is unchanged. The
// firing itself is only observable on a target, which is where #419's BSS
// measurement was taken.

mod common;
use common::{
    compile_and_run_with_heap, compile_and_run_with_heap_and_cc_flags, ozheap_src,
    ozobject_src as PREAMBLE,
};

/// A class allocated only from a heap: `+dynamicAlloc` and
/// `+dynamicAllocWithHeap:`, and no `[Sensor alloc]` anywhere.
fn heap_only_program(tail: &str) -> String {
    format!(
        "{}{}\n\
@interface Sensor : OZObject {{\n\
\tint _v;\n\
}}\n\
- (void)setValue:(int)v;\n\
- (int)value;\n\
@end\n\
@implementation Sensor\n\
- (void)setValue:(int)v {{ _v = v; }}\n\
- (int)value {{ return _v; }}\n\
- (void)dealloc {{}}\n\
@end\n\
{}",
        PREAMBLE(),
        ozheap_src(),
        tail
    )
}

fn heap_options() -> oz_static::Options {
    oz_static::Options { heap_support: true, ..Default::default() }
}

fn generated(src: &str) -> String {
    let out = oz_static::transpile_with_options(src, &heap_options())
        .unwrap_or_else(|d| panic!("should transpile: {:?}", d));
    format!("{}\n{}", out.source_c, out.companion_c)
}

/// The body of one generated C function's *definition*, by name -- skipping
/// lines that end in `;`, which are the prototypes the companion header
/// declares for every class. See the same correction in
/// `explicit_ivar_store.rs`.
fn function_body(source_c: &str, name: &str) -> String {
    let needle = format!("{}(", name);
    let mut from = 0;
    while let Some(rel) = source_c[from..].find(&needle) {
        let at = from + rel;
        let line_end = source_c[at..].find('\n').map(|e| at + e).unwrap_or(source_c.len());
        if !source_c[at..line_end].trim_end().ends_with(';') {
            let tail = &source_c[at..];
            let end = tail.find("\n}").map(|e| e + 2).unwrap_or(tail.len());
            return tail[..end].to_string();
        }
        from = at + needle.len();
    }
    panic!("no definition of `{}` in:\n{}", name, source_c);
}

const MAIN: &str = "\
#include <stdio.h>

static char g_buf[512];

int main(void)
{
	OZHeap *h = [OZHeap alloc];
	[h initWithBuffer:g_buf size:512];
	Sensor *a = [[Sensor dynamicAllocWithHeap:h] init];
	[a setValue:7];
	printf(\"named=%d\\n\", [a value]);
	Sensor *b = [[Sensor dynamicAlloc] init];
	[b setValue:9];
	printf(\"system=%d\\n\", [b value]);
	return 0;
}
";

/// Both arms of the heap trap, inside the heap allocator, guarded by the
/// macro -- and the `return nil` still after it, because the contract when
/// the macro is off is unchanged.
///
/// Two arms because "which heap" is half the diagnosis: a nil `heap_obj`
/// is the system heap, reached through `+dynamicAlloc`, and a non-nil one
/// is the `OZHeap` the caller handed over. A single message could name
/// neither.
#[test]
fn heap_allocator_carries_a_named_trap_under_the_macro() {
    let all = generated(&heap_only_program(MAIN));
    let body = function_body(&all, "Sensor_oz_dynamic_alloc_with_heap");
    assert!(
        body.contains("#ifdef OZ_STATIC_TRAP_POOL_EXHAUSTION"),
        "the heap allocator must carry the exhaustion trap; got:\n{}",
        body
    );
    assert!(
        body.contains("the OZHeap passed to '[Sensor dynamicAllocWithHeap:]' is exhausted"),
        "the named-heap arm must name the class and the heap; got:\n{}",
        body
    );
    assert!(
        body.contains("the system heap is exhausted"),
        "the system-heap arm must say which heap ran out; got:\n{}",
        body
    );
    let trap = body.find("#ifdef OZ_STATIC_TRAP_POOL_EXHAUSTION").unwrap();
    let ret = body.find("return (struct Sensor *)0;").unwrap();
    assert!(
        trap < ret,
        "the trap must precede the nil return, which stays for builds \
         without the macro; got:\n{}",
        body
    );
}

/// The trap is opt-in, deliberately: with the macro off the emitted C must
/// contain no assert on the heap path at all, so a caller that checks for
/// nil keeps working and the failure path stays testable.
#[test]
fn the_heap_trap_is_opt_in() {
    let all = generated(&heap_only_program(MAIN));
    let body = function_body(&all, "Sensor_oz_dynamic_alloc_with_heap");
    /* Not vacuous: without a trap at all there is nothing to be opt-in
     * about, and this would pass on the pre-#419 emission. */
    assert!(
        body.contains("#ifdef OZ_STATIC_TRAP_POOL_EXHAUSTION"),
        "there has to be a trap for it to be opt-in; got:\n{}",
        body
    );
    let guarded = body.split("#ifdef OZ_STATIC_TRAP_POOL_EXHAUSTION").next().unwrap();
    assert!(
        !guarded.contains("oz_assert"),
        "nothing may assert ahead of the guard, or the trap is on by \
         default; got:\n{}",
        body
    );
    let after_endif = body.rsplit("#endif").next().unwrap();
    assert!(
        !after_endif.contains("oz_assert"),
        "and nothing after it either; got:\n{}",
        body
    );
}

/// The emitted trap is real C. A trap that fires aborts, so this compiles
/// the program with the macro defined and asserts on the output of the
/// paths that do *not* exhaust -- which is the whole claim: the guarded
/// text compiles, links, and changes nothing about a successful
/// allocation.
#[test]
fn the_heap_trap_compiles_with_the_macro_defined() {
    let out = compile_and_run_with_heap_and_cc_flags(
        &heap_only_program(MAIN),
        "heap_trap_macro_defined",
        &["-DOZ_STATIC_TRAP_POOL_EXHAUSTION"],
    );
    assert_eq!(out, "named=7\nsystem=9\n");
}

/// Without the macro, byte-for-byte the same behaviour as before: the heap
/// paths work and nothing traps.
#[test]
fn heap_allocation_still_works_without_the_macro() {
    let out = compile_and_run_with_heap(&heap_only_program(MAIN), "heap_no_trap");
    assert_eq!(out, "named=7\nsystem=9\n");
}

/// The elision, which is the byte-counting half of #419: a class this
/// program never slab-allocates reserves no `k_mem_slab` and no static
/// storage for one.
///
/// `Sensor` here is reached only through `+dynamicAlloc` and
/// `+dynamicAllocWithHeap:`. Neither is a slab site -- `pools::
/// alloc_receiver_class` compares the selector whole-string against
/// `"alloc"`, which is correct and deliberate (#413) -- so the presence
/// question answers no and the slab goes.
#[test]
fn a_heap_only_class_reserves_no_slab() {
    let all = generated(&heap_only_program(MAIN));
    assert!(
        !all.contains("OZ_SLAB_DEFINE(oz_slab_Sensor"),
        "a class allocated only from a heap must reserve no slab; got:\n{}",
        all
    );
    assert!(
        all.contains("no slab for Sensor"),
        "and must say so where the slab used to be; got:\n{}",
        all
    );
}

/// `OZHeap` itself is the control: the same program *does* send
/// `[OZHeap alloc]`, so its slab stays. Without this the elision test
/// above would pass just as well if the slab emission had been deleted
/// outright.
#[test]
fn a_slab_allocated_class_in_the_same_program_keeps_its_slab() {
    let all = generated(&heap_only_program(MAIN));
    assert!(
        all.contains("OZ_SLAB_DEFINE(oz_slab_OZHeap"),
        "[OZHeap alloc] is a slab site, so OZHeap keeps its slab; got:\n{}",
        all
    );
}

/// A slab-less class's `{name}_oz_free` must not name a slab that does not
/// exist, and must still be defined -- `oz_static_release`'s class_id
/// switch calls it for every class in the program.
#[test]
fn a_slab_less_class_frees_through_the_heap_only() {
    let all = generated(&heap_only_program(MAIN));
    let body = function_body(&all, "Sensor_oz_free");
    assert!(
        !body.contains("oz_slab_free"),
        "there is no slab to return a slot to; got:\n{}",
        body
    );
    assert!(
        body.contains("oz_heap_obj_free"),
        "the heap branch is the only way an instance can have been \
         allocated; got:\n{}",
        body
    );
}
