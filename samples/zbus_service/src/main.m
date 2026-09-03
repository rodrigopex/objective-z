/*
 * Copyright (c) 2012-2014 Wind River Systems, Inc.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

#import <Foundation/Foundation.h>
#import <objc/objc.h>
#include <zephyr/kernel.h>
#include "TemperatureService.h"

/*
 * The listener's callback written inline, as a block (#272).
 *
 * `ZBUS_LISTENER_DEFINE` takes a `void (*)(const struct zbus_channel *)`,
 * and Objective-C refuses block-to-function-pointer conversion in every
 * position, so it cannot be written directly -- Clang rejects the file, and
 * Clang has to parse it for the AST oracle. `OZM` is the escape: its
 * arguments are discarded unparsed on the Objective-C side, while oz_static
 * rewrites the invocation into the real macro for the C compiler, by which
 * point the literal has become the name of a hoisted function. See
 * include/oz_sdk/Foundation/OZMacro.h.
 *
 * `ZBUS_CHAN_ADD_OBS` goes through OZM as well, because it names
 * `lis_print_temp` -- a symbol the discarded definition leaves invisible to
 * Clang. Both being discarded together is what keeps the pair consistent.
 *
 * A hoisted block captures nothing, which costs this callback nothing: it
 * already reached its context through zbus's own channel argument.
 */
OZM(ZBUS_LISTENER_DEFINE, lis_print_temp, ^(const struct zbus_channel *chan) {
	const struct msg_temperature_service_report *report = zbus_chan_const_msg(chan);

	if (chan != [[TemperatureService sharedInstance] reportChannel]) {
		return;
	}

	if (report->tag == TEMPERATURE_SERVICE_REPORT_ERROR) {
		OZLog(" + [listener] Could not read the temperature");
		return;
	}

	OZLog(" + [listener] Temperature: %d", report->temperature.value);
});

OZM(ZBUS_CHAN_ADD_OBS, chan_temperature_service_report, lis_print_temp, 3);

int main(void)
{
	int ret, temp;

	while (1) {
		OZString *str = @"Requesting temperature";

		OZLog("%s:", str.cString);

		ret = [[TemperatureService sharedInstance] requestTemperatureWithRef:&temp andTimeout:K_SECONDS(6)];

		if (ret < 0) {
			OZLog(" + [main] Could not read the temperature");
		} else {
			OZLog(" + [main] Temperature: %d", temp);
		}

		[[TemperatureService sharedInstance] requestTemperatureWithBlock:^(int ret, int temp) {
			if (ret < 0) {
				OZLog(" + [block] Could not read the temperature");
			} else {
				OZLog(" + [block] Temperature: %d", temp);
			}
		} andTimeout:K_SECONDS(6)];

		k_msleep(1000);
	}

	return 0;
}
