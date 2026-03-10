/**
 * @file objc.h
 * @brief Defines the Objective-C runtime.
 * @defgroup objc Objective-C Runtime
 *
 * Pure C runtime interface: class introspection, protocols and resolution
 * of selectors to the methods. For Foundation classes (Object, OZString, etc.),
 * use #import <Foundation/Foundation.h>.
 */
#pragma once

#include "assert.h"
#include "malloc.h"
#include "mutex.h"
#include "runtime.h"

