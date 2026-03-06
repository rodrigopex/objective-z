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
    OZHelper *h1 = [[OZHelper alloc] initWithValue:42 andHelper:nil];
    OZHelper *h2 = [[OZHelper alloc] initWithValue:43 andHelper:h1];
    OZHelper *h3 = [[OZHelper alloc] initWithValue:44 andHelper:h2];
    OZHelper *h4 = [[OZHelper alloc] initWithValue:45 andHelper:h3];
    OZHelper *h5 = [[OZHelper alloc] initWithValue:46 andHelper:h4];
    OZHelper *h6 = [[OZHelper alloc] initWithValue:47 andHelper:h5];

    [led setHelper:h6];

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
