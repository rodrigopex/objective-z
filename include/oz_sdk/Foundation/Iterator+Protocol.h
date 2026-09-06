#import "OZObject.h"

@protocol IteratorProtocol <ObjectProtocol>

@required
@property (nonatomic, readonly) uint16_t iterIdx;

- (instancetype)iter;
- (id)next;

@end
