#pragma once

#import "OZObject.h"

@interface AccDataProducer: OZObject
@property(assign, nonatomic, getter=ackCount) int count;

- (void)sendData;

@end
