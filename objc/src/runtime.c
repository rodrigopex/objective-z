#include <objc/malloc.h>
#include <zephyr/init.h>

#include <zephyr/logging/log.h>
LOG_MODULE_REGISTER(objz, CONFIG_OBJZ_LOG_LEVEL);

int objz_init(void)
{
	objc_heap_init();

	return 0;
}

SYS_INIT(objz_init, APPLICATION, 99);
