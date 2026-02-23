#include <objc/objc.h>
#include "channels.h"

#pragma once

@interface AccDataProducer: Object {
	int _count;
}

@property(assign, nonatomic) int ackCount;

- (void)sendData;

@end
