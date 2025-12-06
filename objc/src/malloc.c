#include "objc/malloc.h"
#include <zephyr/kernel.h>
/*
 ** Allocate memory
 */
void *objc_malloc(size_t size) { return k_malloc(size); }

/*
 ** Free allocated memory
 */
void objc_free(void *ptr) { return k_free(ptr); }
