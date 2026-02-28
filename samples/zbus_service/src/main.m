/*
 * Copyright (c) 2012-2014 Wind River Systems, Inc.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#import <Foundation/Foundation.h>
#import <objc/objc.h>
#include <zephyr/kernel.h>
#include "TemperatureService.h"

void print_temp_callback(const struct zbus_channel *chan)
{
	const struct msg_temperature_service_report *report = zbus_chan_const_msg(chan);

	if (chan != [TemperatureService reportChannel]) {
		return;
	}

	if (report->tag == TEMPERATURE_SERVICE_REPORT_ERROR) {
		OZLog(" + [listener] Could not read the temperature");
		return;
	}

	OZLog(" + [listener] Temperature: %d", report->temperature.value);
}

ZBUS_LISTENER_DEFINE(lis_print_temp, print_temp_callback);

ZBUS_CHAN_ADD_OBS(chan_temperature_service_report, lis_print_temp, 3);

int main(void)
{
	int ret, temp;

	while (1) {
		OZString *str = @"Requesting temperature";

		OZLog("%s:", str.cStr);

		ret = [TemperatureService requestTemperatureWithRef:&temp andTimeout:K_SECONDS(6)];

		if (ret < 0) {
			OZLog(" + [main] Could not read the temperature");
		} else {
			OZLog(" + [main] Temperature: %d", temp);
		}

		k_msleep(1000);
	}

	return 0;
}
