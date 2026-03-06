/* Transpiled LED demo — ObjC source, transpiled to pure C. */

#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>

#import "OZLed.h"

int main(void)
{
    OZLed *led = [[OZLed alloc] initWithPin:13];
    if (!led) {
        printk("Failed to allocate OZLed\n");
        return 1;
    }

    OZHelper *h = [[OZHelper alloc] initWithValue:42];
    [led setHelper:h];

    printk("LED on pin %d, state=%d\n", [led pin], [led state]);

    [led turnOn];
    printk("After turnOn: state=%d\n", [led state]);

    [led toggle];
    printk("After toggle: state=%d\n", [led state]);

    [led toggle];
    printk("After toggle: state=%d\n", [led state]);

    printk("Transpiled LED demo complete\n");
    return 0;
}
