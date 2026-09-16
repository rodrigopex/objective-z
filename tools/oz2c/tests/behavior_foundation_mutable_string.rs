// SPDX-License-Identifier: Apache-2.0
//
// behavior_foundation_mutable_string.rs - OZ-092 Foundation work:
// OZMutableString, ported from tests/behavior/cases/foundation/
// mutable_string_basic.m (11 assertions -- 8 exercise OZMutableString
// itself, 3 exercise plain OZString methods -- hasPrefix:/hasSuffix:/
// isEqualToString: -- not covered by the earlier OZString port).
//
// Uses the real `OZObject` (`common::ozobject_src`) as the root class and
// the real `OZString` (`common::ozstring_src`) as OZMutableString's
// superclass. OZMutableString itself (`common::ozmutablestring_src`) is a
// full transplant, own `-init*`/`-dealloc` included -- unlike OZArray, it
// needs no oz2c-side special-casing at all: its growable `_data`
// buffer is already malloc-based in the real source (ordinary string-
// growth logic, not the object's own alloc/free machinery), and
// `[super init]`/inherited-ivar access through a base chain are both
// already exercised elsewhere (behavior_dispatch.rs, end_to_end_behavior.rs).

mod common;
use common::{compile_and_run, ozmutablestring_src, ozobject_src as PREAMBLE, ozstring_src};

#[test]
fn mutable_string_basic_all_operations() {
    let src = format!(
        "{}{}{}\n\
@interface MutableStringTest : OZObject {{
	struct OZMutableString *_ms;
}}
- (void)buildFromCString;
- (void)buildFromOZString;
- (void)buildWithCapacity;
- (void)buildAndAppendCString;
- (void)buildAndAppendString;
- (void)buildAndAppendGrow;
- (void)buildAndSetString;
- (void)buildAndSetStringNil;
- (const char *)result;
- (unsigned int)resultLength;
- (BOOL)hasPrefixTrue;
- (BOOL)hasSuffixTrue;
- (BOOL)isEqualToStringTrue;
@end

@implementation MutableStringTest

- (void)buildFromCString {{
	_ms = [[OZMutableString alloc] initWithCString:\"hello\"];
}}

- (void)buildFromOZString {{
	OZString *src = @\"world\";
	_ms = [[OZMutableString alloc] initWithString:src];
}}

- (void)buildWithCapacity {{
	_ms = [[OZMutableString alloc] initWithCapacity:64];
	[_ms appendCString:\"reserved\"];
}}

- (void)buildAndAppendCString {{
	_ms = [[OZMutableString alloc] initWithCString:\"hello\"];
	[_ms appendCString:\", world\"];
}}

- (void)buildAndAppendString {{
	_ms = [[OZMutableString alloc] initWithCString:\"hello\"];
	OZString *suffix = @\", world\";
	[_ms appendString:suffix];
}}

- (void)buildAndAppendGrow {{
	_ms = [[OZMutableString alloc] initWithCString:\"a\"];
	[_ms appendCString:\"bcdefghijklmnop\"];
	[_ms appendCString:\"qrstuvwxyz\"];
}}

- (void)buildAndSetString {{
	_ms = [[OZMutableString alloc] initWithCString:\"old content\"];
	OZString *replacement = @\"new\";
	[_ms setString:replacement];
}}

- (void)buildAndSetStringNil {{
	_ms = [[OZMutableString alloc] initWithCString:\"content\"];
	[_ms setString:nil];
}}

- (const char *)result {{
	return [_ms cString];
}}

- (unsigned int)resultLength {{
	return [_ms length];
}}

- (BOOL)hasPrefixTrue {{
	OZString *s = @\"hello world\";
	OZString *prefix = @\"hello\";
	return [s hasPrefix:prefix];
}}

- (BOOL)hasSuffixTrue {{
	OZString *s = @\"hello world\";
	OZString *suffix = @\"world\";
	return [s hasSuffix:suffix];
}}

- (BOOL)isEqualToStringTrue {{
	OZString *a = @\"hello\";
	OZString *b = @\"hello\";
	return [a isEqualToString:b];
}}

@end

#include <stdio.h>
int main(void) {{
	MutableStringTest *t = [MutableStringTest alloc];
	[t buildFromCString];
	printf(\"from_cstring=%s len=%u\\n\", [t result], [t resultLength]);
	[t buildFromOZString];
	printf(\"from_ozstring=%s\\n\", [t result]);
	[t buildWithCapacity];
	printf(\"with_capacity=%s\\n\", [t result]);
	[t buildAndAppendCString];
	printf(\"append_cstring=%s\\n\", [t result]);
	[t buildAndAppendString];
	printf(\"append_string=%s\\n\", [t result]);
	[t buildAndAppendGrow];
	printf(\"append_grow=%s\\n\", [t result]);
	[t buildAndSetString];
	printf(\"set_string=%s\\n\", [t result]);
	[t buildAndSetStringNil];
	printf(\"set_string_nil=%s\\n\", [t result]);
	printf(\"has_prefix=%d\\n\", [t hasPrefixTrue]);
	printf(\"has_suffix=%d\\n\", [t hasSuffixTrue]);
	printf(\"is_equal=%d\\n\", [t isEqualToStringTrue]);
	return 0;
}}
",
        PREAMBLE(),
        ozstring_src(),
        ozmutablestring_src()
    );
    let stdout = compile_and_run(&src, "mutable_string_basic_all_operations");
    assert_eq!(
        stdout,
        "from_cstring=hello len=5\n\
         from_ozstring=world\n\
         with_capacity=reserved\n\
         append_cstring=hello, world\n\
         append_string=hello, world\n\
         append_grow=abcdefghijklmnopqrstuvwxyz\n\
         set_string=new\n\
         set_string_nil=\n\
         has_prefix=1\n\
         has_suffix=1\n\
         is_equal=1\n"
    );
}

// ---------------------------------------------------------------------------
// The description of a mutable string (#421)
// ---------------------------------------------------------------------------
//
// #421 reported `%@` on an `OZMutableString` printing `<OZMutableString:
// 0xADDRESS>` -- `OZObject`'s inherited default from #354 -- because the class
// "declares and implements no `-getDescription:maxLength:` at all". The grep in
// the report is accurate: it has none, and `OZString`, `OZArray`,
// `OZDictionary` and `OZNumber` each have their own. The conclusion drawn from
// it is not. `OZMutableString`'s superclass is `OZString`, which implements it,
// and `companion.rs` resolves each class's protocol-dispatch arm by walking the
// superclass chain -- so the arm reads
//
//     case OZ_CLASS_OZMutableString:
//             return OZString_getDescription_maxLength_((struct OZString *)self, ...);
//
// and `%@` prints the contents already. That resolution is what these tests
// pin; before them nothing did, which is how the class came to look broken from
// the outside. Measured on target rather than argued: `mps2/an385`, one
// `OZMutableString` built with `-initWithCString:"hello"` and
// `-appendCString:" world"`, logged through `OZLog("%@")` both as its own type
// and through an `id`, prints
//
//     ms=hello world
//     anon=hello world
//
// A *direct* send would prove nothing here -- it lowers to a static call to
// whichever implementation the receiver's declared type inherits, which is
// `OZString`'s either way. The `id`-typed receiver is the one that goes through
// `OZ_PROTOCOL_SEND_getDescription_maxLength_`, which is the mechanism #421 is
// about, so the run below asserts on that path and the emitted call is asserted
// on too.
//
// `%@` itself cannot be exercised on the host: `src/OZLog.c` includes
// `zephyr/sys/printk.h` and this harness does not link it (the same reason
// `default_description.rs` asserts through a send). The dynamic send plus the
// dispatcher assertion are the host-side halves of it, and the paragraph above
// is the on-target half.
//
// Verified not to pass vacuously, which for a test pinning *inherited*
// behaviour is the whole question. Two counterfactuals, because the first did
// not reach the shape #421 describes:
//
//   - `-getDescription:maxLength:` deleted from `src/OZString.m` but left
//     declared in `OZString.h`: no class in the chain implements it, so
//     `OZMutableString` gets no dispatcher arm at all and the switch's
//     `default: return 0` answers. Both tests fail, with `n=0 text=[]` -- the
//     pre-#354 no-op, indistinguishable from a description that really is
//     empty, which is what #354 exists to have stopped.
//   - deleted from `src/OZString.m` *and* `OZString.h`: the arm resolves one
//     step further up and reaches `OZObject_getDescription_maxLength_`. Both
//     tests fail, and the run prints
//     `n=30 text=[<OZMutableString: 0xb22c00900>]` -- exactly the output #421
//     reported, so that is the tree its report describes.

/// Through an `id`, so the send is dynamic: the contents, bounded by `maxLen`.
#[test]
fn a_mutable_strings_description_is_its_contents() {
    let src = format!(
        "/* oz-pool: OZMutableString=2,Describer=1 */\n{}{}{}\n\
@interface Describer : OZObject
- (void)run;
@end

@implementation Describer
- (void)run {{
	char buf[64];
	int n = 0;
	OZMutableString *ms = [[OZMutableString alloc] initWithCString:\"hello\"];
	id anon = nil;

	[ms appendCString:\" world\"];
	anon = ms;

	memset(buf, 0, sizeof(buf));
	n = [anon getDescription:buf maxLength:sizeof(buf) - 1];
	printf(\"n=%d text=[%s]\\n\", n, buf);

	/* `maxLen` is a hard bound and truncation is silent, the contract
	 * every description in this SDK has. */
	memset(buf, 0, sizeof(buf));
	n = [anon getDescription:buf maxLength:5];
	printf(\"clipped n=%d text=[%s]\\n\", n, buf);
}}
@end

#include <stdio.h>
#include <string.h>
int main(void) {{
	Describer *d = [[Describer alloc] init];
	[d run];
	return 0;
}}
",
        PREAMBLE(),
        ozstring_src(),
        ozmutablestring_src()
    );

    let stdout = compile_and_run(&src, "mutable_string_description_is_its_contents");
    assert_eq!(
        stdout,
        "n=11 text=[hello world]\nclipped n=5 text=[hello]\n",
        "a mutable string's description must be its contents, not its address"
    );
}

/// The dispatcher arm itself: `OZString`'s implementation, not `OZObject`'s
/// default. Asserted on the text because that is where the resolution shows,
/// and because the run above cannot tell a correct answer reached by the wrong
/// route from one reached by the right one.
#[test]
fn the_description_dispatcher_routes_a_mutable_string_to_its_superclass() {
    let src = format!(
        "{}{}{}\n\
@interface Describer : OZObject
- (int)describe:(id)obj into:(char *)buf;
@end
@implementation Describer
- (int)describe:(id)obj into:(char *)buf {{
	return [obj getDescription:buf maxLength:16];
}}
@end
",
        PREAMBLE(),
        ozstring_src(),
        ozmutablestring_src()
    );

    let out = oz2c::transpile(&src).expect("should transpile");
    let c = &out.companion_c;

    assert!(
        c.contains(
            "case OZ_CLASS_OZMutableString: return \
             OZString_getDescription_maxLength_("
        ),
        "OZMutableString's description arm does not reach OZString's \
         implementation:\n{}",
        c
    );
    assert!(
        !c.contains(
            "case OZ_CLASS_OZMutableString: return \
             OZObject_getDescription_maxLength_("
        ),
        "OZMutableString's description arm fell through to OZObject's \
         `<ClassName: 0xADDRESS>` default (#421):\n{}",
        c
    );
    /* And the send that reaches it is the dynamic one, not a static call
     * resolved from a declared type. */
    assert!(
        out.source_c.contains("OZ_PROTOCOL_SEND_getDescription_maxLength_"),
        "the `id`-typed send did not go through the protocol dispatcher:\n{}",
        out.source_c
    );
}

// ---------------------------------------------------------------------------
// Mutators on a receiver that never ran an initialiser (#542)
// ---------------------------------------------------------------------------
//
// `_oz_alloc` memsets a fresh slab slot, and nothing in `OZMutableString`
// requires an initialiser before a mutator, so `[OZMutableString alloc]`
// followed by a send arrives in `-appendCString:` and `-setString:` with
// `_capacity == 0` and `_data == NULL`. Three defects followed, all fixed in
// `src/OZMutableString.m` -- which `ozmutablestring_src` splices in verbatim
// via `include_str!`, so this test reads the shipped source:
//
//   - both capacity-doubling loops seeded `newCap` at `_capacity` and doubled,
//     and `0 * 2 == 0` never reaches `newLen + 1`. Not a crash: an unbounded
//     hang, with no allocator to fail and no fault to trap.
//   - `-appendCString:`'s grow path did `memcpy(newBuf, _data, _length)` with
//     `_data == NULL`, which ISO C leaves undefined even at length zero.
//   - `-setString:`'s nil branch wrote `((char *)_data)[0] = '\0'` through
//     that NULL. Its *argument* check was present and correct; the receiver's
//     state was what went unchecked.
//
// `alarm(3)` rather than a bare run, and it is load-bearing: `compile_and_run`
// executes the built binary through `Command::output()` with no timeout of its
// own, so a regression in either loop would wedge `cargo test` -- and CI --
// rather than fail it. Under SIGALRM the process dies, `output()` returns, the
// harness's `status.success()` assertion fires, and the failure names itself.
// The behavior corpus half of this coverage
// (`tests/behavior/cases/foundation/mutable_string_basic.m`) needs no such
// trick: its runner already bounds the binary at 30s.
//
// Counterfactuals, each with the other two fixes left in place: dropping the
// `-setString:` floor times out at 30s in the corpus run; dropping the nil
// guard exits 245. Both floors and the guard were reverted one at a time, so
// no one of the three is carrying the other two.
#[test]
fn mutators_survive_a_receiver_that_never_ran_an_initialiser() {
    let src = format!(
        "/* oz-pool: OZMutableString=4,Uninit=1 */\n{}{}{}\n\
@interface Uninit : OZObject
- (void)run;
@end

@implementation Uninit
- (void)run {{
	/* Appending to a zero-capacity receiver: the floored grow has to
	 * terminate *and* allocate a usable buffer, so the contents are
	 * asserted rather than just the survival. */
	OZMutableString *appended = [OZMutableString alloc];
	OZMutableString *assigned = [OZMutableString alloc];
	OZMutableString *emptied = [OZMutableString alloc];
	OZString *replacement = @\"assigned from nothing\";

	[appended appendCString:\"grown from nothing\"];
	printf(\"appended=%s len=%u\\n\", [appended cString], (unsigned)[appended length]);

	[assigned setString:replacement];
	printf(\"assigned=%s len=%u\\n\", [assigned cString], (unsigned)[assigned length]);

	/* No contents to assert: `-cString` returns `_data` verbatim and
	 * this receiver has no buffer. Reaching the print is the claim. */
	[emptied setString:nil];
	printf(\"emptied len=%u\\n\", (unsigned)[emptied length]);
}}
@end

#include <stdio.h>
#include <unistd.h>
int main(void) {{
	Uninit *u = [[Uninit alloc] init];
	/* See the comment above: the pre-#542 failure is a hang, and the
	 * Rust harness runs the binary unbounded. */
	alarm(3);
	[u run];
	alarm(0);
	return 0;
}}
",
        PREAMBLE(),
        ozstring_src(),
        ozmutablestring_src()
    );

    let stdout = compile_and_run(&src, "mutable_string_uninitialised_receiver");
    assert_eq!(
        stdout,
        "appended=grown from nothing len=18\n\
         assigned=assigned from nothing len=21\n\
         emptied len=0\n",
        "a mutator on a receiver straight off `+alloc` must terminate and \
         must not write through a NULL `_data` (#542)"
    );
}
