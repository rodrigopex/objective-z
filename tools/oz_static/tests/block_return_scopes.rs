// SPDX-License-Identifier: Apache-2.0
//
// block_return_scopes.rs -- a `return` inside a block literal releases
// only what the *block* owns (#342).
//
// A `block_literal` is rendered on the **enclosing** body's `EmitCtx`:
// block bodies deliberately share their enclosing body's flat scope, a
// stated spike simplification commented in `render_block`. So the
// enclosing method's ARC scopes are still stacked when the block's own
// `return` is rendered, and `releases_for_all_scopes` used to walk every
// one of them -- emitting the enclosing method's releases inside the
// *hoisted* function, where those names do not exist:
//
//     oz_static_release((struct OZObject *)(outerKeep));   <- not in scope
//
// `error: 'outerKeep' undeclared (first use in this function)`. The whole
// transpile produces C that cannot compile, so every case here is a hard
// failure without the fix rather than a wrong result -- and
// `compile_and_run_strict` is what proves it, since a text assertion alone
// could be satisfied by output that still does not build.
//
// Three things have to coincide to reach it: a block literal, an owned
// local in the enclosing body declared *before* it, and a cleanup pending
// at the block's own `return` (a second owned local that is not the
// returned value). Nothing in `samples/`, `tests/behavior/cases/` or
// `tests/adapted/` has that combination, which is why no gate caught it --
// the same reason #336 and #339 went unseen.
//
// The `@synchronized` case at the end is the same boundary reached through
// the other pending-cleanup list: `ctx.sync_cleanups` is not scope
// structure and so is cleared for the literal's body rather than marked
// (see `render_block`).

mod common;
use common::{compile_and_run_strict, ozobject_src};

const THING: &str = "\
@interface Thing : OZObject {
	int _n;
}
- (id)initWithN:(int)n;
- (int)n;
@end
@implementation Thing
- (id)initWithN:(int)n
{
	self = [super init];
	if (self != nil) {
		_n = n;
	}
	return self;
}
- (int)n
{
	return _n;
}
@end
";

fn program(body: &str) -> String {
    format!("{}{}\n{}", ozobject_src(), THING, body)
}

/// The text of every hoisted block function in `source_c`.
///
/// Each is introduced by the banner `render_block` writes and ends at the
/// first closing brace in column zero, which is where a synthesized
/// function's body ends -- the body is emitted with tab-indented
/// statements, so no inner brace can be mistaken for it.
///
/// Located by the banner rather than by the `oz_block_L..._C..._N` symbol
/// because that name carries a line number of the *merged* buffer, which
/// moves whenever anything above it in `ozobject_src()` does.
fn hoisted_blocks(source_c: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = source_c;
    while let Some(start) = rest.find("/* block at ") {
        let tail = &rest[start..];
        let end = tail.find("\n}").map(|at| at + 2).unwrap_or(tail.len());
        blocks.push(tail[..end].to_string());
        rest = &tail[end..];
    }
    assert!(!blocks.is_empty(), "no hoisted block function in:\n{}", source_c);
    blocks
}

/// Everything in `source_c` that is *not* a hoisted block function -- the
/// enclosing bodies, so a release can be asserted to have stayed with the
/// body that owns the local rather than merely to have moved.
fn outside_hoisted_blocks(source_c: &str) -> String {
    let mut kept = source_c.to_string();
    for block in hoisted_blocks(source_c) {
        kept = kept.replace(&block, "");
    }
    kept
}

fn release_of(name: &str) -> String {
    format!("oz_static_release((struct OZObject *)({}))", name)
}

/// The case #342 was filed on, reduced from the issue: an owned local in
/// the enclosing method declared ahead of the literal, and a block whose
/// own `return` has a release pending.
///
/// Without the fix the hoisted function releases `outerKeep`, a local of
/// `Host_run`, and GCC rejects the file outright.
#[test]
fn a_return_in_a_block_does_not_release_the_enclosing_bodys_locals() {
    let src = program(
        "\
#include <stdio.h>

@interface Host : OZObject
- (int)run;
@end
@implementation Host
- (int)run
{
	Thing *outerKeep = [[Thing alloc] initWithN:7];
	int (^mk)(void) = ^int(void) {
		Thing *innerKeep = [[Thing alloc] initWithN:1];
		Thing *out = [[Thing alloc] initWithN:2];
		printf(\"inner=%d out=%d\\n\", [innerKeep n], [out n]);
		return 1;
	};
	int v = mk();
	printf(\"outer=%d v=%d\\n\", [outerKeep n], v);
	return v;
}
@end

int main(void)
{
	Host *h = [[Host alloc] init];
	return [h run] - 1;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let hoisted = hoisted_blocks(&out.source_c);
    assert_eq!(hoisted.len(), 1, "expected exactly one hoisted block, got:\n{}", out.source_c);
    let block = &hoisted[0];

    assert!(
        !block.contains("outerKeep"),
        "the hoisted function names the enclosing method's local:\n{}",
        block
    );
    /* What the block *does* owe is still owed: both of its own locals,
     * innermost declaration first. An over-eager boundary that released
     * nothing at all would pass the assertion above. */
    assert!(block.contains(&release_of("out")), "the block stopped releasing its own:\n{}", block);
    assert!(
        block.contains(&release_of("innerKeep")),
        "the block stopped releasing its own:\n{}",
        block
    );
    /* And the enclosing method still releases it, on its own exit. */
    assert!(
        outside_hoisted_blocks(&out.source_c).contains(&release_of("outerKeep")),
        "the enclosing method no longer releases its own local:\n{}",
        out.source_c
    );

    let stdout = compile_and_run_strict(&src, "block_return_scope_outer");
    assert_eq!(stdout, "inner=1 out=2\nouter=7 v=1\n");
}

/// A `return` from inside a loop inside a block: the boundary is the
/// block's body, so the loop's own scope is released *and* the block's,
/// and the walk stops there.
///
/// This is the case that says the block mark is checked after a scope is
/// drained rather than before -- a boundary tested first would stop at the
/// loop and leak `innerKeep`.
#[test]
fn a_return_inside_a_loop_inside_a_block_releases_both_of_the_blocks_scopes() {
    let src = program(
        "\
#include <stdio.h>

@interface Host : OZObject
- (int)run;
@end
@implementation Host
- (int)run
{
	Thing *outerKeep = [[Thing alloc] initWithN:9];
	int (^mk)(void) = ^int(void) {
		Thing *innerKeep = [[Thing alloc] initWithN:3];
		for (int i = 0; i < 2; i++) {
			Thing *perIteration = [[Thing alloc] initWithN:4];
			printf(\"iter=%d per=%d\\n\", i, [perIteration n]);
			return [innerKeep n];
		}
		return 0;
	};
	int v = mk();
	printf(\"outer=%d v=%d\\n\", [outerKeep n], v);
	return v;
}
@end

int main(void)
{
	Host *h = [[Host alloc] init];
	return [h run] - 3;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let block = &hoisted_blocks(&out.source_c)[0];

    assert!(
        block.contains(&release_of("perIteration")),
        "the loop body's local was not released by the return:\n{}",
        block
    );
    assert!(
        block.contains(&release_of("innerKeep")),
        "the walk stopped at the loop instead of the block body:\n{}",
        block
    );
    assert!(
        !block.contains("outerKeep"),
        "the walk did not stop at the block body:\n{}",
        block
    );

    let stdout = compile_and_run_strict(&src, "block_return_scope_loop");
    assert_eq!(stdout, "iter=0 per=4\nouter=9 v=3\n");
}

/// `break` is unaffected. It uses `releases_up_to_loop`, which stops at
/// the nearest loop body and was already correct -- the block mark must
/// not reach it, or a `break` would start releasing the block's locals
/// while the block goes on running.
#[test]
fn a_break_inside_a_blocks_loop_still_stops_at_the_loop() {
    let src = program(
        "\
#include <stdio.h>

@interface Host : OZObject
- (int)run;
@end
@implementation Host
- (int)run
{
	Thing *outerKeep = [[Thing alloc] initWithN:8];
	int (^mk)(void) = ^int(void) {
		Thing *innerKeep = [[Thing alloc] initWithN:5];
		for (int i = 0; i < 3; i++) {
			Thing *perIteration = [[Thing alloc] initWithN:6];
			printf(\"iter=%d per=%d\\n\", i, [perIteration n]);
			if (i == 1) {
				break;
			}
		}
		printf(\"after loop inner=%d\\n\", [innerKeep n]);
		return [innerKeep n];
	};
	int v = mk();
	printf(\"outer=%d v=%d\\n\", [outerKeep n], v);
	return v;
}
@end

int main(void)
{
	Host *h = [[Host alloc] init];
	return [h run] - 5;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let block = &hoisted_blocks(&out.source_c)[0];

    /* The `break` releases the iteration's local and nothing further:
     * `innerKeep` is read *after* the loop, so a break that released it
     * would hand `-n` a freed object. */
    let at_break = block
        .lines()
        .position(|l| l.trim() == "break;")
        .expect(&format!("no break in the hoisted function:\n{}", block));
    let before_break: String = block.lines().take(at_break).collect::<Vec<_>>().join("\n");
    let tail_of_break: String = before_break
        .lines()
        .rev()
        .take_while(|l| l.contains("oz_static_release"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        tail_of_break.contains(&release_of("perIteration")),
        "the break stopped releasing the loop's local:\n{}",
        block
    );
    assert!(
        !tail_of_break.contains("innerKeep"),
        "the break unwound past the loop into the block's own scope:\n{}",
        block
    );

    let stdout = compile_and_run_strict(&src, "block_return_scope_break");
    assert_eq!(stdout, "iter=0 per=6\niter=1 per=6\nafter loop inner=5\nouter=8 v=5\n");
}

/// A `return` in the enclosing body, *after* the literal, still releases
/// the enclosing body's locals. The mark is popped with the block's scope,
/// so nothing about the enclosing walk changes -- the mirror of the
/// restore `render_block` does for `method_return_type` (#339).
#[test]
fn a_return_after_a_block_literal_still_releases_the_enclosing_locals() {
    let src = program(
        "\
#include <stdio.h>

@interface Host : OZObject
- (int)run;
@end
@implementation Host
- (int)run
{
	Thing *keptBefore = [[Thing alloc] initWithN:1];
	int (^mk)(void) = ^int(void) {
		Thing *inner = [[Thing alloc] initWithN:2];
		return [inner n];
	};
	Thing *keptAfter = [[Thing alloc] initWithN:3];
	printf(\"before=%d after=%d v=%d\\n\", [keptBefore n], [keptAfter n], mk());
	return 0;
}
@end

int main(void)
{
	Host *h = [[Host alloc] init];
	return [h run];
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let enclosing = outside_hoisted_blocks(&out.source_c);
    for name in ["keptBefore", "keptAfter"] {
        assert!(
            enclosing.contains(&release_of(name)),
            "the enclosing method's `{}` is never released:\n{}",
            name,
            out.source_c
        );
    }

    let stdout = compile_and_run_strict(&src, "block_return_scope_after");
    assert_eq!(stdout, "before=1 after=3 v=2\n");
}

/// The same boundary through `ctx.sync_cleanups`: a block literal written
/// inside an `@synchronized` body, whose own `return` has a cleanup
/// pending.
///
/// Without the clear, the hoisted function replays the enclosing body's
/// `oz_spin_unlock` -- naming the lock temporary the *enclosing* body
/// declared, so again no valid C, and semantically wrong even if the name
/// resolved: the block runs at its call site, which may be outside the
/// critical section entirely.
#[test]
fn a_return_in_a_block_does_not_replay_the_enclosing_synchronized_unlock() {
    let src = program(
        "\
#include <stdio.h>

@interface Host : OZObject
- (int)run;
@end
@implementation Host
- (int)run
{
	int v = 0;
	@synchronized(self) {
		Thing *outerKeep = [[Thing alloc] initWithN:1];
		int (^mk)(void) = ^int(void) {
			Thing *innerKeep = [[Thing alloc] initWithN:2];
			Thing *out = [[Thing alloc] initWithN:3];
			printf(\"inner=%d out=%d\\n\", [innerKeep n], [out n]);
			return 4;
		};
		v = mk();
		printf(\"outer=%d\\n\", [outerKeep n]);
	}
	printf(\"v=%d\\n\", v);
	return v;
}
@end

int main(void)
{
	Host *h = [[Host alloc] init];
	return [h run] - 4;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let block = &hoisted_blocks(&out.source_c)[0];

    assert!(
        !block.contains("oz_spin_unlock"),
        "the hoisted function replays the enclosing body's unlock:\n{}",
        block
    );
    assert!(!block.contains("outerKeep"), "and its releases too:\n{}", block);
    /* The enclosing body still unlocks on its own way out. */
    assert!(
        outside_hoisted_blocks(&out.source_c).contains("oz_spin_unlock"),
        "the enclosing body stopped unlocking:\n{}",
        out.source_c
    );

    let stdout = compile_and_run_strict(&src, "block_return_scope_sync");
    assert_eq!(stdout, "inner=2 out=3\nouter=1\nv=4\n");
}
