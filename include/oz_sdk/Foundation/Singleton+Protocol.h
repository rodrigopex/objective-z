/**
 * @file Singleton+Protocol.h
 * @brief Protocol for OZ singleton classes.
 *
 * Classes conforming to SingletonProtocol are created once via
 * +initialize (auto-called before main via SYS_INIT) and accessed
 * through +sharedInstance.  Singleton objects are immortal — they
 * are never deallocated.
 */
#pragma once

#import "OZObject.h"

@protocol SingletonProtocol
@required
+ (void)initialize;
+ (instancetype)sharedInstance;
@end
