#import <Foundation/OZObject.h>

@interface Proxy : OZObject
- (void)forwardInvocation:(id)invocation;
@end

@implementation Proxy

- (void)forwardInvocation:(id)invocation {
}

@end
