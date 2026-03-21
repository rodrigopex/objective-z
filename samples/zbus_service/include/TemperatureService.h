#include <Foundation/Foundation.h>
#include <objc/objc.h>
#include <zephyr/zbus/zbus.h>

#pragma once

struct msg_temperature_service_invoke {
    int context;
	enum {
		TEMPERATURE_SERVICE_INVOKE_REQ_TEMP,
	} tag;
};

struct msg_temperature_service_report {
    int context;
    uint64_t timestamp;
	enum {
		TEMPERATURE_SERVICE_REPORT_TEMPERATURE,
		TEMPERATURE_SERVICE_REPORT_ERROR,
	} tag;
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

@interface TemperatureService: Object <SingletonProtocol>

@property (nonatomic, readonly, unsafe_unretained) const struct zbus_channel *invokeChannel;
@property (nonatomic, readonly, unsafe_unretained) const struct zbus_channel *reportChannel;

+(instancetype)sharedInstance;

- (int)requestTemperature:(k_timeout_t)timeout;

- (int)requestTemperatureWithRef:(int *)temperature andTimeout:(k_timeout_t)timeout;

- (int)requestTemperatureWithBlock:(void (^)(int error_code,int temperature))block  andTimeout:(k_timeout_t)timeout;

@end
