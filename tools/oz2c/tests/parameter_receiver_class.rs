// SPDX-License-Identifier: Apache-2.0
//
// parameter_receiver_class.rs -- `arc.rs` did not know `method_parameter`
// exists, so a statically dispatched send took its ownership answer from
// an ambiguous poll over classes it can never reach (#481).
//
// `collect_declared_types` matched `"declaration" | "parameter_declaration"`.
// An Objective-C method parameter is a `method_parameter` node, and that
// kind appeared **zero** times in all of `arc.rs` -- against four times
// each in `staticbar.rs` and `collect.rs`. So `arc.rs` was the only module
// that did not know the kind existed.
//
// **The defect is not "a parameter receiver is unresolved".** The emitter
// resolves it perfectly well: `ctx.scope` is seeded from
// `collect::extract_method_sig`, and the emitted call is *static*. Only
// `arc` failed, so `message_target` answered `None`,
// `dispatch_ownership` fell through to polling every reachable
// implementor, and where two classes declare the selector and disagree
// the poll answered `Ambiguous` -- which reads as borrowed and emits no
// release.
//
// So a **statically dispatched send took its ownership from an ambiguous
// poll over classes it can never reach**, and the `Ambiguous` refusal in
// `emit::dynamic_dispatch_call` could not save it: that guards a
// *dynamically* dispatched send. Two resolvers, one receiver, disagreeing
// silently.
//
// **Three conditions have to coincide, and the tests below are built
// around that rather than around the issue's first description.** The
// receiver is a method parameter; the selector is *not* a create-rule
// name; and two or more classes declare it with disagreeing ownership.
//
// #477's original D6 repro was `[t copy]` on a parameter, and it **does
// not leak** -- `copy` is an exact entry in `CREATE_RULE_SELECTORS`, so
// `creates_reference` short-circuits the poll and the receiver's class is
// never needed. A test written from that description would have passed on
// unfixed code. Both non-reproducing shapes are pinned below as controls,
// so nobody re-derives them and nobody mistakes them for coverage.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// `Thing` plus a dealloc counter, shared by every case.
const THING: &str = "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (int)tag;
@end
@implementation Thing
- (int)tag {
	return 7;
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end
";

/// Two classes declaring one non-create-rule selector and **disagreeing**:
/// the shape that makes the poll ambiguous.
const DISAGREEING: &str = "\
@interface Owner : OZObject
- (Thing *)build;
@end
@implementation Owner
- (Thing *)build {
	return [[Thing alloc] init];
}
@end

@interface Lender : OZObject
{
	Thing *_held;
}
- (Thing *)build;
@end
@implementation Lender
- (Thing *)build {
	return _held;
}
@end
";

/// The defect: all three conditions, receiver a method parameter.
///
/// A runtime dealloc oracle rather than a grep for the release, because
/// this whole program has now produced five cases where an absence or
/// presence claim about emitted text held while the property was gone.
#[test]
fn a_parameter_receiver_resolves_so_the_send_is_not_polled_ambiguously() {
    let src = format!(
        "/* oz-pool: Thing=4,Owner=2,Lender=2,P=1 */\n{}{}{}{}",
        PREAMBLE(),
        THING,
        DISAGREEING,
        "\
@interface P : OZObject
- (int)viaParameter:(Owner *)o;
@end
@implementation P
- (int)viaParameter:(Owner *)o {
	Thing *t = [o build];
	return [t tag];
}
@end

#include <stdio.h>
int main(void) {
	Owner *o = [Owner alloc];
	P *p = [P alloc];
	int v = [p viaParameter:o];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "parameter_receiver_disagreeing");
    assert_eq!(
        out, "v=7 deallocs=1\n",
        "-build on a parameter receiver hands back +1 and must be released: {}",
        out
    );
}

/// The same disagreement with the receiver declared as a **local**, which
/// was always correct.
///
/// The control that localises the defect to the parameter kind rather than
/// to the disagreement. Without it, a reader cannot tell whether the bug
/// was about parameters or about ambiguity.
#[test]
fn a_local_receiver_was_always_resolved() {
    let src = format!(
        "/* oz-pool: Thing=4,Owner=2,Lender=2,P=1 */\n{}{}{}{}",
        PREAMBLE(),
        THING,
        DISAGREEING,
        "\
@interface P : OZObject
- (int)viaLocal;
@end
@implementation P
- (int)viaLocal {
	Owner *o = [Owner alloc];
	Thing *t = [o build];
	return [t tag];
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int v = [p viaLocal];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "local_receiver_disagreeing");
    assert_eq!(
        out, "v=7 deallocs=1\n",
        "a local receiver was always resolvable and must stay correct: {}",
        out
    );
}

/// **Control: the issue's original repro, which never leaked.** Condition
/// 2 fails.
///
/// `[t copy]` on a parameter receiver releases correctly *without* this
/// fix, because `copy` is an exact entry in `CREATE_RULE_SELECTORS` and
/// `creates_reference` short-circuits `dispatch_ownership` before the
/// implementor poll -- so the receiver's class is never consulted.
///
/// Pinned so that this shape is never mistaken for coverage of the defect.
/// A test written from #477's description would have been exactly this and
/// would have passed on unfixed code.
#[test]
fn a_create_rule_selector_never_needed_the_receivers_class() {
    let src = format!(
        "/* oz-pool: Thing=4,P=1 */\n{}{}{}",
        PREAMBLE(),
        THING,
        "\
@interface Thing (Copying)
- (Thing *)copy;
@end
@implementation Thing (Copying)
- (Thing *)copy {
	return [[Thing alloc] init];
}
@end

@interface P : OZObject
- (int)viaCopy:(Thing *)t;
@end
@implementation P
- (int)viaCopy:(Thing *)t {
	Thing *c = [t copy];
	return [c tag];
}
@end

#include <stdio.h>
int main(void) {
	Thing *t = [[Thing alloc] init];
	P *p = [P alloc];
	int v = [p viaCopy:t];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "create_rule_on_parameter");
    /* One dealloc: the -copy result. `t` is the driver's and still live. */
    assert_eq!(
        out, "v=7 deallocs=1\n",
        "a create-rule selector short-circuits the poll and was never affected: {}",
        out
    );
}

/// **Control: an unambiguous analysis-derived factory, which never
/// leaked.** Condition 3 fails.
///
/// With only one class declaring `-build`, the implementor poll agrees and
/// answers `Owning` without the receiver's class. So the parameter kind
/// alone is not sufficient for the defect -- the disagreement is what
/// makes the unresolved receiver matter.
#[test]
fn one_implementor_means_the_poll_agrees_without_the_class() {
    let src = format!(
        "/* oz-pool: Thing=4,Owner=2,P=1 */\n{}{}{}",
        PREAMBLE(),
        THING,
        "\
@interface Owner : OZObject
- (Thing *)build;
@end
@implementation Owner
- (Thing *)build {
	return [[Thing alloc] init];
}
@end

@interface P : OZObject
- (int)viaParameter:(Owner *)o;
@end
@implementation P
- (int)viaParameter:(Owner *)o {
	Thing *t = [o build];
	return [t tag];
}
@end

#include <stdio.h>
int main(void) {
	Owner *o = [Owner alloc];
	P *p = [P alloc];
	int v = [p viaParameter:o];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "one_implementor_parameter");
    assert_eq!(
        out, "v=7 deallocs=1\n",
        "a single implementor makes the poll unanimous, so this never leaked: {}",
        out
    );
}
