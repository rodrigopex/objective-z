// SPDX-License-Identifier: Apache-2.0
//
// integer_slot_ownership.rs -- a scope releases only slots that can hold
// a reference (#380).
//
// `emit::owned_locals_of` decided what a scope releases from
// `arc::binds_ownership` alone, with no check that the slot is a pointer.
// That is sound only if `binds_ownership` never answers yes for an
// expression whose value is not an object -- and it does, because it looks
// through a non-bridging cast, deliberately and correctly (#332). So
//
//     int n = (int)makeThing();
//
// emitted `oz_static_release((struct OZObject *)(n))`. The cast in the
// generated release is what let it compile, and then the decrement landed
// at whatever address the integer held.
//
// The width decides how bad it is, and it is why no board gate caught it:
// on a 64-bit host `int` truncates the pointer, so the release is a wrong
// store; on the 32-bit targets that ship, the integer holds the whole
// pointer and the release is accidentally correct.
//
// **The reference is now leaked rather than released, and that is the
// intended answer, not a compromise.** Once a pointer is cast to an
// integer nothing static can follow it -- ARC requires
// `__bridge_retained`/`CFBridgingRetain` for exactly this shape and treats
// the reference as handed to the programmer. Leaking is also the direction
// the standing rule demands when ownership cannot be established: a leak
// is recoverable, a wrong store is not.
//
// This is the third position to need the pointer check locally rather than
// by widening `binds_ownership`: `arc::hoists_owning_operand`'s callers
// (#375) and the `for`-header arm (#376) were the first two.

mod common;
use common::{compile_and_run_strict, ozobject_src};

const PRELUDE: &str = "\
#include <stdio.h>

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

Thing *makeThing(int n)
{
	return [[Thing alloc] initWithN:n];
}
";

fn program(body: &str) -> String {
    format!("{}{}\n{}", ozobject_src(), PRELUDE, body)
}

/// The defect, in the truncating width.
///
/// Asserted on the emitted text, because that is where the wrongness is:
/// on this host the truncated address is very unlikely to be mapped, so a
/// run would show a crash *or* silence depending on the allocator, and
/// neither is evidence about what was emitted. The run is here too, to
/// show the program no longer touches that address at all.
#[test]
fn an_int_slot_holding_a_cast_pointer_is_not_released() {
    let src = program(
        "\
int main(void)
{
	int n = (int)makeThing(7);

	printf(\"n!=0 is %d\\n\", n != 0);
	return 0;
}
",
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        !out.source_c.contains("oz_static_release((struct OZObject *)(n))"),
        "an `int` slot cannot hold a reference and must not be released through; got:\n{}",
        out.source_c
    );
    assert_eq!(compile_and_run_strict(&src, "int_slot"), "n!=0 is 1\n");
}

/// `long`, where the integer does hold the whole pointer on this host, so
/// the release was accidentally *correct* and only the emitted text
/// distinguishes right from lucky.
///
/// Worth its own case: a fix that keyed on the width rather than on
/// "is this a pointer slot" would pass the case above and leave this one
/// releasing through an integer.
#[test]
fn a_long_slot_holding_a_cast_pointer_is_not_released_either() {
    let src = program(
        "\
int main(void)
{
	long n = (long)makeThing(7);

	printf(\"n!=0 is %d\\n\", n != 0);
	return 0;
}
",
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        !out.source_c.contains("oz_static_release((struct OZObject *)(n))"),
        "a `long` slot must not be released through either, however wide it is; got:\n{}",
        out.source_c
    );
    assert_eq!(compile_and_run_strict(&src, "long_slot"), "n!=0 is 1\n");
}

/// The guard must not cost the ordinary case anything: a genuine object
/// pointer bound from the same cast shape is still owned and still
/// released.
///
/// This is #332's own shape -- `Thing *t = (Thing *)[Thing alloc];` leaked
/// until `binds_ownership` looked through the cast -- so it is exactly
/// what a careless pointer check would break, and it is asserted on the
/// run as well as the text: three allocations on one slab slot only
/// succeed if each is released.
#[test]
fn a_pointer_slot_bound_through_a_cast_is_still_released() {
    let src = program(
        "\
int main(void)
{
	int i;

	for (i = 0; i < 3; i++) {
		Thing *t = (Thing *)makeThing(i);
		printf(\"t=%d\\n\", [t n]);
	}
	return 0;
}
",
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("oz_static_release((struct OZObject *)(t))"),
        "a cast between object pointers still binds ownership (#332); got:\n{}",
        out.source_c
    );
    assert_eq!(compile_and_run_strict(&src, "ptr_slot_cast"), "t=0\nt=1\nt=2\n");
}
