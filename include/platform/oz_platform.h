/* Platform Abstraction Layer — ifdef router */
#ifndef OZ_PLATFORM_H
#define OZ_PLATFORM_H

#include "oz_platform_types.h"

#ifdef OZ_PLATFORM_ZEPHYR
#include "oz_platform_zephyr.h"
#elif defined(OZ_PLATFORM_HOST)
#include "oz_platform_host.h"
#else
#error "Define OZ_PLATFORM_ZEPHYR or OZ_PLATFORM_HOST"
#endif

#endif /* OZ_PLATFORM_H */
