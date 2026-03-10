#import "OZObject.h"

@protocol IteratorProtocol

@required
@property (nonatomic, readonly) uint16_t iterIdx;

- (instancetype)iter;
- (id)next;

@end
