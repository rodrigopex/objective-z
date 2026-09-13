// SPDX-License-Identifier: Apache-2.0
//
// naming_refcount_entry_point.rs - `oz_static_retain_count` is the one
// refcount entry point, and the one Objective-C source may call (#418).
//
// It replaced `__objc_refcount_get`, which was four problems in one name:
// public under `__objc_`, the *internal* prefix; a reserved identifier,
// since a leading double underscore belongs to the implementation in C; a
// second public name for a function the companion already generated; and
// the SDK's last `get`-prefixed getter, where #413 settled that `get`
// marks a method writing through a caller's pointer.
//
// Two claims are pinned here, because retiring a name is only half of it:
//
//   1. **The spelling is gone from generated C.** A name change that leaves
//      the old symbol emitted retires nothing -- and this one is reached by
//      user code (`samples/mem_demo`), so a stale emission would keep
//      compiling and hide the break.
//   2. **The parameter type is `id`, in both the declaration and the
//      definition.** This is the load-bearing half.
//      `include/oz_sdk/Foundation/OZObject.h` has to declare this function
//      too -- Clang resolves calls to it while dumping the AST, before any
//      generated header exists -- and that header cannot name the root
//      struct, which is generated. Since the SDK header is *spliced into*
//      the generated C, the two declarations land in one translation unit:
//      identical, they are redundant and legal; differing in the parameter
//      type, they are a conflicting declaration and nothing compiles. That
//      is precisely why the retired name existed as a separate `id`-taking
//      forwarder, and why collapsing the two names meant changing this
//      signature rather than deleting a line.
//
// `ozobject_src()` splices the real `OZObject.h`, so the compile in
// `retain_count_is_callable_from_source` is the check that the redundancy
// really is redundant.

mod common;
use common::{compile_and_run_strict, ozobject_src};

/// Every generated artifact, so a survivor cannot hide in the half this
/// test does not look at.
fn generated(src: &str) -> String {
    let out = oz2c::transpile(src).expect("should transpile");
    format!("{}\n{}\n{}", out.companion_h, out.companion_c, out.source_c)
}

/// The retired spelling must appear nowhere in generated C -- and neither
/// must any other `__objc_` name, which is the standing rule #418 leaves
/// behind rather than a restatement of the same check.
#[test]
fn no_generated_name_carries_the_reserved_objc_prefix() {
    let src = format!("{}\n{}", ozobject_src(), "int main(void) { return 0; }\n");
    let all = generated(&src);
    assert!(
        !all.contains("__objc_refcount_get"),
        "__objc_refcount_get was retired in #418; generated C still emits it:\n{}",
        all
    );
    assert!(
        !all.contains("__objc_"),
        "`__objc_` is a reserved identifier and retired as a prefix (#418); \
         generated C still emits one:\n{}",
        all
    );
}

/// The declaration and the definition both take `id`, matching what
/// `include/oz_sdk/Foundation/OZObject.h` declares. A `struct <root> *`
/// here is the conflicting-declaration failure described in the header
/// comment, and it is not visible in a single-file compile that happens
/// not to splice the SDK header.
#[test]
fn retain_count_takes_id_in_both_halves() {
    let src = format!("{}\n{}", ozobject_src(), "int main(void) { return 0; }\n");
    let out = oz2c::transpile(&src).expect("should transpile");
    assert!(
        out.companion_h.contains("int oz_static_retain_count(id obj);"),
        "the companion header must declare the `id` form, to agree with the \
         SDK header spliced beside it; got:\n{}",
        out.companion_h
    );
    assert!(
        out.companion_c.contains("int oz_static_retain_count(id obj)\n{"),
        "the definition must take `id` too; got:\n{}",
        out.companion_c
    );
}

/// The whole point of the name: Objective-C source can call it. ARC
/// forbids `[obj retainCount]`, so a plain C call is the only spelling
/// there is, and `samples/mem_demo` uses exactly this one.
///
/// `compile_and_run_strict`, not `compile_and_run`, and that is the whole
/// test: plain `cc` only *warns* on passing a `struct Sensor *` where a
/// `struct OZObject *` is declared, so with the pre-#418 signature this
/// compiled, ran, and printed the right number. `-Werror=
/// incompatible-pointer-types` is what turns the wrong signature into a
/// failure -- the same reason that flag exists for #365's shape.
#[test]
fn retain_count_is_callable_from_source() {
    let src = format!(
        "{}\n{}",
        ozobject_src(),
        "@interface Sensor : OZObject { int _v; }\n\
         - (void)setValue:(int)v;\n\
         @end\n\
         @implementation Sensor\n\
         - (void)setValue:(int)v { _v = v; }\n\
         - (void)dealloc {}\n\
         @end\n\
         int main(void)\n\
         {\n\
         \tSensor *s = [[Sensor alloc] init];\n\
         \t[s setValue:42];\n\
         \tprintf(\"rc=%d\\n\", oz_static_retain_count(s));\n\
         \treturn 0;\n\
         }\n"
    );
    let out = compile_and_run_strict(&src, "retain_count_from_source");
    assert!(
        out.contains("rc=1"),
        "a freshly allocated object has one reference; got:\n{}",
        out
    );
}

/// The heap bridge carries `oz_static_`, and the retired `oz_heap_obj_*`
/// spelling appears nowhere in generated C (#417).
///
/// Here rather than in `heap_exhaustion_and_slab_elision.rs` because this is
/// the file that already pins "a name is retired only when the old spelling
/// is gone from the output" -- the rule #418 left behind, applied to a second
/// rename. That file asserts the *new* name is present, which a half-done
/// rename also satisfies; this asserts the old one is absent, which is the
/// half that catches it.
///
/// The two names are `oz_static_heap_alloc`/`oz_static_heap_free`: declared
/// by the PAL, defined by the companion, so they belong to the generated
/// namespace exactly as `oz_static_retain_count` does. Under `oz_heap_` they
/// were anagrams of the PAL functions they call -- `oz_heap_obj_alloc`
/// calling `oz_heap_alloc_obj`.
///
/// A stale spelling on only one side of that split is also a **link**
/// failure, since the PAL's declaration and the companion's definition would
/// name different symbols -- `samples/heap_alloc` would not link. This test
/// is the cheaper signal, not the only one.
#[test]
fn the_heap_bridge_is_in_the_generated_namespace() {
    /* **`heap_support: true` is the whole test.** The bridge is emitted only
     * under `--heap-support`, so the first version of this asserted the
     * retired spelling was absent from output that never contained the
     * bridge at all -- and passed with the old name deliberately restored.
     * Checked by doing exactly that, which is the only way to tell this
     * shape of vacuous pass from a real one (#413's `ALWAYS_DYNAMIC` was
     * the same trap). */
    let src = format!(
        "{}\n{}",
        ozobject_src(),
        "@interface Sensor : OZObject { int _v; }\n@end\n\
         @implementation Sensor\n@end\n"
    );
    let opts = oz2c::Options { heap_support: true, ..Default::default() };
    let built = oz2c::transpile_with_options(&src, &opts)
        .unwrap_or_else(|d| panic!("should transpile: {:?}", d));
    let out = format!("{}\n{}", built.source_c, built.companion_c);
    assert!(
        out.contains("oz_static_heap_alloc") && out.contains("oz_static_heap_free"),
        "the bridge has to be in this output for the absence checks below to mean \
         anything; got:\n{}",
        out
    );
    for retired in ["oz_heap_obj_alloc", "oz_heap_obj_free"] {
        assert!(
            !out.contains(retired),
            "'{}' is retired (#417) and must appear nowhere in generated C; got:\n{}",
            retired,
            out
        );
    }
    /* And the PAL's own pair keeps its spelling, which is the half a
     * search-and-replace across both layers would have broken. */
    assert!(
        !out.contains("oz_static_heap_alloc_obj") && !out.contains("oz_static_heap_free_obj"),
        "the PAL's oz_heap_alloc_obj/oz_heap_free_obj must not have been renamed too; \
         got:\n{}",
        out
    );
}
