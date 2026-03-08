#import "OZObject.h"

@protocol IteratorProtocol <OZObject>

@required
@property (nonatomic, readonly) NSUInteger iterIdx;

- (instancetype)iter;
- (id)next;

@end
