/*
 * Adapted from: clang/test/CodeGenObjC/arc-precise-lifetime.m
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies retain/release calls are inserted at the
 *             correct scope boundaries in the transpiled C output.
 */
#import "OZTestBase.h"

@interface Resource : OZObject {
	int _value;
}
- (int)value;
- (void)setValue:(int)value;
@end

@implementation Resource
- (int)value { return _value; }
- (void)setValue:(int)value { _value = value; }
@end

@interface Manager : OZObject {
	Resource *_resource;
}
- (void)setResource:(Resource *)resource;
- (Resource *)resource;
- (int)resourceValue;
@end

@implementation Manager
- (void)setResource:(Resource *)resource { _resource = resource; }
- (Resource *)resource { return _resource; }
- (int)resourceValue { return [_resource value]; }
@end
