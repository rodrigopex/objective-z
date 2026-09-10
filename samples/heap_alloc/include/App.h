#import <Foundation/Foundation.h>

@interface App : OZObject <OZSingletonProtocol>
@property(readonly, nonatomic) OZHeap *heap;
+ (instancetype)sharedInstance;
@end
