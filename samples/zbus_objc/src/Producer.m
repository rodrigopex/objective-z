#include "Producer.h"
#include "zephyr/sys/printk.h"

#include <zephyr/kernel.h>
#include <zephyr/zbus/zbus.h>
#include <zephyr/random/random.h>

ZBUS_MSG_SUBSCRIBER_DEFINE(msub_acc_consumed);

ZBUS_CHAN_ADD_OBS(chan_acc_data_consumed, msub_acc_consumed, 3);

@implementation AccDataProducer

@synthesize ackCount = _count;

- (id)init
{
	self = [super init];

	self->_count = 0;

	return self;
}
- (void)dealloc
{
	printk("Deallocating AccDataProducer instance\n");
	[super dealloc];
}

- (int)sendData
{
	struct msg_acc_data msg = {
		.x = sys_rand8_get() % 100, .y = sys_rand8_get() % 100, .z = sys_rand8_get() % 100};

	zbus_chan_pub(&chan_acc_data, &msg, K_NO_WAIT);

	const struct zbus_channel *chan;
	struct msg_acc_data_consumed msg_consumed;

	zbus_sub_wait_msg(&msub_acc_consumed, &chan, &msg_consumed, K_FOREVER);

	self->_count = msg_consumed.count;
}

@end

void thread_entry_producer(void *arg1, void *arg2, void *arg3)
{
	AccDataProducer *producer = [[AccDataProducer alloc] init];

	while (1) {
		[producer sendData];

		if (producer.ackCount > 9) {
			printk("Producer: Received %d acknowledgments from Consumer. Stopping "
			       "production.\n",
			       producer.ackCount);
			[producer dealloc];

			break;
		}

		k_sleep(K_MSEC(1000));
	}
}

K_THREAD_DEFINE(thread_producer_id, 4096, thread_entry_producer, NULL, NULL, NULL, 3, 0, 0);
