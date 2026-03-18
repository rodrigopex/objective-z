/* Deferred cleanup implementation for OZ transpiler. */

#import <Foundation/OZDefer.h>

@implementation OZDefer

- (instancetype)initWithOwner:(id)owner block:(void (^)(id))aBlock
{
	_owner = owner;
	_block = aBlock;
	return self;
}

- (instancetype)initWithBlock:(void (^)(id))aBlock
{
	_owner = nil;
	_block = aBlock;
	return self;
}

- (void)dealloc
{
	if (_block) {
		_block(_owner);
	}
	[super dealloc];
}

@end
