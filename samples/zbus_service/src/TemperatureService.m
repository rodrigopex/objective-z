#include "TemperatureService.h"

#include <Foundation/OZLog.h>
#include <zephyr/kernel.h>
#include <zephyr/zbus/zbus.h>
#include <zephyr/random/random.h>

ZBUS_CHAN_DEFINE(chan_temperature_service_invoke, struct msg_temperature_service_invoke, NULL, NULL,
		 ZBUS_OBSERVERS(_alis_temperature_service), ZBUS_MSG_INIT(0));

ZBUS_CHAN_DEFINE(chan_temperature_service_report, struct msg_temperature_service_report, NULL, NULL,
		 ZBUS_OBSERVERS(_msub_temp_serv_report_channel), ZBUS_MSG_INIT(0));

void alis_invoke_callback(const struct zbus_channel *chan, const void *message)
{
    const struct msg_temperature_service_invoke *msg = message;
	struct msg_temperature_service_report report = {
        .context = msg->context,
	    .tag = TEMPERATURE_SERVICE_REPORT_ERROR,
	    .timestamp = k_uptime_get(),
	    .error = {
					.code = -ENODEV,
	    }
	};

	if (chan == &chan_temperature_service_invoke) {
		switch (msg->tag) {
		case TEMPERATURE_SERVICE_INVOKE_REQ_TEMP: {
			k_usleep(1000000 * (sys_rand8_get() % 5));

			report.tag = TEMPERATURE_SERVICE_REPORT_TEMPERATURE;
			report.timestamp = k_uptime_get(),
			report.temperature.value = sys_rand8_get() % 100;

			zbus_chan_pub(&chan_temperature_service_report, &report, K_MSEC(250));
		} break;
		default:
			report.error.code = -EINVAL;
			goto cleanup;
		}
	}

	return;
cleanup:

	zbus_chan_pub(&chan_temperature_service_report, &report, K_MSEC(250));
}

ZBUS_MSG_SUBSCRIBER_DEFINE_WITH_ENABLE(_msub_temp_serv_report_channel, false);

ZBUS_ASYNC_LISTENER_DEFINE(_alis_temperature_service, alis_invoke_callback);

static void _observe_report_channel(BOOL enable)
{
    printk("%s observe report channel\n", enable ? "enable" : "disable");
	zbus_obs_set_enable(&_msub_temp_serv_report_channel, enable);
}

static int _wait_report(const struct zbus_channel **chan,
			 struct msg_temperature_service_report *msg, k_timeout_t timeout)
{
	return zbus_sub_wait_msg(&_msub_temp_serv_report_channel, chan, msg, timeout);
}

static int _request_temperature(k_timeout_t timeout){
   	return zbus_chan_pub(&chan_temperature_service_invoke,
			     &(struct msg_temperature_service_invoke){
				     .tag = TEMPERATURE_SERVICE_INVOKE_REQ_TEMP},
			     timeout);
}

static TemperatureService *_shared;

@implementation TemperatureService

@synthesize invokeChannel = _invokeChannel;
@synthesize reportChannel = _reportChannel;

-(instancetype)init {
    self = [super init];
    if (self) {
        _invokeChannel = &chan_temperature_service_invoke;
        _reportChannel = &chan_temperature_service_report;
    }
    return self;
}

+ (void)initialize
{
    _shared = [[TemperatureService alloc] init];
}

+ (instancetype)shared
{
    return _shared;
}

- (int)requestTemperature:(k_timeout_t)timeout
{
	return _request_temperature(timeout);
}

- (int)requestTemperatureWithRef:(int *)temperature andTimeout:(k_timeout_t)timeout
{
	k_timepoint_t end_time = sys_timepoint_calc(timeout);

	_observe_report_channel(YES);

	if (_request_temperature(timeout) == 0) {
		const struct zbus_channel *chan;
		struct msg_temperature_service_report msg;

		_wait_report(&chan, &msg, sys_timepoint_timeout(end_time));

		_observe_report_channel(NO);

		if (msg.tag == TEMPERATURE_SERVICE_REPORT_ERROR) {
			return msg.error.code;
		}

		*temperature = msg.temperature.value;

		return 0;
	} else {
		OZLog("Error!");
	}

	_observe_report_channel(NO);
	return -EAGAIN;
}
- (int)requestTemperatureWithBlock:(void (^)(int error_code,int temperature))block andTimeout:(k_timeout_t)timeout
{
    k_timepoint_t end_time = sys_timepoint_calc(timeout);

    _observe_report_channel(YES);

	if (_request_temperature(timeout) == 0) {
		const struct zbus_channel *chan;
		struct msg_temperature_service_report msg;

		int err = _wait_report(&chan, &msg, sys_timepoint_timeout(end_time));

		_observe_report_channel(NO);

		if(err!=0) {
			block(err, 0);
		} else if (msg.tag == TEMPERATURE_SERVICE_REPORT_ERROR) {
		    block(msg.error.code, 0);
			err = msg.error.code;
		} else {
    		block(0, msg.temperature.value);
		}

		return err;

	} else {
		OZLog("Error!");
	}

	_observe_report_channel(NO);
	return -EAGAIN;
}

@end
