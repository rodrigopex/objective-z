/* SPDX-License-Identifier: Apache-2.0 */
/* Behavior test base header for Foundation class tests.
 * Imports OZObject + Foundation class headers and implementations
 * so Clang produces a complete AST for the transpiler. */

#import <Foundation/OZObject.h>
#import <Foundation/OZString.h>
#import <Foundation/OZNumber.h>
#import <Foundation/OZArray.h>
#import <Foundation/OZDictionary.h>

#import "OZObject.m"
#import "OZString.m"
#import "OZNumber.m"
#import "OZArray.m"
#import "OZDictionary.m"
