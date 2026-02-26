#include "TemperatureService.h"

#include <objc/OZLog.h>
#include <zephyr/kernel.h>
#include <zephyr/zbus/zbus.h>
#include <zephyr/random/random.h>

ZBUS_CHAN_DEFINE(chan_temperature_service_invoke, struct msg_temperature_service_invoke, NULL, NULL,
		 ZBUS_OBSERVERS(_alis_temperature_service), ZBUS_MSG_INIT(0));

ZBUS_CHAN_DEFINE(chan_temperature_service_report, struct msg_temperature_service_report, NULL, NULL,
		 ZBUS_OBSERVERS(_msub_temp_serv_report_channel), ZBUS_MSG_INIT(0));

void alis_invoke_callback(const struct zbus_channel *chan, const void *message)
{
	struct msg_temperature_service_report report = {.tag = TEMPERATURE_SERVICE_REPORT_ERROR,
							.timestamp = k_uptime_get(),
							.error = {
								.code = -ENODEV,
							}};

	if (chan == &chan_temperature_service_invoke) {

		const struct msg_temperature_service_invoke *msg = message;

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

@implementation TemperatureService

static const struct zbus_channel *_invokeChannel = &chan_temperature_service_invoke;
static const struct zbus_channel *_reportChannel = &chan_temperature_service_report;

static void _observe_report_channel(BOOL obs)
{
	zbus_obs_set_enable(&_msub_temp_serv_report_channel, obs);
}

static void _wait_report(const struct zbus_channel **chan,
			 struct msg_temperature_service_report *msg, k_timeout_t timeout)
{
	zbus_sub_wait_msg(&_msub_temp_serv_report_channel, chan, msg, timeout);
}

+ (const struct zbus_channel *)invokeChannel
{
	return _invokeChannel;
}

+ (const struct zbus_channel *)reportChannel
{
	return _reportChannel;
}

+ (int)requestTemperature:(k_timeout_t)timeout
{
	return zbus_chan_pub(&chan_temperature_service_invoke,
			     &(struct msg_temperature_service_invoke){
				     .tag = TEMPERATURE_SERVICE_INVOKE_REQ_TEMP},
			     timeout);
}

+ (int)requestTemperatureWithRef:(int *)temperature andTimeout:(k_timeout_t)timeout
{
	k_timepoint_t end_time = sys_timepoint_calc(timeout);

	_observe_report_channel(YES);

	if ([TemperatureService requestTemperature:timeout] == 0) {
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

	zbus_obs_set_enable(&_msub_temp_serv_report_channel, false);
	return -EAGAIN;
}

@end
