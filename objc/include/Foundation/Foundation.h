/**
 * @file Foundation.h
 * @brief Foundation framework umbrella header.
 *
 * Imports all Foundation classes: Object, OZString, OZMutableString,
 * OZAutoreleasePool, OZLog, and (when enabled) collection/literal classes.
 */
#pragma once

#include "OZLog.h"

#if __OBJC__

#include "Object+Protocol.h"
#include "Object.h"
#include "OZString+Protocol.h"
#include "OZString.h"
#include "OZAutoreleasePool.h"
#include "OZMutableString.h"
#include "Protocol.h"

#ifdef CONFIG_OBJZ_COLLECTIONS
#include "NSFastEnumeration.h"
#include "OZArray.h"
#include "OZDictionary.h"
#endif

#ifdef CONFIG_OBJZ_NUMBERS
#include "OZNumber.h"
#endif

#endif /* __OBJC__ */
