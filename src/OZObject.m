/* Root class implementation for OZ transpiler samples. */

#import <Foundation/OZObject.h>

@implementation OZObject
+ (instancetype)alloc
{
	return nil;
}
- (instancetype)init
{
	return self;
}
- (void)dealloc
{
}
- (BOOL)isEqual:(id)anObject
{
	return self == anObject;
}
- (int)cDescription:(char *)buf maxLength:(int)maxLen
{
	return 0;
}
@end
