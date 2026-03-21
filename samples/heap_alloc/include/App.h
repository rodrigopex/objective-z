#import <Foundation/Foundation.h>

@interface App : OZObject
@property(readonly, nonatomic) OZHeap *heap;
+ (instancetype)shared;
@end
