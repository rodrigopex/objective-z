#include <objc/objc.h>
#include <zephyr/zbus/zbus.h>

#pragma once

struct msg_temperature_service_invoke {
	enum {
		TEMPERATURE_SERVICE_INVOKE_REQ_TEMP,
	} tag;
};

struct msg_temperature_service_report {
	enum {
		TEMPERATURE_SERVICE_REPORT_TEMPERATURE,
		TEMPERATURE_SERVICE_REPORT_ERROR,
	} tag;
	uint64_t timestamp;

	union {
		struct {
			int value;
		} temperature;

		struct {
			int code;
		} error;
	};
};

ZBUS_CHAN_DECLARE(chan_temperature_service_invoke, chan_temperature_service_report);

@interface TemperatureService: Object

+ (const struct zbus_channel *)invokeChannel;

+ (const struct zbus_channel *)reportChannel;

+ (int)requestTemperature:(k_timeout_t)timeout;

+ (int)requestTemperatureWithRef:(int *)temperature andTimeout:(k_timeout_t)timeout;

@end
