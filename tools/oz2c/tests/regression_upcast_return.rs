// SPDX-License-Identifier: Apache-2.0
//
// regression_upcast_return.rs - #532 regression tests.
//
// A method declared to return a superclass and returning a subclass
// instance -- `- (OZString *)labelWith:` building an `OZMutableString *`
// and handing it back -- is a plain upcast: legal Objective-C, accepted by
// Clang, and the shape of any factory typed as the abstract class. But
// inheritance here is struct embedding, so C has no implicit conversion
// from a subclass struct pointer to its base, and the uncast C `return`
// oz2c used to emit is
//
//   error: returning 'struct OZMutableString *' from a function with
//   incompatible return type 'struct OZString *'
//
// on GCC. Found by writing px-app's challenge suite (CHALLENGES.md F9,
// worked around there as WA-016).
//
// `render_return_statement` has two exits and both emitted it uncast, so
// there are two tests:
//
//   1. the plain return, which is rebuilt in place;
//   2. the same return with an ARC release owed by another local in the
//      same scope, which goes out through the cleanup path instead --
//      `{ret_ty} {tmp} = {value};` there, so the identical
//      incompatible-pointer diagnostic one construct over.
//
// Both use `compile_and_run_strict`
// (`-Werror=incompatible-pointer-types`), and that is load-bearing:
// Apple clang on this host only *warns* on an incompatible pointer
// return, so under plain `compile_and_run` these would pass with the
// defect fully present -- the same trap
// `regression_instancetype_covariance.rs` documents for OZ-100.

mod common;
use common::{
    compile_and_run_strict, ozmutablestring_src, ozobject_src as PREAMBLE, ozstring_src,
};

/// Exit 1: the plain `return built;`, with nothing else owed at the
/// return. This is the shape px-app hit twice.
#[test]
fn upcast_return_is_cast_to_the_declared_return_type() {
    let src = format!(
        "/* oz-pool: OZMutableString=2,Labeller=1 */\n{}{}{}\n\
@interface Labeller : OZObject
- (OZString *)labelWith:(OZString *)prefix;
@end

@implementation Labeller

- (OZString *)labelWith:(OZString *)prefix {{
	OZMutableString *built = [[OZMutableString alloc] initWithString:prefix];
	[built appendCString:\"-trace\"];
	return built;
}}

@end

#include <stdio.h>

int main(void) {{
	Labeller *l = [[Labeller alloc] init];
	OZString *label = [l labelWith:@\"px\"];

	printf(\"label=[%s]\\n\", [label cString]);
	return 0;
}}
",
        PREAMBLE(),
        ozstring_src(),
        ozmutablestring_src()
    );

    let stdout = compile_and_run_strict(&src, "upcast_return_plain");
    assert_eq!(
        stdout, "label=[px-trace]\n",
        "a subclass instance returned from a superclass-typed method must be \
         cast to the declared return type"
    );
}

/// Exit 2: the same upcast with an ARC release live in the same scope, so
/// the `return` leaves through the cleanup path and the cast has to land
/// on the temporary's *initialiser* rather than on the `return`.
///
/// `scratch` is the release: it is owned, it is not what is returned, and
/// so it is released on the way out. Nothing about it is otherwise
/// interesting -- its only job is to make `releases_for_all_scopes`
/// answer non-empty, which is what selects the other exit.
///
/// It is read back through `-cString` rather than handed to
/// `-appendString:` on purpose. The obvious `[built appendString:scratch]`
/// is an upcast in an *argument* position, which is the same missing cast
/// in a different place and not what #532 covers -- writing it here made
/// this test fail for that reason as well as its own, which is exactly
/// the kind of doubled cause a regression test must not have.
#[test]
fn upcast_return_is_cast_on_the_cleanup_path_too() {
    let src = format!(
        "/* oz-pool: OZMutableString=3,Labeller=1 */\n{}{}{}\n\
@interface Labeller : OZObject
- (OZString *)labelWith:(OZString *)prefix;
@end

@implementation Labeller

- (OZString *)labelWith:(OZString *)prefix {{
	OZMutableString *scratch = [[OZMutableString alloc] initWithCString:\"scratch\"];
	OZMutableString *built = [[OZMutableString alloc] initWithString:prefix];

	[scratch appendCString:\"!\"];
	[built appendCString:\"-\"];
	[built appendCString:[scratch cString]];
	return built;
}}

@end

#include <stdio.h>

int main(void) {{
	Labeller *l = [[Labeller alloc] init];
	OZString *label = [l labelWith:@\"px\"];

	printf(\"label=[%s]\\n\", [label cString]);
	return 0;
}}
",
        PREAMBLE(),
        ozstring_src(),
        ozmutablestring_src()
    );

    let stdout = compile_and_run_strict(&src, "upcast_return_with_cleanup");
    assert_eq!(
        stdout, "label=[px-scratch!]\n",
        "the cleanup path's temporary is declared with the method's return \
         type, so the upcast needs the same cast there"
    );
}
