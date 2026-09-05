// SPDX-License-Identifier: Apache-2.0
//
// end_to_end_behavior.rs - OZ-091 Track B: the static subset must not
// just parse and emit -- it must compile against the real PAL (host
// backend) and run with correct behavior.

mod common;
use common::{compile_and_run, ozobject_src};

#[test]
fn dispatch_refcounting_super_and_ivars() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Sensor : OZObject {
	int _value;
}
- (void)setValue:(int)v;
- (int)value;
- (void)bump:(int)delta andLog:(int)flag;
@end

@implementation Sensor
- (void)setValue:(int)v {
	_value = v;
}
- (int)value {
	return _value;
}
- (void)bump:(int)delta andLog:(int)flag {
	_value = _value + delta;
	if (flag) {
		_value = _value;
	}
}
- (void)dealloc {
	[super dealloc];
}
@end

#include <stdio.h>

int main(void) {
	Sensor *s = [Sensor alloc];
	[s setValue:10];
	[s bump:5 andLog:1];
	[s retain];
	[s release];
	printf(\"value=%d\\n\", [s value]);
	[s release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "dispatch_refcounting_super_and_ivars");
    assert_eq!(stdout, "value=15\n");
}

#[test]
fn class_methods_initialize_and_singleton() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Counter : OZObject {
	int _n;
}
+ (void)initialize;
+ (Counter *)shared;
- (int)bump;
@end

@implementation Counter
static Counter *gShared;

+ (void)initialize {
	gShared = [Counter alloc];
}
+ (Counter *)shared {
	return gShared;
}
- (int)bump {
	_n = _n + 1;
	return _n;
}
@end

#include <stdio.h>

int main(void) {
	Counter *c = [Counter shared];
	printf(\"bump1=%d\\n\", [c bump]);
	printf(\"bump2=%d\\n\", [c bump]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "class_methods_initialize_and_singleton");
    assert_eq!(stdout, "bump1=1\nbump2=2\n");
}

#[test]
fn inherited_ivar_access_through_base_chain() {
    // A subclass method reading/writing an ivar declared by an ancestor
    // (not itself) must use the correct `self->base.<name>` hop path.
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Base : OZObject {
	size_t _count;
}
@end
@implementation Base
@end

@interface Mid : Base
@end
@implementation Mid
@end

@interface Leaf : Mid
- (void)bumpCount;
- (size_t)count;
@end
@implementation Leaf
- (void)bumpCount {
	_count = _count + 1;
}
- (size_t)count {
	return _count;
}
@end

#include <stdio.h>

int main(void) {
	Leaf *l = [Leaf alloc];
	[l bumpCount];
	[l bumpCount];
	printf(\"count=%d\\n\", [l count]);
	[l release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "inherited_ivar_access_through_base_chain");
    assert_eq!(stdout, "count=2\n");
}
