#import <Foundation/Foundation.h>

@interface App : OZObject <SingletonProtocol>
@property(readonly, nonatomic) OZHeap *heap;
+ (instancetype)sharedInstance;
@end
