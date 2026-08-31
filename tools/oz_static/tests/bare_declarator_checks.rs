// SPDX-License-Identifier: Apache-2.0
//
// bare_declarator_checks.rs -- checks that used to skip a declaration written
// without an initializer (#240).
//
// In tree-sitter-objc a pointer declaration with no initializer produces a
// `pointer_declarator` and no `init_declarator` anywhere:
//
//     declaration :: "Counter *c;"
//       type_identifier    :: "Counter"
//       pointer_declarator :: "*c"   <- not an `init_declarator`, not an `identifier`
//       ;
//
// Several places matched declarators by kind and listed only
// `init_declarator` and `identifier`, so each silently skipped the bare form.
// #234 fixed one of them (`emit::collect_local_decls`, which left such a local
// out of `ctx.scope` so a send to it was rejected as an `id` receiver); these
// are the rest.
//
// Every case below pairs the bare spelling with the initialized one, because
// the bug was never that a check was wrong -- it was that the check never ran.
// Asserting only the bare form would pass just as well against a build where
// the check had been deleted outright.

mod common;
use common::{compile_and_run, expect_reject, ozarray_src, ozobject_src, ozq31_src};

fn transpiles(src: &str) -> bool {
    oz_static::transpile(src).is_ok()
}

/// A block capturing a stack local is rejected -- blocks are hoisted to plain
/// C functions, which have no closure to carry a captured variable in.
///
/// `scope.locals` was filled from `init_declarator` only, so `find_capture`
/// never matched a bare-declared name and the capture was accepted. The
/// consequence was loud but unhelpful: the hoisted function could not see the
/// variable, giving `use of undeclared identifier 'n'` against generated code
/// the user never wrote, with no located oz_static diagnostic.
#[test]
fn bare_declared_local_captured_by_block_rejected() {
    let bare = format!(
        "{}\n@interface Foo : OZObject\n- (int)run;\n@end\n@implementation Foo\n\
         - (int)run {{\n\tint n;\n\tn = 7;\n\tint (^f)(void) = ^{{ return n; }};\n\treturn f();\n}}\n@end\n\
         int main(void) {{ return 0; }}\n",
        ozobject_src()
    );
    let diags = expect_reject(&bare);
    assert!(diags.contains("block captures 'n'"), "diagnostics: {}", diags);

    // The initialized spelling, which was already rejected -- so the test
    // cannot pass by the check having been removed.
    let with_init = bare.replace("int n;\n\tn = 7;", "int n = 7;");
    let init_diags = expect_reject(&with_init);
    assert!(init_diags.contains("block captures 'n'"), "diagnostics: {}", init_diags);
}

/// The object-typed counterpart, which is the shape that actually occurs:
/// #234 made a bare object local type-tracked, so writing one and capturing it
/// became reachable in a way it had not been before.
#[test]
fn bare_declared_object_local_captured_by_block_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (void)poke;\n- (void)run;\n@end\n@implementation Foo\n\
         - (void)poke {{ }}\n\
         - (void)run {{\n\tFoo *p;\n\tp = [Foo alloc];\n\tvoid (^f)(void) = ^{{ [p poke]; }};\n\t(void)f;\n}}\n@end\n\
         int main(void) {{ return 0; }}\n",
        ozobject_src()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("block captures 'p'"), "diagnostics: {}", diags);
}

/// `__block` is the *supported* way to share a local with a block: those are
/// promoted to file-scope statics (`emit::hoist_block_var`), so they are not
/// real stack captures and are exempt from the check above.
///
/// This non-pointer case already worked before #240, and is here to pin that
/// the `staticbar` fix did not break it: recording a bare declaration in
/// `scope.locals` must also record it in `block_locals` when the declaration
/// is `__block`-qualified, or the exemption would be lost and this would start
/// being rejected. The pointer case below is the one that was broken.
///
/// Runs rather than just transpiles, since the point is that the value is
/// really shared.
#[test]
fn bare_block_qualified_local_is_hoisted_not_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (int)run;\n@end\n@implementation Foo\n\
         - (int)run {{\n\t__block int n;\n\tn = 7;\n\tint (^f)(void) = ^{{ return n; }};\n\treturn f();\n}}\n@end\n\
         \n#include <stdio.h>\nint main(void) {{\n\tFoo *x = [Foo alloc];\n\tprintf(\"r=%d\\n\", [x run]);\n\treturn 0;\n}}\n",
        ozobject_src()
    );
    let stdout = compile_and_run(&src, "bare_block_qualified_local_is_hoisted_not_rejected");
    assert_eq!(stdout, "r=7\n", "a __block local must be hoisted and shared, not captured");
}

/// A `__block` *pointer* declared without an initializer was the one shape
/// `hoist_block_var` missed, and the reason it went unnoticed is instructive: a
/// bare non-pointer declarator is itself an `identifier`, so `__block int q;`
/// was already handled and only `__block Foo *p;` fell through. Nothing was
/// hoisted at all, so the block referenced a name that did not exist.
///
/// Measured before the fix: `__block Foo *p = 0;` hoisted
/// `static struct Foo * p = 0;`, `__block int q;` hoisted `static int q;`, and
/// `__block Foo *p;` hoisted nothing.
#[test]
fn bare_block_qualified_pointer_is_hoisted() {
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (int)run;\n@end\n@implementation Foo\n\
         - (int)run {{\n\t__block Foo *p;\n\tp = 0;\n\tint (^f)(void) = ^{{ return p == 0; }};\n\treturn f();\n}}\n@end\n\
         \n#include <stdio.h>\nint main(void) {{\n\tFoo *x = [Foo alloc];\n\tprintf(\"r=%d\\n\", [x run]);\n\treturn 0;\n}}\n",
        ozobject_src()
    );
    let out = oz_static::transpile(&src).expect("a __block local is supported, not rejected");
    assert!(
        out.source_c.contains("static struct Foo * p;")
            || out.source_c.contains("static struct Foo *p;"),
        "a bare __block pointer must be hoisted to a file-scope static; got:\n{}",
        out.source_c
    );
    // And it has to actually build and run, since an unhoisted one produced C
    // that referenced an undeclared name.
    let stdout = compile_and_run(&src, "bare_block_qualified_pointer_is_hoisted");
    assert_eq!(stdout, "r=1\n");
}

/// The generic element-type constraint must hold however the declaration is
/// spelled. This gap was *silent*: no diagnostic at all, and the generated C
/// compiled and ran with an unchecked element type -- which makes it worse than
/// the capture gap, since the whole value of a constraint check is that it
/// cannot be sidestepped.
#[test]
fn generic_constraint_checked_on_bare_declaration() {
    let base = format!(
        "{}{}{}\n@interface Widget : OZObject\n@end\n@implementation Widget\n@end\n",
        ozobject_src(),
        ozq31_src(),
        ozarray_src()
    );
    let bare = format!(
        "{}\n@interface Runner : OZObject\n- (void)run;\n@end\n@implementation Runner\n\
         - (void)run {{\n\tOZArray<Widget *> *a;\n\ta = @[@(1)];\n\t(void)a;\n}}\n@end\n\
         int main(void) {{ return 0; }}\n",
        base
    );
    let diags = expect_reject(&bare);
    assert!(diags.contains("does not satisfy constraint 'Widget'"), "diagnostics: {}", diags);

    // Already rejected before #240, for the same reason.
    let with_init = format!(
        "{}\n@interface Runner : OZObject\n- (void)run;\n@end\n@implementation Runner\n\
         - (void)run {{\n\tOZArray<Widget *> *a = @[@(1)];\n\t(void)a;\n}}\n@end\n\
         int main(void) {{ return 0; }}\n",
        base
    );
    let init_diags = expect_reject(&with_init);
    assert!(
        init_diags.contains("does not satisfy constraint 'Widget'"),
        "diagnostics: {}",
        init_diags
    );
}

/// The constraint check must not become over-broad: a bare declaration whose
/// element type *does* satisfy the constraint stays accepted.
#[test]
fn matching_generic_constraint_on_bare_declaration_accepted() {
    let src = format!(
        "{}{}{}\n@interface Runner : OZObject\n- (void)run;\n@end\n@implementation Runner\n\
         - (void)run {{\n\tOZArray<OZQ31 *> *a;\n\ta = @[@(1)];\n\t(void)a;\n}}\n@end\n\
         int main(void) {{ return 0; }}\n",
        ozobject_src(),
        ozq31_src(),
        ozarray_src()
    );
    assert!(transpiles(&src), "a satisfied constraint must stay accepted");
}
