// SPDX-License-Identifier: Apache-2.0
//
// behavior_blocks.rs - OZ-092: port of tests/behavior/cases/blocks/ (the
// Python-pipeline oracle) to oz_static. All 3 fixtures are in scope: the
// static bar only rejects *capturing* blocks, and none of these capture
// self/an ivar/an enclosing local -- see staticbar.rs's check_block_capture.
//
// OZ-099 adds one more, non-oracle case: a `__block`-qualified local is
// promoted to a file-scope static rather than rejected as a capture (see
// emit.rs's `hoist_block_var`) -- self/ivar/plain-local capture is still
// rejected, covered by static_bar_rejects.rs's capturing_block_rejected
// and self_capturing_block_rejected.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// Port of non_capturing_basic.m/_test.c: a block literal assigned to a
/// local, capturing nothing but its own param, called through that local.
#[test]
fn non_capturing_basic() {
    let src = format!(
        "{}\n\
@interface BlockBasicTest : OZObject {{
\tint _result;
}}
- (void)run;
- (int)result;
@end

@implementation BlockBasicTest
- (void)run {{
\tint (^square)(int) = ^(int x) {{
\t\treturn x * x;
\t}};
\t_result = square(7);
}}
- (int)result {{
\treturn _result;
}}
@end

#include <stdio.h>

int main(void) {{
\tBlockBasicTest *t = [BlockBasicTest alloc];
\t[t run];
\tprintf(\"result=%d\\n\", [t result]);
\t[t release];
\treturn 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "non_capturing_basic");
    assert_eq!(stdout, "result=49\n");
}

/// Port of block_with_static_var.m/_test.c: a block referencing a
/// file-scope static variable. A plain C global/static isn't stack state
/// the block would need to keep alive, so it's not a "capture" in the
/// ObjC sense -- the static bar accepts it (see staticbar.rs's
/// find_capture: it only flags self, ivars, and enclosing locals/params).
#[test]
fn block_with_static_var() {
    let src = format!(
        "{}\n\
static int g_multiplier = 3;

@interface StaticBlockTest : OZObject {{
\tint _result;
}}
- (void)run;
- (int)result;
@end

@implementation StaticBlockTest
- (void)run {{
\tint (^mul)(int) = ^(int x) {{
\t\treturn x * g_multiplier;
\t}};
\t_result = mul(5);
}}
- (int)result {{
\treturn _result;
}}
@end

#include <stdio.h>

int main(void) {{
\tStaticBlockTest *t = [StaticBlockTest alloc];
\t[t run];
\tprintf(\"result=%d\\n\", [t result]);
\t[t release];
\treturn 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "block_with_static_var");
    assert_eq!(stdout, "result=15\n");
}

/// Adapted from block_as_method_param.m/_test.c. The Python oracle's C
/// harness passes a plain C function where a block-typed parameter is
/// expected (Python erases blocks to function pointers at the ABI level,
/// so that's a valid call there); oz_static has no separate test harness
/// language to exploit that shortcut in. Instead this exercises the more
/// interesting structural case the fixture is really about -- a block
/// *typed* parameter (`(int (^)(int))blk`) -- with an actual `^(...)`
/// block literal passed directly as the message argument (not assigned to
/// a local first), since block translation happens wherever a
/// `block_literal` node appears in the tree, including inline in a
/// message send's argument list.
#[test]
fn block_as_method_param() {
    let src = format!(
        "{}\n\
@interface BlockParamTest : OZObject {{
\tint _computed;
}}
- (void)applyBlock:(int (^)(int))blk toValue:(int)v;
- (int)computed;
@end

@implementation BlockParamTest
- (void)applyBlock:(int (^)(int))blk toValue:(int)v {{
\t_computed = blk(v);
}}
- (int)computed {{
\treturn _computed;
}}
@end

#include <stdio.h>

int main(void) {{
\tBlockParamTest *t = [BlockParamTest alloc];
\t[t applyBlock:^(int x) {{
\t\treturn x * 2;
\t}} toValue:21];
\tprintf(\"computed=%d\\n\", [t computed]);
\t[t release];
\treturn 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "block_as_method_param");
    assert_eq!(stdout, "computed=42\n");
}

/// OZ-099: `__block`-qualified locals, promoted to file scope (see
/// emit.rs's `hoist_block_var`) rather than rejected as a capture --
/// matching oz_transpile's own `__block`/BlocksAttr promotion
/// (collect.py `_collect_block_vars`, emit.py:1179-1181). Not a port of
/// a Python-pipeline oracle fixture (there isn't one under
/// tests/behavior/cases/blocks/ yet), but the same pattern the repo's
/// own samples/transpiled_blocks/src/main.m documents and relies on:
/// a block mutating a `__block` local across repeated calls.
#[test]
fn block_mutates_block_qualified_var() {
    let src = format!(
        "{}\n\
@interface CounterTest : OZObject {{
\tint _result;
}}
- (void)run;
- (int)result;
@end

@implementation CounterTest
- (void)run {{
\t__block int counter = 0;
\tvoid (^increment)(void) = ^{{
\t\tcounter++;
\t}};
\tincrement();
\tincrement();
\tincrement();
\t_result = counter;
}}
- (int)result {{
\treturn _result;
}}
@end

#include <stdio.h>

int main(void) {{
\tCounterTest *t = [CounterTest alloc];
\t[t run];
\tprintf(\"result=%d\\n\", [t result]);
\t[t release];
\treturn 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "block_mutates_block_qualified_var");
    assert_eq!(stdout, "result=3\n");
}
