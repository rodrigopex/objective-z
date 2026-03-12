#include "Producer.h"
#include "channels.h"

#import <Foundation/OZLog.h>
#include <zephyr/kernel.h>

ZBUS_MSG_SUBSCRIBER_DEFINE(msub_consumed);

@interface AccDataProducer: OZObject
@property (readonly) int count;
- (void)sendData;
@end

@implementation AccDataProducer {
	int _count;
}

@synthesize count = _count;

- (id)init
{
	self = [super init];
	self->_count = 0;
	return self;
}

- (void)sendData
{
	self->_count = self->_count + 1;
}

@end

void thread_entry_producer(void *arg1, void *arg2, void *arg3)
{
	/* Thread entry point */
}

K_THREAD_DEFINE(thread_producer_id, 4096, thread_entry_producer, NULL, NULL, NULL, 3, 0, 0);
