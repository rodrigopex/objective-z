// SPDX-License-Identifier: Apache-2.0
//
// behavior_dot_syntax.rs - Objective-C property dot syntax (`obj.prop`,
// `obj.prop = v`), lowered to the accessor call by
// `emit::render_field_expression` / `render_assignment_expression`.
//
// This was a silent gap rather than a rejected construct: a `.` on an
// object used to pass straight through as C member access, which the C
// compiler then rejected on a pointer -- `samples/heap_alloc` failed with
// "member reference type 'struct App *' is a pointer; did you mean '->'".
//
// The shapes covered here are the ones that actually occur across the
// repository's Objective-C sources; a survey found ten dot accesses in
// `samples/`, all reads, in four distinct shapes:
//
//   * `super.spec`             (samples/gpio_demo)  -- dot syntax on `super`
//   * `producer.ackCount`      (samples/zbus_objc)  -- a `getter=` rename
//   * `str.cString`            (samples/zbus_service) -- a bare getter, no
//                                                     `@property` at all
//   * `[App sharedInstance].heap` (samples/heap_alloc) -- on a send result
//
// The write and compound-write forms occur nowhere in the repository, so
// they are covered here on their own account rather than pinned by a
// sample. The oracle's own `tests/behavior/cases/properties/dot_syntax.m`
// is named for this feature but never uses it -- it declares a property and
// stops -- so there is no coverage on that side to compare against.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src};

/// A property read, a `getter=`-renamed read, and a plain getter method
/// with no `@property` behind it -- the three read shapes that differ in
/// how the selector is found, checked in one program.
#[test]
fn property_read_resolves_through_getter_selector() {
    let src = format!(
        "{}\n{}",
        ozobject_src(),
        "\
#include <stdio.h>
@interface Gauge : OZObject {
	int _reading;
	int _ticks;
}
@property(assign, nonatomic) int reading;
@property(assign, nonatomic, getter=tickCount) int ticks;
- (int)doubled;
@end

@implementation Gauge
@synthesize reading = _reading;
@synthesize ticks = _ticks;
/* No @property for this one: Objective-C accepts dot syntax against a
   bare getter method too, which is how `str.cString` works. */
- (int)doubled {
	return _reading * 2;
}
@end

int main(void) {
	Gauge *g = [Gauge alloc];
	[g setReading:21];
	[g setTicks:7];
	printf(\"reading=%d\\n\", g.reading);
	printf(\"ticks=%d\\n\", g.tickCount);
	printf(\"doubled=%d\\n\", g.doubled);
	[g release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "property_read_resolves_through_getter_selector");
    assert_eq!(stdout, "reading=21\nticks=7\ndoubled=42\n");
}

/// `obj.prop = v` is the setter call, not an assignment to it -- including
/// through a `setter=` rename, and including the retain/release a strong
/// object property's synthesized setter does.
#[test]
fn property_write_becomes_the_setter_call() {
    let src = format!(
        "{}\n{}",
        ozobject_src(),
        "\
#include <stdio.h>
@interface Slot : OZObject {
	int _value;
	int _limit;
}
@property(assign, nonatomic) int value;
@property(assign, nonatomic, setter=putLimit:) int limit;
@end

@implementation Slot
@synthesize value = _value;
@synthesize limit = _limit;
@end

int main(void) {
	Slot *s = [Slot alloc];
	s.value = 12;
	s.limit = 99;
	printf(\"value=%d limit=%d\\n\", s.value, [s limit]);
	s.value = s.value + 5;
	printf(\"value=%d\\n\", s.value);
	[s release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "property_write_becomes_the_setter_call");
    assert_eq!(stdout, "value=12 limit=99\nvalue=17\n");
}

/// `super.prop` must call the superclass's accessor directly. If it were
/// routed through the receiver's own class_id switch the way an ordinary
/// send is, an override reading `super.thing` would re-enter itself and
/// recurse forever -- so this test would not return at all rather than
/// return a wrong number.
#[test]
fn super_property_read_calls_the_superclass_accessor() {
    let src = format!(
        "{}\n{}",
        ozobject_src(),
        "\
#include <stdio.h>
@interface Base : OZObject {
	int _tag;
}
@property(assign, nonatomic) int tag;
@end

@implementation Base
@synthesize tag = _tag;
@end

@interface Leaf : Base
- (int)tag;
@end

@implementation Leaf
/* Overrides the getter and reads the inherited one through `super`. */
- (int)tag {
	return super.tag + 100;
}
@end

int main(void) {
	Leaf *l = [Leaf alloc];
	[l setTag:5];
	printf(\"own=%d\\n\", [l tag]);
	printf(\"dot=%d\\n\", l.tag);
	[l release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "super_property_read_calls_the_superclass_accessor");
    assert_eq!(stdout, "own=105\ndot=105\n");
}

/// Dot syntax chains: the inner accessor's return type is what resolves
/// the outer field, exactly as it would for a chain of message sends.
#[test]
fn chained_property_read_resolves_through_the_inner_return_type() {
    let src = format!(
        "{}\n{}",
        ozobject_src(),
        "\
#include <stdio.h>
@interface Inner : OZObject {
	int _depth;
}
@property(assign, nonatomic) int depth;
@end
@implementation Inner
@synthesize depth = _depth;
@end

@interface Outer : OZObject {
	Inner *_inner;
}
@property(assign, nonatomic) Inner *inner;
@end
@implementation Outer
@synthesize inner = _inner;
@end

int main(void) {
	Inner *i = [Inner alloc];
	i.depth = 3;
	Outer *o = [Outer alloc];
	o.inner = i;
	printf(\"depth=%d\\n\", o.inner.depth);
	[o release];
	[i release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "chained_property_read_resolves_through_the_inner_return_type");
    assert_eq!(stdout, "depth=3\n");
}

/// Plain C member access on a struct value is not dot syntax and must pass
/// through untouched. Only an object-typed left side makes the `.`
/// Objective-C's -- and in C, `.` on a pointer is not legal at all, so
/// there is nothing ambiguous left to decide.
#[test]
fn c_struct_member_access_is_left_alone() {
    let src = format!(
        "{}\n{}",
        ozobject_src(),
        "\
#include <stdio.h>
struct point {
	int x;
	int y;
};

@interface Plotter : OZObject
- (int)sumOf:(struct point)p;
@end

@implementation Plotter
- (int)sumOf:(struct point)p {
	return p.x + p.y;
}
@end

int main(void) {
	struct point p;
	p.x = 4;
	p.y = 6;
	Plotter *plot = [Plotter alloc];
	printf(\"sum=%d\\n\", [plot sumOf:p]);
	[plot release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "c_struct_member_access_is_left_alone");
    assert_eq!(stdout, "sum=10\n");
}

/// Reaching a bare ivar through dot syntax is not Objective-C -- `.` is
/// accessor syntax, and Clang rejects it the same way. Rewriting it to
/// `->` would compile, which is exactly why it is not done: it would
/// accept a program the language does not, and bypass whatever the real
/// accessor does.
#[test]
fn dot_syntax_on_a_bare_ivar_is_rejected() {
    let src = format!(
        "{}\n{}",
        ozobject_src(),
        "\
@interface Hidden : OZObject {
	int _secret;
}
@end
@implementation Hidden
@end

int main(void) {
	Hidden *h = [Hidden alloc];
	return h._secret;
}
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("has no property or getter named '_secret'"),
        "unexpected diagnostics: {}",
        diags
    );
}

/// Assigning to a readonly property is an error in Objective-C, and says
/// so here rather than emitting a call to a setter that was never
/// generated.
#[test]
fn assigning_to_a_readonly_property_is_rejected() {
    let src = format!(
        "{}\n{}",
        ozobject_src(),
        "\
@interface Meter : OZObject {
	int _count;
}
@property(readonly, nonatomic) int count;
@end
@implementation Meter
@synthesize count = _count;
@end

int main(void) {
	Meter *m = [Meter alloc];
	m.count = 4;
	return 0;
}
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("is a readonly property"),
        "unexpected diagnostics: {}",
        diags
    );
}

/// A compound assignment has to read the property and write it back, which
/// mentions the receiver twice. Where the receiver is a plain identifier
/// that is provably harmless, so it is allowed; where it is a message send
/// it would send twice, and stays a hard error instead.
#[test]
fn compound_assignment_is_allowed_on_a_plain_receiver_and_rejected_otherwise() {
    let src = format!(
        "{}\n{}",
        ozobject_src(),
        "\
#include <stdio.h>
@interface Tally : OZObject {
	int _total;
}
@property(assign, nonatomic) int total;
@end
@implementation Tally
@synthesize total = _total;
@end

int main(void) {
	Tally *t = [Tally alloc];
	t.total = 10;
	t.total += 5;
	t.total *= 2;
	printf(\"total=%d\\n\", t.total);
	[t release];
	return 0;
}
"
    );
    let stdout = compile_and_run(
        &src,
        "compound_assignment_is_allowed_on_a_plain_receiver_and_rejected_otherwise",
    );
    assert_eq!(stdout, "total=30\n");

    let rejected = format!(
        "{}\n{}",
        ozobject_src(),
        "\
@interface Tally : OZObject {
	int _total;
}
@property(assign, nonatomic) int total;
+ (instancetype)shared;
@end
@implementation Tally
@synthesize total = _total;
+ (instancetype)shared {
	return [Tally alloc];
}
@end

int main(void) {
	[Tally shared].total += 5;
	return 0;
}
"
    );
    let diags = expect_reject(&rejected);
    assert!(
        diags.contains("would evaluate the receiver"),
        "unexpected diagnostics: {}",
        diags
    );
}
